use std::path::PathBuf;

use anyhow::Result;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::models::{SourceConfig, SourceKind, WebDavCredentials};

pub mod local;
pub mod webdav;

pub use local::LocalSource;
pub use webdav::WebDavSource;

#[derive(Debug)]
pub struct MaterializedFile {
    pub path: PathBuf,
    temporary: bool,
}

impl Drop for MaterializedFile {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

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

    pub async fn materialize(&self, rel_path: &str) -> Result<MaterializedFile> {
        match self {
            Self::Local(source) => Ok(MaterializedFile {
                path: source.resolve(rel_path),
                temporary: false,
            }),
            Self::WebDav(source) => {
                let response = source.get(rel_path, None).await?;
                let unique = format!(
                    "llibrary-scan-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                );
                let path = std::env::temp_dir().join(unique);
                let mut file = tokio::fs::File::create(&path).await?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(chunk) => {
                            if let Err(error) = file.write_all(&chunk).await {
                                let _ = tokio::fs::remove_file(&path).await;
                                return Err(error.into());
                            }
                        }
                        Err(error) => {
                            let _ = tokio::fs::remove_file(&path).await;
                            return Err(error.into());
                        }
                    }
                }
                file.flush().await?;
                drop(file);
                Ok(MaterializedFile {
                    path,
                    temporary: true,
                })
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SourceConfig, SourceKind, SourceRecord};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn local_config(root: &std::path::Path) -> SourceConfig {
        SourceConfig {
            record: SourceRecord {
                id: 1,
                name: "local".to_string(),
                kind: SourceKind::Local,
                location: root.to_string_lossy().to_string(),
                enabled: true,
                scan_on_add: false,
                schedule: None,
                hash_policy: "hash".to_string(),
                read_only: true,
            },
            credentials: None,
        }
    }

    #[tokio::test]
    async fn local_materialize_returns_source_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("book.epub");
        std::fs::write(&file, b"epub-data").expect("write");
        let backend = SourceBackend::build(&local_config(dir.path())).expect("backend");
        let materialized = backend.materialize("book.epub").await.expect("materialize");
        assert_eq!(
            std::fs::read(&materialized.path).expect("read"),
            b"epub-data"
        );
        drop(materialized);
        assert!(file.exists());
    }

    #[tokio::test]
    async fn webdav_materialize_streams_and_cleans_temp_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book.epub"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"remote-data".to_vec()))
            .mount(&server)
            .await;
        let config = SourceConfig {
            record: SourceRecord {
                id: 1,
                name: "webdav".to_string(),
                kind: SourceKind::WebDav,
                location: server.uri(),
                enabled: true,
                scan_on_add: false,
                schedule: None,
                hash_policy: "none".to_string(),
                read_only: true,
            },
            credentials: Some(WebDavCredentials {
                username: "reader".to_string(),
                password: "secret".to_string(),
            }),
        };
        let backend = SourceBackend::build(&config).expect("backend");
        let materialized = backend.materialize("book.epub").await.expect("materialize");
        let path = materialized.path.clone();
        assert_eq!(std::fs::read(&path).expect("read"), b"remote-data");
        drop(materialized);
        assert!(!path.exists());
    }
}
