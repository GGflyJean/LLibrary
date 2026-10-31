use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use cron::Schedule;
use tokio::sync::{mpsc, Mutex};

use crate::covers::CoverStore;
use crate::crypto::SecretBox;
use crate::db::{Db, NewFile};
use crate::metadata;
use crate::models::{BookMetadata, ScanProgress, ScanReport, ScanStats, SourceConfig, SourceKind};
use crate::sources::SourceBackend;

#[derive(Debug, Clone)]
pub enum ScanCommand {
    All,
    Source(i64),
}

#[derive(Clone)]
pub struct Scanner {
    db: Db,
    covers: CoverStore,
    secret: Option<String>,
    progress: Arc<RwLock<Option<ScanProgress>>>,
    scan_lock: Arc<Mutex<()>>,
    active_scans: Arc<AtomicUsize>,
    max_active_scans: Arc<AtomicUsize>,
}

struct ActiveScan {
    active: Arc<AtomicUsize>,
}

impl ActiveScan {
    fn new(active: Arc<AtomicUsize>, max: Arc<AtomicUsize>) -> Self {
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        max.fetch_max(current, Ordering::SeqCst);
        Self { active }
    }
}

impl Drop for ActiveScan {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Scanner {
    pub fn new(db: Db, covers: CoverStore, secret: Option<String>) -> Self {
        Self {
            db,
            covers,
            secret,
            progress: Arc::new(RwLock::new(None)),
            scan_lock: Arc::new(Mutex::new(())),
            active_scans: Arc::new(AtomicUsize::new(0)),
            max_active_scans: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn max_concurrent_scans(&self) -> usize {
        self.max_active_scans.load(Ordering::SeqCst)
    }

    pub fn progress(&self) -> Option<ScanProgress> {
        self.progress.read().ok().and_then(|value| value.clone())
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub async fn scan_all(&self) -> Vec<ScanReport> {
        let sources = match self.db.list_sources() {
            Ok(sources) => sources,
            Err(error) => {
                tracing::error!(%error, "无法读取来源列表");
                return vec![ScanReport {
                    source_id: 0,
                    source_name: "all".to_string(),
                    stats: ScanStats::default(),
                    error: Some(error.to_string()),
                }];
            }
        };
        let mut reports = Vec::new();
        for source in sources.into_iter().filter(|source| source.enabled) {
            match self.scan_source(source.id).await {
                Ok(report) => reports.push(report),
                Err(error) => reports.push(ScanReport {
                    source_id: source.id,
                    source_name: source.name,
                    stats: ScanStats::default(),
                    error: Some(error.to_string()),
                }),
            }
        }
        reports
    }

    pub async fn scan_source(&self, source_id: i64) -> Result<ScanReport> {
        let _lock = self.scan_lock.lock().await;
        let _active = ActiveScan::new(self.active_scans.clone(), self.max_active_scans.clone());
        let config = self.load_source_config(source_id)?;
        let source = config.record.clone();
        let run_id = self.db.start_scan(source_id)?;
        let started = Instant::now();
        let mut stats = ScanStats::default();
        self.set_progress(Some(ScanProgress {
            run_id,
            source_id,
            source_name: source.name.clone(),
            phase: "listing".to_string(),
            processed: 0,
            total: 0,
            stats: stats.clone(),
        }));

        let backend = match SourceBackend::build(&config) {
            Ok(backend) => backend,
            Err(error) => {
                stats.errors += 1;
                stats.duration_ms = started.elapsed().as_millis();
                self.db
                    .finish_scan(run_id, "failed", &stats, Some(&error.to_string()))?;
                self.set_progress(None);
                return Ok(ScanReport {
                    source_id,
                    source_name: source.name,
                    stats,
                    error: Some(error.to_string()),
                });
            }
        };

        let entries = match backend.list().await {
            Ok(entries) => entries,
            Err(error) => {
                stats.errors += 1;
                stats.duration_ms = started.elapsed().as_millis();
                self.db
                    .finish_scan(run_id, "failed", &stats, Some(&error.to_string()))?;
                self.set_progress(None);
                return Ok(ScanReport {
                    source_id,
                    source_name: source.name,
                    stats,
                    error: Some(error.to_string()),
                });
            }
        };

        let mut files: Vec<_> = entries
            .into_iter()
            .filter(|entry| !entry.is_dir && metadata::is_supported(&entry.rel_path))
            .collect();
        files.sort_by_key(|entry| format_priority(&entry.rel_path));
        stats.files_seen = files.len();
        let total = files.len();
        let mut seen = HashSet::new();
        let mut errors: Vec<String> = Vec::new();

        for (index, entry) in files.iter().enumerate() {
            seen.insert(entry.rel_path.clone());
            self.set_progress(Some(ScanProgress {
                run_id,
                source_id,
                source_name: source.name.clone(),
                phase: "scanning".to_string(),
                processed: index,
                total,
                stats: stats.clone(),
            }));

            let existing = self.db.find_file(source_id, &entry.rel_path)?;
            if let Some(existing) = &existing {
                let hash_ok = source.hash_policy != "hash" || !existing.hash.is_empty();
                if !existing.missing
                    && existing.size == entry.size as i64
                    && existing.mtime == entry.mtime
                    && hash_ok
                {
                    stats.skipped += 1;
                    continue;
                }
            }

            let bytes = match backend.read(&entry.rel_path).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    stats.errors += 1;
                    errors.push(format!("{}: {error}", entry.rel_path));
                    if let Some(existing) = &existing {
                        self.db.set_file_error(existing.id, &error.to_string())?;
                    }
                    continue;
                }
            };

            let hash = if source.hash_policy == "hash" {
                blake3::hash(&bytes).to_hex().to_string()
            } else {
                String::new()
            };
            let format = metadata::extension_of(&entry.rel_path);
            let fallback = std::panic::catch_unwind(|| metadata::parse_filename(&entry.rel_path))
                .unwrap_or_else(|_| metadata::FilenameInfo {
                    title: metadata::basename_of(&entry.rel_path),
                    author: String::new(),
                });
            let extraction = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                metadata::extract(&format, &bytes, &entry.rel_path)
            }));
            let (mut book, cover, page_count, parse_error) = match extraction {
                Ok(Ok(extracted)) => (
                    extracted.metadata,
                    extracted.cover,
                    extracted.page_count,
                    None,
                ),
                Ok(Err(error)) => {
                    stats.errors += 1;
                    errors.push(format!("{}: {error}", entry.rel_path));
                    (
                        BookMetadata {
                            title: fallback.title.clone(),
                            authors: fallback.author.clone(),
                            ..Default::default()
                        },
                        None,
                        0,
                        Some(error.to_string()),
                    )
                }
                Err(_) => {
                    stats.errors += 1;
                    errors.push(format!("{}: 元数据解析 panic", entry.rel_path));
                    (
                        BookMetadata {
                            title: fallback.title.clone(),
                            authors: fallback.author.clone(),
                            ..Default::default()
                        },
                        None,
                        0,
                        Some("元数据解析 panic".to_string()),
                    )
                }
            };
            if book.title.trim().is_empty() {
                book.title = fallback.title.clone();
            }
            if book.authors.trim().is_empty() {
                book.authors = fallback.author.clone();
            }
            if book.publisher.trim().is_empty() {
                book.publisher = publisher_from_path(&entry.rel_path);
            }
            let basename = metadata::basename_of(&entry.rel_path);
            let work_id = self.find_or_create_work(&book, &basename)?;

            let update = if format == "epub" {
                book.clone()
            } else {
                BookMetadata::default()
            };
            self.db.update_work_metadata(work_id, &update, None)?;

            let cover_url = if format == "epub" {
                match cover {
                    Some(bytes) => self.covers.save_epub_cover(work_id, &bytes)?,
                    None => None,
                }
            } else {
                None
            };
            if let Some(cover_url) = cover_url {
                self.db.set_work_cover(work_id, &cover_url)?;
            } else if !self.covers.path_for(work_id).exists() {
                let placeholder = self.covers.ensure_placeholder(work_id)?;
                self.db.set_work_cover(work_id, &placeholder)?;
            }

            self.db.upsert_file(&NewFile {
                work_id,
                source_id,
                rel_path: entry.rel_path.clone(),
                basename,
                format,
                size: entry.size as i64,
                mtime: entry.mtime,
                hash,
                page_count,
                error: parse_error,
            })?;

            if existing.is_some() {
                stats.changed += 1;
            } else {
                stats.new += 1;
            }
        }

        stats.missing = self.db.mark_missing(source_id, &seen)?;
        stats.duration_ms = started.elapsed().as_millis();
        let status = if stats.errors > 0 {
            "completed_with_errors"
        } else {
            "completed"
        };
        let error_summary = if errors.is_empty() {
            None
        } else {
            Some(errors.join("\n"))
        };
        self.db
            .finish_scan(run_id, status, &stats, error_summary.as_deref())?;
        self.set_progress(None);

        Ok(ScanReport {
            source_id,
            source_name: source.name,
            stats,
            error: error_summary,
        })
    }

    fn find_or_create_work(&self, book: &BookMetadata, basename: &str) -> Result<i64> {
        if let Some(work_id) = self.db.find_work_id_by_basename(basename)? {
            return Ok(work_id);
        }
        let key = metadata::work_key(&book.title, &book.authors);
        if let Some(work_id) = self.db.find_work_by_key(&key)? {
            return Ok(work_id);
        }
        self.db.insert_work(&key, book, None)
    }

    fn load_source_config(&self, source_id: i64) -> Result<SourceConfig> {
        let record = self
            .db
            .get_source(source_id)?
            .with_context(|| format!("来源不存在: {source_id}"))?;
        let credentials = match self.db.get_source_credentials(source_id)? {
            Some(encrypted) => {
                if record.kind == SourceKind::WebDav {
                    let secret = self.secret.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("未配置 LLIBRARY_SECRET，无法解密 WebDAV 凭据")
                    })?;
                    let credentials = SecretBox::new(secret).decode_credentials(&encrypted)?;
                    Some(credentials)
                } else {
                    None
                }
            }
            None => None,
        };
        Ok(SourceConfig {
            record,
            credentials,
        })
    }

    fn set_progress(&self, progress: Option<ScanProgress>) {
        if let Ok(mut guard) = self.progress.write() {
            *guard = progress;
        }
    }
}

#[derive(Clone)]
pub struct ScannerHandle {
    sender: mpsc::Sender<ScanCommand>,
    scanner: Scanner,
}

impl ScannerHandle {
    pub fn spawn(scanner: Scanner) -> Self {
        let (sender, mut receiver) = mpsc::channel(16);
        let worker = scanner.clone();
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    ScanCommand::All => {
                        let reports = worker.scan_all().await;
                        for report in reports {
                            if let Some(error) = report.error {
                                tracing::warn!(source = %report.source_name, %error, "扫描完成但存在错误");
                            }
                        }
                    }
                    ScanCommand::Source(id) => {
                        if let Err(error) = worker.scan_source(id).await {
                            tracing::warn!(source_id = id, %error, "扫描失败");
                        }
                    }
                }
            }
        });
        Self { sender, scanner }
    }

    pub async fn enqueue_all(&self) -> Result<()> {
        self.sender
            .send(ScanCommand::All)
            .await
            .map_err(|_| anyhow::anyhow!("扫描队列已关闭"))
    }

    pub async fn enqueue_source(&self, id: i64) -> Result<()> {
        self.sender
            .send(ScanCommand::Source(id))
            .await
            .map_err(|_| anyhow::anyhow!("扫描队列已关闭"))
    }

    pub fn progress(&self) -> Option<ScanProgress> {
        self.scanner.progress()
    }

    pub fn db(&self) -> &Db {
        self.scanner.db()
    }

    pub async fn scan_source_now(&self, id: i64) -> Result<ScanReport> {
        self.scanner.scan_source(id).await
    }
}

fn parse_schedule(expression: &str) -> Option<Schedule> {
    Schedule::from_str(expression).ok().or_else(|| {
        if expression.split_whitespace().count() == 5 {
            Schedule::from_str(&format!("0 {expression}")).ok()
        } else {
            None
        }
    })
}

pub fn spawn_cron(handle: ScannerHandle, expression: &str) -> Option<tokio::task::JoinHandle<()>> {
    let schedule = parse_schedule(expression)?;
    Some(tokio::spawn(async move {
        while let Some(next) = schedule.upcoming(Utc).next() {
            let now = Utc::now();
            if let Ok(wait) = (next - now).to_std() {
                tokio::time::sleep(wait).await;
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
            if handle.enqueue_all().await.is_err() {
                break;
            }
        }
    }))
}

pub fn spawn_source_cron(
    handle: ScannerHandle,
    source_id: i64,
    expression: &str,
) -> Option<tokio::task::JoinHandle<()>> {
    let schedule = parse_schedule(expression)?;
    Some(tokio::spawn(async move {
        while let Some(next) = schedule.upcoming(Utc).next() {
            let now = Utc::now();
            if let Ok(wait) = (next - now).to_std() {
                tokio::time::sleep(wait).await;
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
            if handle.enqueue_source(source_id).await.is_err() {
                break;
            }
        }
    }))
}

fn format_priority(rel_path: &str) -> u8 {
    match metadata::extension_of(rel_path).as_str() {
        "epub" => 0,
        "azw3" => 1,
        "azw" => 2,
        "mobi" => 3,
        "pdf" => 4,
        _ => 5,
    }
}

fn publisher_from_path(rel_path: &str) -> String {
    let mut parts = rel_path.split('/');
    let first = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        first.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WorkFilter;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn epub_bytes(title: &str, author: &str) -> Vec<u8> {
        let mut cover = Vec::new();
        let image: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            image::ImageBuffer::from_pixel(20, 30, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut cover), image::ImageFormat::Png)
            .unwrap();

        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", options).unwrap();
        let opf = format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
    <dc:publisher>测试出版社</dc:publisher>
    <dc:identifier>urn:isbn:9787000000000</dc:identifier>
    <dc:language>zh</dc:language>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="cover-img" href="images/cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine/>
</package>"#
        );
        zip.write_all(opf.as_bytes()).unwrap();
        zip.start_file("OEBPS/images/cover.png", options).unwrap();
        zip.write_all(&cover).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn pdf_bytes(page_count: usize) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object};
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..page_count {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            kids.push(page_id.into());
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => page_count as i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut output = Vec::new();
        document.save_to(&mut output).unwrap();
        output
    }

    fn setup() -> (tempfile::TempDir, std::path::PathBuf, Db, i64, Scanner) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("library");
        std::fs::create_dir_all(root.join("社/批1")).unwrap();
        std::fs::create_dir_all(root.join("社/批2")).unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let source = db
            .insert_source(
                "测试来源",
                SourceKind::Local,
                root.to_str().unwrap(),
                None,
                true,
                true,
                None,
                "hash",
            )
            .unwrap();
        let covers = CoverStore::new(dir.path().join("covers")).unwrap();
        let scanner = Scanner::new(db.clone(), covers, None);
        (dir, root, db, source, scanner)
    }

    #[tokio::test]
    async fn scan_groups_formats_and_duplicates() {
        let (dir, root, db, source, scanner) = setup();
        let epub = epub_bytes("书名", "作者");
        std::fs::write(root.join("社/批1/书名.epub"), &epub).unwrap();
        std::fs::write(root.join("社/批1/书名.azw3"), b"dummy azw3").unwrap();
        std::fs::write(root.join("社/批2/书名.epub"), &epub).unwrap();
        std::fs::write(root.join("社/批2/另一本.pdf"), pdf_bytes(1)).unwrap();
        std::fs::write(root.join("社/批2/ignore.dmg"), b"ignore").unwrap();

        let report = scanner.scan_source(source).await.expect("scan");
        assert_eq!(report.stats.files_seen, 4);
        assert_eq!(report.stats.new, 4);
        assert_eq!(report.stats.errors, 0);

        let works = db.list_works(&WorkFilter::default(), 100, 0).unwrap();
        assert_eq!(works.len(), 2);
        let book = works.iter().find(|work| work.title == "书名").unwrap();
        let files = db.list_files(book.id).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files.iter().filter(|file| file.is_duplicate).count(), 1);
        assert!(book.formats.contains(&"epub".to_string()));
        assert!(book.formats.contains(&"azw3".to_string()));
        assert!(book
            .cover_path
            .as_deref()
            .unwrap_or("")
            .starts_with("/covers/"));

        let other = works.iter().find(|work| work.title == "另一本").unwrap();
        assert_eq!(other.publisher, "社");
        assert!(other.cover_path.is_some());

        let runs = db.latest_scan_runs(1).unwrap();
        assert_eq!(runs[0].status, "completed");
        assert_eq!(runs[0].stats.new, 4);
        drop(dir);
    }

    #[tokio::test]
    async fn incremental_scan_skips_unchanged_then_detects_changes_and_missing() {
        let (dir, root, db, source, scanner) = setup();
        let epub = epub_bytes("书名", "作者");
        std::fs::write(root.join("社/批1/书名.epub"), &epub).unwrap();
        std::fs::write(root.join("社/批1/书名.azw3"), b"dummy azw3").unwrap();
        std::fs::write(root.join("社/批2/另一本.pdf"), pdf_bytes(1)).unwrap();

        let first = scanner.scan_source(source).await.expect("first");
        assert_eq!(first.stats.new, 3);

        let second = scanner.scan_source(source).await.expect("second");
        assert_eq!(second.stats.new, 0);
        assert_eq!(second.stats.changed, 0);
        assert_eq!(second.stats.skipped, 3);

        std::fs::write(root.join("社/批2/另一本.pdf"), pdf_bytes(2)).unwrap();
        let third = scanner.scan_source(source).await.expect("third");
        assert_eq!(third.stats.changed, 1);
        assert_eq!(third.stats.skipped, 2);

        std::fs::remove_file(root.join("社/批1/书名.azw3")).unwrap();
        let fourth = scanner.scan_source(source).await.expect("fourth");
        assert_eq!(fourth.stats.missing, 1);

        std::fs::write(root.join("社/批1/书名.azw3"), b"dummy azw3").unwrap();
        let fifth = scanner.scan_source(source).await.expect("fifth");
        assert_eq!(fifth.stats.changed, 1);
        assert_eq!(fifth.stats.missing, 0);

        let works = db.list_works(&WorkFilter::default(), 100, 0).unwrap();
        assert_eq!(works.len(), 2);
        drop(dir);
    }

    #[tokio::test]
    async fn scan_all_isolates_failing_source() {
        let (dir, root, db, _good_source, scanner) = setup();
        std::fs::write(root.join("社/批1/书名.epub"), epub_bytes("书名", "作者")).unwrap();
        let bad_source = db
            .insert_source(
                "坏来源",
                SourceKind::Local,
                dir.path().join("missing").to_str().unwrap(),
                None,
                true,
                true,
                None,
                "hash",
            )
            .unwrap();

        let reports = scanner.scan_all().await;
        assert_eq!(reports.len(), 2);
        let bad = reports
            .iter()
            .find(|report| report.source_id == bad_source)
            .unwrap();
        assert!(bad.error.is_some());
        let good = reports
            .iter()
            .find(|report| report.source_id != bad_source)
            .unwrap();
        assert!(good.error.is_none());
        drop(dir);
    }

    #[tokio::test]
    async fn empty_cron_is_disabled() {
        let (_dir, _root, _db, source, scanner) = setup();
        let handle = ScannerHandle::spawn(scanner);
        assert!(spawn_cron(handle.clone(), "").is_none());
        assert!(spawn_source_cron(handle.clone(), source, "").is_none());
        assert!(parse_schedule("0 3 * * *").is_some());
        assert!(parse_schedule("0 0 3 * * * *").is_some());
        handle.enqueue_source(source).await.expect("enqueue");
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use crate::models::WorkFilter;
    use crate::sources::local::LocalSource;

    #[tokio::test]
    async fn scans_are_serialized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root1 = dir.path().join("library1");
        let root2 = dir.path().join("library2");
        std::fs::create_dir_all(root1.join("社")).unwrap();
        std::fs::create_dir_all(root2.join("社")).unwrap();
        std::fs::write(root1.join("社/书一.epub"), b"not an epub").unwrap();
        std::fs::write(root2.join("社/书二.epub"), b"not an epub").unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let source1 = db
            .insert_source(
                "s1",
                SourceKind::Local,
                root1.to_str().unwrap(),
                None,
                true,
                true,
                None,
                "hash",
            )
            .unwrap();
        let source2 = db
            .insert_source(
                "s2",
                SourceKind::Local,
                root2.to_str().unwrap(),
                None,
                true,
                true,
                None,
                "hash",
            )
            .unwrap();
        let scanner = Scanner::new(
            db.clone(),
            CoverStore::new(dir.path().join("covers")).unwrap(),
            None,
        );
        let second = scanner.clone();
        let (first_report, second_report) =
            tokio::join!(scanner.scan_source(source1), second.scan_source(source2));
        assert!(first_report.is_ok());
        assert!(second_report.is_ok());
        assert!(scanner.max_concurrent_scans() <= 1);
        let works = db.list_works(&WorkFilter::default(), 10, 0).unwrap();
        assert_eq!(works.len(), 2);
        let _ = LocalSource::new(&root1);
    }
}
