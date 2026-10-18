use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::SourceEntry;

#[derive(Debug, Clone)]
pub struct LocalSource {
    root: PathBuf,
}

impl LocalSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, rel_path: &str) -> PathBuf {
        self.root.join(rel_path)
    }

    pub async fn list(&self) -> Result<Vec<SourceEntry>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || list_blocking(&root))
            .await
            .context("本地来源扫描线程失败")?
    }

    pub async fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        let path = self.resolve(rel_path);
        tokio::fs::read(&path)
            .await
            .with_context(|| format!("无法读取 {}", path.display()))
    }

    pub async fn read_range(&self, rel_path: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        if start > end {
            anyhow::bail!("无效的字节范围 {start}-{end}");
        }
        let path = self.resolve(rel_path);
        let mut file = tokio::fs::File::open(&path)
            .await
            .with_context(|| format!("无法打开 {}", path.display()))?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let length = end - start + 1;
        let mut buffer = Vec::with_capacity(length.min(8 * 1024 * 1024) as usize);
        file.take(length).read_to_end(&mut buffer).await?;
        Ok(buffer)
    }

    pub async fn open(&self, rel_path: &str) -> Result<tokio::fs::File> {
        let path = self.resolve(rel_path);
        tokio::fs::File::open(&path)
            .await
            .with_context(|| format!("无法打开 {}", path.display()))
    }
}

fn list_blocking(root: &Path) -> Result<Vec<SourceEntry>> {
    if !root.is_dir() {
        anyhow::bail!("本地来源不是目录: {}", root.display());
    }
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("遍历 {} 失败", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .with_context(|| format!("计算相对路径失败: {}", entry.path().display()))?;
        let rel_path = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        entries.push(SourceEntry {
            rel_path,
            size: metadata.len(),
            mtime,
            is_dir: false,
        });
    }
    entries.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_nested_files_and_reads_ranges() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("出版社/批次")).expect("mkdir");
        let path = dir.path().join("出版社/批次/书名 #1.epub");
        std::fs::write(&path, b"0123456789").expect("write");

        let source = LocalSource::new(dir.path());
        let entries = source.list().await.expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rel_path, "出版社/批次/书名 #1.epub");
        assert_eq!(entries[0].size, 10);
        assert!(!entries[0].is_dir);

        let all = source.read(&entries[0].rel_path).await.expect("read");
        assert_eq!(all, b"0123456789");
        let range = source
            .read_range(&entries[0].rel_path, 2, 5)
            .await
            .expect("range");
        assert_eq!(range, b"2345");
    }

    #[tokio::test]
    async fn missing_root_fails() {
        let source = LocalSource::new("/definitely/not/a/library");
        assert!(source.list().await.is_err());
    }
}
