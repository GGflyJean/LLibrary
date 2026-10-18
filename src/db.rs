use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::models::{
    BookMetadata, FileRecord, ScanRun, ScanStats, SourceKind, SourceRecord, WorkDetail, WorkFilter,
    WorkSummary,
};

#[derive(Debug, Clone)]
pub struct Db {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct NewFile {
    pub work_id: i64,
    pub source_id: i64,
    pub rel_path: String,
    pub basename: String,
    pub format: String,
    pub size: i64,
    pub mtime: i64,
    pub hash: String,
    pub page_count: i64,
    pub error: Option<String>,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Self { path };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)
            .with_context(|| format!("无法打开数据库 {}", self.path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        Ok(conn)
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                type TEXT NOT NULL CHECK(type IN ('local','webdav')),
                location TEXT NOT NULL,
                credentials_enc BLOB,
                enabled INTEGER NOT NULL DEFAULT 1,
                scan_on_add INTEGER NOT NULL DEFAULT 1,
                schedule TEXT,
                hash_policy TEXT NOT NULL DEFAULT 'hash',
                read_only INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                UNIQUE(type, location)
            );

            CREATE TABLE IF NOT EXISTS scan_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                status TEXT NOT NULL,
                stats_json TEXT NOT NULL DEFAULT '{}',
                error TEXT
            );

            CREATE TABLE IF NOT EXISTS works (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_key TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                sort_title TEXT NOT NULL,
                authors TEXT NOT NULL DEFAULT '',
                publisher TEXT NOT NULL DEFAULT '',
                isbn TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                language TEXT NOT NULL DEFAULT '',
                pub_date TEXT NOT NULL DEFAULT '',
                clc_code TEXT NOT NULL DEFAULT '',
                clc_label TEXT NOT NULL DEFAULT '',
                cover_path TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
                rel_path TEXT NOT NULL,
                basename TEXT NOT NULL,
                format TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                hash TEXT NOT NULL DEFAULT '',
                is_duplicate INTEGER NOT NULL DEFAULT 0,
                missing INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                page_count INTEGER NOT NULL DEFAULT 0,
                UNIQUE(source_id, rel_path)
            );

            CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash);
            CREATE INDEX IF NOT EXISTS idx_files_basename ON files(basename);
            CREATE INDEX IF NOT EXISTS idx_files_work ON files(work_id);
            CREATE INDEX IF NOT EXISTS idx_files_source ON files(source_id);
            CREATE INDEX IF NOT EXISTS idx_works_publisher ON works(publisher);
            CREATE INDEX IF NOT EXISTS idx_works_updated ON works(updated_at);

            CREATE VIRTUAL TABLE IF NOT EXISTS works_fts USING fts5(
                title, authors, publisher, isbn, description,
                content='works', content_rowid='id',
                tokenize='unicode61 remove_diacritics 2'
            );

            CREATE TRIGGER IF NOT EXISTS works_ai AFTER INSERT ON works BEGIN
                INSERT INTO works_fts(rowid, title, authors, publisher, isbn, description)
                VALUES (new.id, new.title, new.authors, new.publisher, new.isbn, new.description);
            END;

            CREATE TRIGGER IF NOT EXISTS works_ad AFTER DELETE ON works BEGIN
                INSERT INTO works_fts(works_fts, rowid, title, authors, publisher, isbn, description)
                VALUES ('delete', old.id, old.title, old.authors, old.publisher, old.isbn, old.description);
            END;

            CREATE TRIGGER IF NOT EXISTS works_au AFTER UPDATE ON works BEGIN
                INSERT INTO works_fts(works_fts, rowid, title, authors, publisher, isbn, description)
                VALUES ('delete', old.id, old.title, old.authors, old.publisher, old.isbn, old.description);
                INSERT INTO works_fts(rowid, title, authors, publisher, isbn, description)
                VALUES (new.id, new.title, new.authors, new.publisher, new.isbn, new.description);
            END;
            "#,
        )?;

        let mut has_read_only = false;
        {
            let mut stmt = conn.prepare("PRAGMA table_info(sources)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for name in rows {
                if name? == "read_only" {
                    has_read_only = true;
                    break;
                }
            }
        }
        if !has_read_only {
            conn.execute(
                "ALTER TABLE sources ADD COLUMN read_only INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_source(
        &self,
        name: &str,
        kind: SourceKind,
        location: &str,
        credentials_enc: Option<&[u8]>,
        enabled: bool,
        scan_on_add: bool,
        schedule: Option<&str>,
        hash_policy: &str,
    ) -> Result<i64> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO sources (name, type, location, credentials_enc, enabled, scan_on_add, schedule, hash_policy, read_only, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
            params![
                name,
                kind.as_str(),
                location,
                credentials_enc,
                enabled as i64,
                scan_on_add as i64,
                schedule,
                hash_policy,
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_sources(&self) -> Result<Vec<SourceRecord>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, type, location, enabled, scan_on_add, schedule, hash_policy, read_only
             FROM sources ORDER BY id",
        )?;
        let rows = stmt.query_map([], source_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_source(&self, id: i64) -> Result<Option<SourceRecord>> {
        let conn = self.connection()?;
        let row = conn
            .query_row(
                "SELECT id, name, type, location, enabled, scan_on_add, schedule, hash_policy, read_only
                 FROM sources WHERE id = ?1",
                params![id],
                source_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn get_source_credentials(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.connection()?;
        let value: Option<Option<Vec<u8>>> = conn
            .query_row(
                "SELECT credentials_enc FROM sources WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.flatten())
    }

    pub fn update_source_credentials(&self, id: i64, credentials_enc: Option<&[u8]>) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE sources SET credentials_enc = ?1 WHERE id = ?2",
            params![credentials_enc, id],
        )?;
        Ok(())
    }

    pub fn delete_source(&self, id: i64) -> Result<bool> {
        let conn = self.connection()?;
        let changed = conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    pub fn start_scan(&self, source_id: i64) -> Result<i64> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO scan_runs (source_id, started_at, status, stats_json) VALUES (?1, ?2, 'running', '{}')",
            params![source_id, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_scan(
        &self,
        run_id: i64,
        status: &str,
        stats: &ScanStats,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection()?;
        let stats_json = serde_json::to_string(stats)?;
        conn.execute(
            "UPDATE scan_runs SET finished_at = ?1, status = ?2, stats_json = ?3, error = ?4 WHERE id = ?5",
            params![now(), status, stats_json, error, run_id],
        )?;
        Ok(())
    }

    pub fn get_scan_run(&self, id: i64) -> Result<Option<ScanRun>> {
        let conn = self.connection()?;
        let row = conn
            .query_row(
                "SELECT id, source_id, started_at, finished_at, status, stats_json, error
                 FROM scan_runs WHERE id = ?1",
                params![id],
                scan_run_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn latest_scan_runs(&self, limit: i64) -> Result<Vec<ScanRun>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, started_at, finished_at, status, stats_json, error
             FROM scan_runs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], scan_run_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn find_file(&self, source_id: i64, rel_path: &str) -> Result<Option<FileRecord>> {
        let conn = self.connection()?;
        let row = conn
            .query_row(
                "SELECT id, work_id, source_id, rel_path, basename, format, size, mtime, hash,
                        is_duplicate, missing, error, page_count
                 FROM files WHERE source_id = ?1 AND rel_path = ?2",
                params![source_id, rel_path],
                file_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn get_file(&self, id: i64) -> Result<Option<FileRecord>> {
        let conn = self.connection()?;
        let row = conn
            .query_row(
                "SELECT id, work_id, source_id, rel_path, basename, format, size, mtime, hash,
                        is_duplicate, missing, error, page_count
                 FROM files WHERE id = ?1",
                params![id],
                file_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_files(&self, work_id: i64) -> Result<Vec<FileRecord>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, work_id, source_id, rel_path, basename, format, size, mtime, hash,
                    is_duplicate, missing, error, page_count
             FROM files WHERE work_id = ?1 ORDER BY missing ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![work_id], file_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn find_work_id_by_basename(&self, basename: &str) -> Result<Option<i64>> {
        let conn = self.connection()?;
        let id = conn
            .query_row(
                "SELECT work_id FROM files WHERE basename = ?1 ORDER BY missing ASC, id ASC LIMIT 1",
                params![basename],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    pub fn find_work_by_key(&self, key: &str) -> Result<Option<i64>> {
        let conn = self.connection()?;
        let id = conn
            .query_row(
                "SELECT id FROM works WHERE work_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    pub fn insert_work(
        &self,
        work_key: &str,
        metadata: &BookMetadata,
        cover_path: Option<&str>,
    ) -> Result<i64> {
        let conn = self.connection()?;
        let timestamp = now();
        conn.execute(
            "INSERT INTO works (work_key, title, sort_title, authors, publisher, isbn, description,
                                language, pub_date, cover_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                work_key,
                metadata.title,
                sort_key(&metadata.title),
                metadata.authors,
                metadata.publisher,
                metadata.isbn,
                metadata.description,
                metadata.language,
                metadata.pub_date,
                cover_path,
                timestamp,
                timestamp,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_work_metadata(
        &self,
        id: i64,
        metadata: &BookMetadata,
        cover_path: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE works SET
                title = CASE WHEN ?1 <> '' THEN ?1 ELSE title END,
                sort_title = CASE WHEN ?1 <> '' THEN ?2 ELSE sort_title END,
                authors = CASE WHEN ?3 <> '' THEN ?3 ELSE authors END,
                publisher = CASE WHEN ?4 <> '' THEN ?4 ELSE publisher END,
                isbn = CASE WHEN ?5 <> '' THEN ?5 ELSE isbn END,
                description = CASE WHEN ?6 <> '' THEN ?6 ELSE description END,
                language = CASE WHEN ?7 <> '' THEN ?7 ELSE language END,
                pub_date = CASE WHEN ?8 <> '' THEN ?8 ELSE pub_date END,
                cover_path = COALESCE(?9, cover_path),
                updated_at = ?10
             WHERE id = ?11",
            params![
                metadata.title,
                sort_key(&metadata.title),
                metadata.authors,
                metadata.publisher,
                metadata.isbn,
                metadata.description,
                metadata.language,
                metadata.pub_date,
                cover_path,
                now(),
                id,
            ],
        )?;
        Ok(())
    }

    pub fn set_work_cover(&self, id: i64, cover_path: &str) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE works SET cover_path = ?1, updated_at = ?2 WHERE id = ?3",
            params![cover_path, now(), id],
        )?;
        Ok(())
    }

    pub fn upsert_file(&self, file: &NewFile) -> Result<i64> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO files (work_id, source_id, rel_path, basename, format, size, mtime, hash,
                                is_duplicate, missing, error, page_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?10)
             ON CONFLICT(source_id, rel_path) DO UPDATE SET
                work_id = excluded.work_id,
                basename = excluded.basename,
                format = excluded.format,
                size = excluded.size,
                mtime = excluded.mtime,
                hash = excluded.hash,
                missing = 0,
                error = excluded.error,
                page_count = excluded.page_count",
            params![
                file.work_id,
                file.source_id,
                file.rel_path,
                file.basename,
                file.format,
                file.size,
                file.mtime,
                file.hash,
                file.error,
                file.page_count,
            ],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM files WHERE source_id = ?1 AND rel_path = ?2",
            params![file.source_id, file.rel_path],
            |row| row.get(0),
        )?;
        self.refresh_duplicate(&conn, id)?;
        Ok(id)
    }

    fn refresh_duplicate(&self, conn: &Connection, file_id: i64) -> Result<()> {
        let (hash, work_id): (String, i64) = conn.query_row(
            "SELECT hash, work_id FROM files WHERE id = ?1",
            params![file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let duplicate = if hash.is_empty() {
            false
        } else {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE hash = ?1 AND id <> ?2 AND work_id = ?3)",
                params![hash, file_id, work_id],
                |row| row.get::<_, i64>(0),
            )? > 0
        };
        conn.execute(
            "UPDATE files SET is_duplicate = ?1 WHERE id = ?2",
            params![duplicate as i64, file_id],
        )?;
        Ok(())
    }

    pub fn mark_missing(&self, source_id: i64, seen: &HashSet<String>) -> Result<usize> {
        let conn = self.connection()?;
        let mut stmt =
            conn.prepare("SELECT id, rel_path FROM files WHERE source_id = ?1 AND missing = 0")?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut missing_ids = Vec::new();
        for row in rows {
            let (id, rel_path) = row?;
            if !seen.contains(&rel_path) {
                missing_ids.push(id);
            }
        }
        for id in &missing_ids {
            conn.execute("UPDATE files SET missing = 1 WHERE id = ?1", params![id])?;
        }
        Ok(missing_ids.len())
    }

    pub fn list_works(
        &self,
        filter: &WorkFilter,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<WorkSummary>> {
        let conn = self.connection()?;
        let (where_sql, values) = build_work_where(filter);
        let sql = format!(
            "SELECT w.id, w.title, w.authors, w.publisher, w.isbn, w.description, w.language,
                    w.pub_date, w.cover_path,
                    (SELECT GROUP_CONCAT(DISTINCT f.format) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                    (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id),
                    (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                    (SELECT MAX(f.is_duplicate) FROM files f WHERE f.work_id = w.id)
             FROM works w
             {where_sql}
             ORDER BY w.sort_title, w.id
             LIMIT ? OFFSET ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut all_values = values;
        all_values.push(Value::Integer(limit));
        all_values.push(Value::Integer(offset));
        let rows = stmt.query_map(params_from_iter(all_values.iter()), work_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn count_works(&self, filter: &WorkFilter) -> Result<i64> {
        let conn = self.connection()?;
        let (where_sql, values) = build_work_where(filter);
        let sql = format!("SELECT COUNT(*) FROM works w {where_sql}");
        let count = conn.query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))?;
        Ok(count)
    }

    pub fn get_work(&self, id: i64) -> Result<Option<WorkDetail>> {
        let conn = self.connection()?;
        let work = conn
            .query_row(
                "SELECT w.id, w.title, w.authors, w.publisher, w.isbn, w.description, w.language,
                        w.pub_date, w.cover_path,
                        (SELECT GROUP_CONCAT(DISTINCT f.format) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                        (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id),
                        (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                        (SELECT MAX(f.is_duplicate) FROM files f WHERE f.work_id = w.id)
                 FROM works w WHERE w.id = ?1",
                params![id],
                work_from_row,
            )
            .optional()?;
        match work {
            Some(work) => {
                let files = self.list_files(id)?;
                Ok(Some(WorkDetail { work, files }))
            }
            None => Ok(None),
        }
    }

    pub fn publishers(&self) -> Result<Vec<String>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT publisher FROM works WHERE publisher <> '' ORDER BY publisher",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_works_opds(
        &self,
        publisher: Option<&str>,
        limit: i64,
        latest: bool,
    ) -> Result<Vec<WorkSummary>> {
        let conn = self.connection()?;
        let order = if latest {
            "w.updated_at DESC, w.id DESC"
        } else {
            "w.sort_title, w.id"
        };
        let sql = format!(
            "SELECT w.id, w.title, w.authors, w.publisher, w.isbn, w.description, w.language,
                    w.pub_date, w.cover_path,
                    (SELECT GROUP_CONCAT(DISTINCT f.format) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                    (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id),
                    (SELECT COUNT(*) FROM files f WHERE f.work_id = w.id AND f.missing = 0),
                    (SELECT MAX(f.is_duplicate) FROM files f WHERE f.work_id = w.id)
             FROM works w
             WHERE (?1 IS NULL OR w.publisher = ?1)
             ORDER BY {order}
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![publisher, limit], work_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn set_file_error(&self, id: i64, error: &str) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE files SET error = ?1, missing = 1 WHERE id = ?2",
            params![error, id],
        )?;
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn sort_key(title: &str) -> String {
    title.to_lowercase()
}

fn source_from_row(row: &Row<'_>) -> rusqlite::Result<SourceRecord> {
    let kind_raw: String = row.get(2)?;
    let kind = SourceKind::parse(&kind_raw).unwrap_or(SourceKind::Local);
    Ok(SourceRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        kind,
        location: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        scan_on_add: row.get::<_, i64>(5)? != 0,
        schedule: row.get(6)?,
        hash_policy: row.get(7)?,
        read_only: row.get::<_, i64>(8)? != 0,
    })
}

fn scan_run_from_row(row: &Row<'_>) -> rusqlite::Result<ScanRun> {
    let stats_json: String = row.get(5)?;
    let stats = serde_json::from_str(&stats_json).unwrap_or_default();
    Ok(ScanRun {
        id: row.get(0)?,
        source_id: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        status: row.get(4)?,
        stats,
        error: row.get(6)?,
    })
}

fn file_from_row(row: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        work_id: row.get(1)?,
        source_id: row.get(2)?,
        rel_path: row.get(3)?,
        basename: row.get(4)?,
        format: row.get(5)?,
        size: row.get(6)?,
        mtime: row.get(7)?,
        hash: row.get(8)?,
        is_duplicate: row.get::<_, i64>(9)? != 0,
        missing: row.get::<_, i64>(10)? != 0,
        error: row.get(11)?,
        page_count: row.get(12)?,
    })
}

fn work_from_row(row: &Row<'_>) -> rusqlite::Result<WorkSummary> {
    let formats_raw: Option<String> = row.get(9)?;
    let total: i64 = row.get(10)?;
    let available: i64 = row.get(11)?;
    let duplicate: Option<i64> = row.get(12)?;
    let formats = formats_raw
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    Ok(WorkSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        authors: row.get(2)?,
        publisher: row.get(3)?,
        isbn: row.get(4)?,
        description: row.get(5)?,
        language: row.get(6)?,
        pub_date: row.get(7)?,
        cover_path: row.get(8)?,
        formats,
        all_missing: total > 0 && available == 0,
        has_duplicate: duplicate.unwrap_or(0) > 0,
    })
}

fn build_work_where(filter: &WorkFilter) -> (String, Vec<Value>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    if let Some(query) = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        let like = format!("%{query}%");
        clauses.push(
            "(w.title LIKE ? OR w.authors LIKE ? OR w.publisher LIKE ? OR w.isbn LIKE ? OR w.description LIKE ?
              OR w.id IN (SELECT rowid FROM works_fts WHERE works_fts MATCH ?))"
                .to_string(),
        );
        for _ in 0..5 {
            values.push(Value::Text(like.clone()));
        }
        values.push(Value::Text(format!("\"{}\"", query.replace('"', ""))));
    }
    if let Some(publisher) = &filter.publisher {
        clauses.push("w.publisher = ?".to_string());
        values.push(Value::Text(publisher.clone()));
    }
    if let Some(format) = &filter.format {
        clauses.push(
            "EXISTS (SELECT 1 FROM files f WHERE f.work_id = w.id AND f.format = ?)".to_string(),
        );
        values.push(Value::Text(format.clone()));
    }
    if let Some(source_id) = filter.source_id {
        clauses.push(
            "EXISTS (SELECT 1 FROM files f WHERE f.work_id = w.id AND f.source_id = ?)".to_string(),
        );
        values.push(Value::Integer(source_id));
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (sql, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceKind;

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Db::open(dir.path().join("library.db")).expect("db");
        (dir, db)
    }

    fn metadata(title: &str, authors: &str, isbn: &str) -> BookMetadata {
        BookMetadata {
            title: title.to_string(),
            authors: authors.to_string(),
            publisher: "测试出版社".to_string(),
            isbn: isbn.to_string(),
            description: "简介".to_string(),
            language: "zh".to_string(),
            pub_date: "2024".to_string(),
        }
    }

    #[test]
    fn migration_creates_schema() {
        let (_dir, db) = temp_db();
        let conn = Connection::open(db.path()).expect("conn");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('sources','scan_runs','works','files','works_fts')",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 5);
    }

    #[test]
    fn source_crud() {
        let (_dir, db) = temp_db();
        let id = db
            .insert_source(
                "主书库",
                SourceKind::Local,
                "/books",
                None,
                true,
                true,
                None,
                "hash",
            )
            .expect("insert");
        let sources = db.list_sources().expect("list");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, id);
        assert_eq!(sources[0].kind, SourceKind::Local);
        db.delete_source(id).expect("delete");
        assert!(db.list_sources().expect("list").is_empty());
    }

    #[test]
    fn work_with_multiple_formats_and_missing() {
        let (_dir, db) = temp_db();
        let source = db
            .insert_source(
                "s",
                SourceKind::Local,
                "/books",
                None,
                true,
                true,
                None,
                "hash",
            )
            .expect("source");
        let work = db
            .insert_work("key", &metadata("书名", "作者", "9787000000000"), None)
            .expect("work");
        for (format, hash) in [("epub", "hash-a"), ("azw3", "hash-b")] {
            db.upsert_file(&NewFile {
                work_id: work,
                source_id: source,
                rel_path: format!("{format}.{format}"),
                basename: "书名".to_string(),
                format: format.to_string(),
                size: 10,
                mtime: 1,
                hash: hash.to_string(),
                page_count: 0,
                error: None,
            })
            .expect("file");
        }
        let detail = db.get_work(work).expect("detail").expect("work");
        assert_eq!(detail.files.len(), 2);
        assert_eq!(detail.work.formats.len(), 2);
        assert!(!detail.work.all_missing);

        let seen: HashSet<String> = HashSet::new();
        let missing = db.mark_missing(source, &seen).expect("mark");
        assert_eq!(missing, 2);
        let detail = db.get_work(work).expect("detail").expect("work");
        assert!(detail.work.all_missing);

        db.upsert_file(&NewFile {
            work_id: work,
            source_id: source,
            rel_path: "epub.epub".to_string(),
            basename: "书名".to_string(),
            format: "epub".to_string(),
            size: 10,
            mtime: 1,
            hash: "hash-a".to_string(),
            page_count: 0,
            error: None,
        })
        .expect("restore");
        let detail = db.get_work(work).expect("detail").expect("work");
        assert!(!detail.work.all_missing);
    }

    #[test]
    fn duplicate_is_marked_with_same_hash() {
        let (_dir, db) = temp_db();
        let source = db
            .insert_source(
                "s",
                SourceKind::Local,
                "/books",
                None,
                true,
                true,
                None,
                "hash",
            )
            .expect("source");
        let work = db
            .insert_work("key", &metadata("书名", "作者", ""), None)
            .expect("work");
        for path in ["a.epub", "b.epub"] {
            db.upsert_file(&NewFile {
                work_id: work,
                source_id: source,
                rel_path: path.to_string(),
                basename: "书名".to_string(),
                format: "epub".to_string(),
                size: 10,
                mtime: 1,
                hash: "same".to_string(),
                page_count: 0,
                error: None,
            })
            .expect("file");
        }
        let files = db.list_files(work).expect("files");
        assert_eq!(files.iter().filter(|file| file.is_duplicate).count(), 1);
    }

    #[test]
    fn search_finds_chinese_and_isbn() {
        let (_dir, db) = temp_db();
        db.insert_work("a", &metadata("论语译注", "杨伯峻", "9787101000000"), None)
            .expect("work");
        db.insert_work(
            "b",
            &metadata("Another Book", "Someone", "1234567890"),
            None,
        )
        .expect("work");

        let filter = WorkFilter {
            query: Some("论语".to_string()),
            ..Default::default()
        };
        let results = db.list_works(&filter, 20, 0).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "论语译注");

        let filter = WorkFilter {
            query: Some("9787101000000".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_works(&filter, 20, 0).expect("search").len(), 1);

        let filter = WorkFilter {
            query: Some("another".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_works(&filter, 20, 0).expect("search").len(), 1);
    }

    #[test]
    fn scan_run_roundtrip() {
        let (_dir, db) = temp_db();
        let source = db
            .insert_source(
                "s",
                SourceKind::Local,
                "/books",
                None,
                true,
                true,
                None,
                "hash",
            )
            .expect("source");
        let run = db.start_scan(source).expect("start");
        let stats = ScanStats {
            files_seen: 3,
            new: 2,
            errors: 1,
            duration_ms: 5,
            ..Default::default()
        };
        db.finish_scan(run, "completed", &stats, Some("partial"))
            .expect("finish");
        let stored = db.get_scan_run(run).expect("get").expect("run");
        assert_eq!(stored.status, "completed");
        assert_eq!(stored.stats.new, 2);
        assert_eq!(stored.error.as_deref(), Some("partial"));
    }
}
