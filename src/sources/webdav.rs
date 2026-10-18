use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::DateTime;
use percent_encoding::percent_decode_str;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::{Method, StatusCode};
use url::Url;

use crate::models::WebDavCredentials;

use super::SourceEntry;

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:getcontentlength/>
    <D:getlastmodified/>
  </D:prop>
</D:propfind>"#;

#[derive(Debug, Clone)]
pub struct WebDavSource {
    root: Url,
    client: reqwest::Client,
    username: String,
    password: String,
}

#[derive(Debug, Default)]
struct DavEntry {
    href: String,
    is_dir: bool,
    size: u64,
    mtime: i64,
}

impl WebDavSource {
    pub fn new(root: &str, credentials: &WebDavCredentials) -> Result<Self> {
        let mut root = Url::parse(root).with_context(|| format!("无效的 WebDAV 地址: {root}"))?;
        if !root.path().ends_with('/') {
            let path = format!("{}/", root.path());
            root.set_path(&path);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            root,
            client,
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        })
    }

    pub fn root(&self) -> &Url {
        &self.root
    }

    fn url_for(&self, rel_path: &str) -> Result<Url> {
        if rel_path.is_empty() {
            return Ok(self.root.clone());
        }
        let encoded = encode_path(rel_path);
        self.root
            .join(&encoded)
            .with_context(|| format!("无法拼接 WebDAV 地址: {rel_path}"))
    }

    async fn propfind(&self, url: &Url) -> Result<reqwest::Response> {
        let method = Method::from_bytes(b"PROPFIND")?;
        let response = self
            .client
            .request(method, url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(PROPFIND_BODY)
            .send()
            .await
            .with_context(|| format!("WebDAV PROPFIND 失败: {url}"))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            anyhow::bail!("WebDAV 认证失败: {}", url);
        }
        if !response.status().is_success() {
            anyhow::bail!("WebDAV PROPFIND {url} 返回 {}", response.status());
        }
        Ok(response)
    }

    pub async fn list(&self) -> Result<Vec<SourceEntry>> {
        let root_path = percent_decode_str(self.root.path())
            .decode_utf8_lossy()
            .to_string();
        let mut queue = vec![(self.root.clone(), String::new())];
        let mut visited = HashSet::new();
        let mut entries = Vec::new();

        while let Some((url, _relative)) = queue.pop() {
            if !visited.insert(url.to_string()) {
                continue;
            }
            let response = self.propfind(&url).await?;
            let xml = response.text().await?;
            for entry in parse_multistatus(&xml)? {
                if entry.href.is_empty() {
                    continue;
                }
                let href_path = if let Ok(parsed) = Url::parse(&entry.href) {
                    percent_decode_str(parsed.path())
                        .decode_utf8_lossy()
                        .to_string()
                } else {
                    percent_decode_str(&entry.href)
                        .decode_utf8_lossy()
                        .to_string()
                };
                let relative = href_path
                    .strip_prefix(&root_path)
                    .unwrap_or(&href_path)
                    .trim_start_matches('/')
                    .to_string();
                if relative.is_empty() {
                    continue;
                }
                if entry.is_dir {
                    let encoded = format!("{}/", encode_path(&relative));
                    if let Ok(child) = self.root.join(&encoded) {
                        queue.push((child, relative.clone()));
                        entries.push(SourceEntry {
                            rel_path: relative,
                            size: 0,
                            mtime: entry.mtime,
                            is_dir: true,
                        });
                    }
                } else {
                    entries.push(SourceEntry {
                        rel_path: relative,
                        size: entry.size,
                        mtime: entry.mtime,
                        is_dir: false,
                    });
                }
            }
        }
        entries.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
        Ok(entries)
    }

    pub async fn get(
        &self,
        rel_path: &str,
        range: Option<(u64, u64)>,
    ) -> Result<reqwest::Response> {
        let url = self.url_for(rel_path)?;
        let mut request = self
            .client
            .get(url.clone())
            .basic_auth(&self.username, Some(&self.password));
        if let Some((start, end)) = range {
            request = request.header("Range", format!("bytes={start}-{end}"));
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("WebDAV GET 失败: {url}"))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            anyhow::bail!("WebDAV 认证失败: {url}");
        }
        if !response.status().is_success() {
            anyhow::bail!("WebDAV GET {url} 返回 {}", response.status());
        }
        Ok(response)
    }

    pub async fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        let response = self.get(rel_path, None).await?;
        Ok(response.bytes().await?.to_vec())
    }

    pub async fn read_range(&self, rel_path: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        if start > end {
            anyhow::bail!("无效的字节范围 {start}-{end}");
        }
        let response = self.get(rel_path, Some((start, end))).await?;
        let status = response.status();
        let body = response.bytes().await?;
        if status == StatusCode::PARTIAL_CONTENT {
            return Ok(body.to_vec());
        }
        let start_index = start as usize;
        let end_index = (end as usize).min(body.len().saturating_sub(1));
        if start_index >= body.len() {
            anyhow::bail!("范围超出文件长度");
        }
        Ok(body[start_index..=end_index].to_vec())
    }
}

fn encode_path(rel_path: &str) -> String {
    // Keep normal path characters intact; Url::join encodes non-ASCII and spaces.
    rel_path
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F")
}

fn parse_multistatus(xml: &str) -> Result<Vec<DavEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();
    let mut current: Option<DavEntry> = None;
    let mut tag = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "response" {
                    current = Some(DavEntry::default());
                } else if name == "collection" {
                    if let Some(entry) = current.as_mut() {
                        entry.is_dir = true;
                    }
                }
                tag = name;
            }
            Ok(Event::Empty(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "collection" {
                    if let Some(entry) = current.as_mut() {
                        entry.is_dir = true;
                    }
                }
            }
            Ok(Event::Text(event)) => {
                if let Some(entry) = current.as_mut() {
                    let text = event.unescape().unwrap_or_default().to_string();
                    match tag.as_str() {
                        "href" => entry.href.push_str(&text),
                        "getcontentlength" => {
                            entry.size = text.trim().parse().unwrap_or(0);
                        }
                        "getlastmodified" => {
                            entry.mtime = DateTime::parse_from_rfc2822(text.trim())
                                .map(|value| value.timestamp())
                                .unwrap_or(0);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "response" {
                    if let Some(entry) = current.take() {
                        entries.push(entry);
                    }
                }
                tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(error).context("解析 WebDAV XML 失败"),
            _ => {}
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn credentials() -> WebDavCredentials {
        WebDavCredentials {
            username: "reader".to_string(),
            password: "secret".to_string(),
        }
    }

    const MULTISTATUS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat>
  </D:response>
  <D:response>
    <D:href>/book.epub</D:href>
    <D:propstat><D:prop>
      <D:resourcetype/>
      <D:getcontentlength>6</D:getcontentlength>
      <D:getlastmodified>Sun, 18 Oct 2026 07:59:35 GMT</D:getlastmodified>
    </D:prop></D:propstat>
  </D:response>
</D:multistatus>"#;

    #[tokio::test]
    async fn lists_entries_and_reads_file() {
        let server = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(MULTISTATUS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/book.epub"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"abcdef".to_vec()))
            .mount(&server)
            .await;

        let source = WebDavSource::new(&server.uri(), &credentials()).expect("source");
        let entries = source.list().await.expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rel_path, "book.epub");
        assert_eq!(entries[0].size, 6);
        assert!(entries[0].mtime > 0);

        let bytes = source.read("book.epub").await.expect("read");
        assert_eq!(bytes, b"abcdef");
    }

    #[tokio::test]
    async fn unauthorized_fails() {
        let server = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let source = WebDavSource::new(&server.uri(), &credentials()).expect("source");
        assert!(source.list().await.is_err());
    }

    #[tokio::test]
    async fn range_request_is_forwarded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book.pdf"))
            .and(header("Range", "bytes=2-4"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(b"cde".to_vec()))
            .mount(&server)
            .await;
        let source = WebDavSource::new(&server.uri(), &credentials()).expect("source");
        let bytes = source.read_range("book.pdf", 2, 4).await.expect("range");
        assert_eq!(bytes, b"cde");
    }
}
