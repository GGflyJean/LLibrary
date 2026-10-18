use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Local,
    WebDav,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::WebDav => "webdav",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "webdav" | "web_dav" => Some(Self::WebDav),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: i64,
    pub name: String,
    pub kind: SourceKind,
    pub location: String,
    pub enabled: bool,
    pub scan_on_add: bool,
    pub schedule: Option<String>,
    pub hash_policy: String,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct WebDavCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub record: SourceRecord,
    pub credentials: Option<WebDavCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanStats {
    pub files_seen: usize,
    pub new: usize,
    pub changed: usize,
    pub skipped: usize,
    pub missing: usize,
    pub errors: usize,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub source_id: i64,
    pub source_name: String,
    pub stats: ScanStats,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BookMetadata {
    pub title: String,
    pub authors: String,
    pub publisher: String,
    pub isbn: String,
    pub description: String,
    pub language: String,
    pub pub_date: String,
}

#[derive(Debug, Clone)]
pub struct WorkSummary {
    pub id: i64,
    pub title: String,
    pub authors: String,
    pub publisher: String,
    pub isbn: String,
    pub description: String,
    pub language: String,
    pub pub_date: String,
    pub cover_path: Option<String>,
    pub formats: Vec<String>,
    pub all_missing: bool,
    pub has_duplicate: bool,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: i64,
    pub work_id: i64,
    pub source_id: i64,
    pub rel_path: String,
    pub basename: String,
    pub format: String,
    pub size: i64,
    pub mtime: i64,
    pub hash: String,
    pub is_duplicate: bool,
    pub missing: bool,
    pub error: Option<String>,
    pub page_count: i64,
}

impl FileRecord {
    pub fn mime_type(&self) -> &'static str {
        mime_for_format(&self.format)
    }

    pub fn size_label(&self) -> String {
        human_size(self.size.max(0) as u64)
    }
}

impl WorkSummary {
    pub fn cover_url(&self) -> &str {
        self.cover_path
            .as_deref()
            .unwrap_or("/static/placeholder.svg")
    }

    pub fn formats_label(&self) -> String {
        if self.formats.is_empty() {
            "无可用格式".to_string()
        } else {
            self.formats.join(" · ")
        }
    }

    pub fn authors_label(&self) -> &str {
        if self.authors.trim().is_empty() {
            "未知作者"
        } else {
            &self.authors
        }
    }
}

impl ScanRun {
    pub fn finished_label(&self) -> String {
        self.finished_at.clone().unwrap_or_else(|| "-".to_string())
    }

    pub fn error_label(&self) -> String {
        self.error.clone().unwrap_or_default()
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn mime_for_format(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "epub" => "application/epub+zip",
        "pdf" => "application/pdf",
        "mobi" => "application/x-mobipocket-ebook",
        "azw" | "azw3" => "application/vnd.amazon.ebook",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone)]
pub struct WorkDetail {
    pub work: WorkSummary,
    pub files: Vec<FileRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanRun {
    pub id: i64,
    pub source_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub stats: ScanStats,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceOption {
    pub id: i64,
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    pub run_id: i64,
    pub source_id: i64,
    pub source_name: String,
    pub phase: String,
    pub processed: usize,
    pub total: usize,
    pub stats: ScanStats,
}

#[derive(Debug, Clone, Default)]
pub struct WorkFilter {
    pub query: Option<String>,
    pub publisher: Option<String>,
    pub format: Option<String>,
    pub source_id: Option<i64>,
}
