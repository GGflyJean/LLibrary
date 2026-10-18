use anyhow::Result;

use crate::models::{SourceConfig, SourceKind, WebDavCredentials};

pub mod local;
pub mod webdav;

pub use local::LocalSource;
pub use webdav::WebDavSource;

#[derive(Debug, Clone)]
pub struct SourceEntry {
    pub rel_path: String,
    pub size: u64,
    pub mtime: i64,
    pub is_dir: bool,
}

#[derive(Debug)]
pub enum SourceBackend {
    Local(LocalSource),
    WebDav(WebDavSource),
}

impl SourceBackend {
    pub fn build(config: &SourceConfig) -> Result<Self> {
        match config.record.kind {
            SourceKind::Local => Ok(Self::Local(LocalSource::new(&config.record.location))),
            SourceKind::WebDav => {
                let credentials = config.credentials.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("WebDAV 来源缺少凭据: {}", config.record.name)
                })?;
                Ok(Self::WebDav(WebDavSource::new(
                    &config.record.location,
                    credentials,
                )?))
            }
        }
    }

    pub async fn list(&self) -> Result<Vec<SourceEntry>> {
        match self {
            Self::Local(source) => source.list().await,
            Self::WebDav(source) => source.list().await,
        }
    }

    pub async fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        match self {
            Self::Local(source) => source.read(rel_path).await,
            Self::WebDav(source) => source.read(rel_path).await,
        }
    }

    pub async fn read_range(&self, rel_path: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        match self {
            Self::Local(source) => source.read_range(rel_path, start, end).await,
            Self::WebDav(source) => source.read_range(rel_path, start, end).await,
        }
    }

    pub fn local(&self) -> Option<&LocalSource> {
        match self {
            Self::Local(source) => Some(source),
            Self::WebDav(_) => None,
        }
    }

    pub fn webdav(&self) -> Option<&WebDavSource> {
        match self {
            Self::WebDav(source) => Some(source),
            Self::Local(_) => None,
        }
    }
}

pub fn credentials_from_source(config: &SourceConfig) -> Option<&WebDavCredentials> {
    config.credentials.as_ref()
}
