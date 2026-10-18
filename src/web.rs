use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use askama::Template;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use chrono::Utc;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tower_http::services::ServeDir;

use crate::auth::{self, Auth, SESSION_COOKIE};
use crate::config::Config;
use crate::covers::CoverStore;
use crate::crypto::SecretBox;
use crate::db::Db;
use crate::models::{
    FileRecord, ScanProgress, ScanRun, SourceConfig, SourceKind, SourceOption, WorkFilter,
    WorkSummary,
};
use crate::scanner::ScannerHandle;
use crate::sources::SourceBackend;

const PER_PAGE: i64 = 40;
const COVER_CACHE: &str = "private, max-age=86400";

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    pub auth: Arc<Auth>,
    pub scanner: ScannerHandle,
    pub covers: CoverStore,
    pub static_dir: PathBuf,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    query: String,
    publisher: String,
    format: String,
    publishers: Vec<String>,
    formats: Vec<String>,
    source_options: Vec<SourceOption>,
    source_id: i64,
    works: Vec<WorkSummary>,
    total: i64,
    page: i64,
    total_pages: i64,
    prev_page: i64,
    next_page: i64,
    scanning: bool,
    scan_source: String,
    scan_phase: String,
    scan_processed: usize,
    scan_total: usize,
}

#[derive(Template)]
#[template(path = "detail.html")]
struct DetailTemplate {
    work: WorkSummary,
    files: Vec<FileRecord>,
    cover_url: String,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: String,
    next: String,
}

#[derive(Template)]
#[template(path = "scans.html")]
struct ScansTemplate {
    scanning: bool,
    progress: Option<ScanProgress>,
    runs: Vec<ScanRun>,
}

impl ScansTemplate {
    fn progress_source(&self) -> &str {
        self.progress
            .as_ref()
            .map(|progress| progress.source_name.as_str())
            .unwrap_or("")
    }

    fn progress_phase(&self) -> &str {
        self.progress
            .as_ref()
            .map(|progress| progress.phase.as_str())
            .unwrap_or("")
    }

    fn progress_processed(&self) -> usize {
        self.progress
            .as_ref()
            .map(|progress| progress.processed)
            .unwrap_or(0)
    }

    fn progress_total(&self) -> usize {
        self.progress
            .as_ref()
            .map(|progress| progress.total)
            .unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
struct IndexQuery {
    q: Option<String>,
    publisher: Option<String>,
    format: Option<String>,
    source: Option<i64>,
    page: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    username: String,
    password: String,
    next: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let static_dir = state.static_dir.clone();
    Router::new()
        .route("/", get(index))
        .route("/works/{id}", get(detail))
        .route("/files/{id}", get(file))
        .route("/covers/{file}", get(cover))
        .route("/opds", get(opds_root))
        .route("/opds/all", get(opds_all))
        .route("/opds/latest", get(opds_latest))
        .route("/opds/publisher/{name}", get(opds_publisher))
        .route("/login", get(login_get).post(login_post))
        .route("/logout", post(logout))
        .route("/scans", get(scans_page))
        .route("/scan", post(scan_now))
        .route("/api/scans", get(api_scans))
        .route("/api/scans/current", get(api_scan_current))
        .route("/api/scans/{id}", get(api_scan))
        .route("/health", get(health))
        .nest_service("/static", ServeDir::new(static_dir))
        .with_state(state)
}

async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<IndexQuery>,
) -> Response {
    if let Some(response) = require_web(&state, &headers) {
        return response;
    }
    let page = query.page.unwrap_or(1).max(1);
    let source_id = query.source.filter(|value| *value > 0);
    let filter = WorkFilter {
        query: clean(query.q),
        publisher: clean(query.publisher),
        format: clean(query.format),
        source_id,
    };
    let db = state.db.clone();
    let filter_db = filter.clone();
    let result = blocking(move || {
        let works = db.list_works(&filter_db, PER_PAGE, (page - 1) * PER_PAGE)?;
        let total = db.count_works(&filter_db)?;
        let publishers = db.publishers()?;
        let sources = db.list_sources()?;
        Ok((works, total, publishers, sources))
    })
    .await;
    let (works, total, publishers, sources) = match result {
        Ok(value) => value,
        Err(error) => return internal(error),
    };
    let source_options = sources
        .into_iter()
        .map(|source| SourceOption {
            id: source.id,
            name: source.name,
            selected: source_id == Some(source.id),
        })
        .collect();
    let total_pages = ((total + PER_PAGE - 1) / PER_PAGE).max(1);
    let progress = state.scanner.progress();
    let template = IndexTemplate {
        query: filter.query.clone().unwrap_or_default(),
        publisher: filter.publisher.clone().unwrap_or_default(),
        format: filter.format.clone().unwrap_or_default(),
        publishers,
        formats: ["epub", "azw3", "pdf", "mobi", "azw"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        source_options,
        source_id: source_id.unwrap_or(0),
        works,
        total,
        page,
        total_pages,
        prev_page: if page > 1 { page - 1 } else { 0 },
        next_page: if page < total_pages { page + 1 } else { 0 },
        scanning: progress.is_some(),
        scan_source: progress
            .as_ref()
            .map(|progress| progress.source_name.clone())
            .unwrap_or_default(),
        scan_phase: progress
            .as_ref()
            .map(|progress| progress.phase.clone())
            .unwrap_or_default(),
        scan_processed: progress
            .as_ref()
            .map(|progress| progress.processed)
            .unwrap_or(0),
        scan_total: progress
            .as_ref()
            .map(|progress| progress.total)
            .unwrap_or(0),
    };
    render(template)
}

async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(response) = require_web(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    let result = blocking(move || db.get_work(id)).await;
    match result {
        Ok(Some(detail)) => {
            let cover_url = detail.work.cover_url().to_string();
            render(DetailTemplate {
                work: detail.work,
                files: detail.files,
                cover_url,
            })
        }
        Ok(None) => not_found(),
        Err(error) => internal(error),
    }
}

async fn cover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(file): Path<String>,
) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let Some(work_id) = file
        .strip_suffix(".jpg")
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return not_found();
    };
    if !state.covers.is_ready(work_id) {
        if let Err(error) = maybe_pdf_cover(&state, work_id).await {
            tracing::warn!(work_id, %error, "按需生成 PDF 封面失败");
        }
    }
    let path = state.covers.path_for(work_id);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(COVER_CACHE));
            response
        }
        Err(_) => not_found(),
    }
}

async fn file(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    let record = match blocking(move || db.get_file(id)).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found(),
        Err(error) => return internal(error),
    };
    if record.missing {
        return not_found();
    }
    let db = state.db.clone();
    let source_id = record.source_id;
    let secret = state.config.secret.clone();
    let config = match blocking(move || load_source_config(&db, secret.as_deref(), source_id)).await
    {
        Ok(config) => config,
        Err(error) => return internal(error),
    };
    let backend = match SourceBackend::build(&config) {
        Ok(backend) => backend,
        Err(error) => return internal(error),
    };
    let total = record.size.max(0) as u64;
    let range = match parse_range(
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        total,
    ) {
        Ok(range) => range,
        Err(()) => return range_not_satisfiable(total),
    };
    let (start, end) = range.unwrap_or((0, total.saturating_sub(1)));
    let length = if range.is_some() {
        end.saturating_sub(start) + 1
    } else {
        total
    };
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let body = match &backend {
        SourceBackend::Local(local) => {
            let mut handle = match local.open(&record.rel_path).await {
                Ok(handle) => handle,
                Err(error) => return internal(error),
            };
            if range.is_some() {
                if let Err(error) = handle.seek(std::io::SeekFrom::Start(start)).await {
                    return internal(error);
                }
                Body::from_stream(ReaderStream::new(handle.take(length)))
            } else {
                Body::from_stream(ReaderStream::new(handle))
            }
        }
        SourceBackend::WebDav(webdav) => match webdav.get(&record.rel_path, range).await {
            Ok(response) => {
                if range.is_some() && response.status() == StatusCode::OK {
                    match response.bytes().await {
                        Ok(bytes) => {
                            let start_index = start as usize;
                            let end_index = (end as usize).min(bytes.len().saturating_sub(1));
                            if start_index >= bytes.len() {
                                return range_not_satisfiable(total);
                            }
                            Body::from(bytes[start_index..=end_index].to_vec())
                        }
                        Err(error) => return internal(error),
                    }
                } else {
                    Body::from_stream(response.bytes_stream())
                }
            }
            Err(error) => return internal(error),
        },
    };

    let etag = if record.hash.is_empty() {
        format!("\"{}-{}-{}\"", record.id, record.size, record.mtime)
    } else {
        format!("\"{}\"", record.hash)
    };
    let filename = record
        .rel_path
        .rsplit('/')
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.{}", record.basename, record.format));
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(record.mime_type()),
    );
    response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response_headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    response_headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).unwrap_or(HeaderValue::from_static("\"unknown\"")),
    );
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    let disposition = format!(
        "inline; filename*=UTF-8''{}",
        utf8_percent_encode(&filename, NON_ALPHANUMERIC)
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        response_headers.insert(header::CONTENT_DISPOSITION, value);
    }
    if let Some((range_start, range_end)) = range {
        if let Ok(value) =
            HeaderValue::from_str(&format!("bytes {range_start}-{range_end}/{total}"))
        {
            response_headers.insert(header::CONTENT_RANGE, value);
        }
    }
    response
}

async fn opds_root(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    let publishers = match blocking(move || db.publishers()).await {
        Ok(publishers) => publishers,
        Err(error) => return internal(error),
    };
    let now = Utc::now().to_rfc3339();
    let mut xml = String::new();
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="utf-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">"#
    );
    let _ = write!(
        xml,
        "<id>urn:llibrary:root</id><title>LLibrary</title><updated>{now}</updated>"
    );
    let _ = writeln!(
        xml,
        r#"<link rel="self" href="/opds" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>"#
    );
    for (title, href) in [("全部作品", "/opds/all"), ("最新入库", "/opds/latest")] {
        let _ = write!(
            xml,
            "<entry><title>{}</title><id>urn:llibrary:{}</id><updated>{now}</updated><link rel=\"subsection\" href=\"{}\" type=\"application/atom+xml;profile=opds-catalog;kind=acquisition\"/></entry>",
            xml_escape(title),
            xml_escape(href),
            href
        );
    }
    for publisher in publishers {
        let href = format!(
            "/opds/publisher/{}",
            utf8_percent_encode(&publisher, NON_ALPHANUMERIC)
        );
        let _ = write!(
            xml,
            "<entry><title>{}</title><id>urn:llibrary:publisher:{}</id><updated>{now}</updated><link rel=\"subsection\" href=\"{}\" type=\"application/atom+xml;profile=opds-catalog;kind=acquisition\"/></entry>",
            xml_escape(&publisher),
            xml_escape(&publisher),
            href
        );
    }
    let _ = writeln!(xml, "</feed>");
    opds_response(xml, "navigation")
}

async fn opds_all(State(state): State<AppState>, headers: HeaderMap) -> Response {
    opds_feed(state, headers, None, false).await
}

async fn opds_latest(State(state): State<AppState>, headers: HeaderMap) -> Response {
    opds_feed(state, headers, None, true).await
}

async fn opds_publisher(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    opds_feed(state, headers, Some(name), false).await
}

async fn opds_feed(
    state: AppState,
    headers: HeaderMap,
    publisher: Option<String>,
    latest: bool,
) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    let publisher_db = publisher.clone();
    let works =
        match blocking(move || db.list_works_opds(publisher_db.as_deref(), 100, latest)).await {
            Ok(works) => works,
            Err(error) => return internal(error),
        };
    let now = Utc::now().to_rfc3339();
    let self_href = match &publisher {
        Some(name) => format!(
            "/opds/publisher/{}",
            utf8_percent_encode(name, NON_ALPHANUMERIC)
        ),
        None if latest => "/opds/latest".to_string(),
        None => "/opds/all".to_string(),
    };
    let title = publisher
        .as_deref()
        .map(|name| format!("{name} - LLibrary"))
        .unwrap_or_else(|| {
            if latest {
                "最新入库 - LLibrary".to_string()
            } else {
                "全部作品 - LLibrary".to_string()
            }
        });
    let mut xml = String::new();
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="utf-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">"#
    );
    let _ = write!(
        xml,
        "<id>urn:llibrary:feed:{}</id><title>{}</title><updated>{now}</updated>",
        xml_escape(&self_href),
        xml_escape(&title)
    );
    let _ = writeln!(
        xml,
        r#"<link rel="self" href="{}" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>"#,
        self_href
    );
    for work in works {
        let db = state.db.clone();
        let work_id = work.id;
        let files = match blocking(move || db.list_files(work_id)).await {
            Ok(files) => files,
            Err(error) => return internal(error),
        };
        let _ = write!(
            xml,
            "<entry><id>urn:llibrary:work:{}</id><title>{}</title><updated>{now}</updated>",
            work.id,
            xml_escape(&work.title)
        );
        if !work.authors.is_empty() {
            let _ = write!(
                xml,
                "<author><name>{}</name></author>",
                xml_escape(&work.authors)
            );
        }
        if !work.description.is_empty() {
            let _ = write!(xml, "<summary>{}</summary>", xml_escape(&work.description));
        }
        let cover = work.cover_url();
        let _ = write!(
            xml,
            r#"<link rel="http://opds-spec.org/image" href="{}" type="image/jpeg"/>"#,
            xml_escape(cover)
        );
        for file in files.into_iter().filter(|file| !file.missing) {
            let _ = write!(
                xml,
                r#"<link rel="http://opds-spec.org/acquisition" href="/files/{}" type="{}"/>"#,
                file.id,
                file.mime_type()
            );
        }
        let _ = write!(xml, "</entry>");
    }
    let _ = writeln!(xml, "</feed>");
    opds_response(xml, "acquisition")
}

async fn login_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LoginQuery>,
) -> Response {
    let next = safe_next(query.next);
    if state.auth.authenticate(&headers) {
        return Redirect::to(&next).into_response();
    }
    render(LoginTemplate {
        error: String::new(),
        next,
    })
}

async fn login_post(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let next = safe_next(form.next);
    let Some(token) = state.auth.login(&form.username, &form.password) else {
        let html = LoginTemplate {
            error: "用户名或密码错误".to_string(),
            next,
        }
        .render()
        .unwrap_or_else(|_| "登录失败".to_string());
        return (StatusCode::UNAUTHORIZED, Html(html)).into_response();
    };
    let cookie =
        format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800");
    let mut response = Redirect::to(&next).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = auth::session_token(&headers) {
        state.auth.logout(&token);
    }
    let cookie = format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0");
    let mut response = Redirect::to("/login").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn scans_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_web(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    let runs = match blocking(move || db.latest_scan_runs(20)).await {
        Ok(runs) => runs,
        Err(error) => return internal(error),
    };
    let progress = state.scanner.progress();
    render(ScansTemplate {
        scanning: progress.is_some(),
        progress,
        runs,
    })
}

async fn scan_now(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    match state.scanner.enqueue_all().await {
        Ok(()) => Redirect::to("/scans").into_response(),
        Err(error) => internal(error),
    }
}

async fn api_scans(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    match blocking(move || db.latest_scan_runs(50)).await {
        Ok(runs) => Json(runs).into_response(),
        Err(error) => internal(error),
    }
}

async fn api_scan_current(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    Json(state.scanner.progress()).into_response()
}

async fn api_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Some(response) = require_api(&state, &headers) {
        return response;
    }
    let db = state.db.clone();
    match blocking(move || db.get_scan_run(id)).await {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => not_found(),
        Err(error) => internal(error),
    }
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn require_web(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if state.auth.authenticate(headers) {
        None
    } else {
        Some(Redirect::to("/login").into_response())
    }
}

fn require_api(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if state.auth.authenticate(headers) {
        return None;
    }
    let mut response = (StatusCode::UNAUTHORIZED, "需要认证").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"LLibrary\""),
    );
    Some(response)
}

fn render<T: Template>(template: T) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => internal(error),
    }
}

fn internal(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("内部错误: {error}"),
    )
        .into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "未找到").into_response()
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn safe_next(value: Option<String>) -> String {
    value
        .filter(|value| value.starts_with('/') && !value.starts_with("//"))
        .unwrap_or_else(|| "/".to_string())
}

fn load_source_config(db: &Db, secret: Option<&str>, source_id: i64) -> Result<SourceConfig> {
    let record = db
        .get_source(source_id)?
        .with_context(|| format!("来源不存在: {source_id}"))?;
    let credentials = match db.get_source_credentials(source_id)? {
        Some(encrypted) => {
            if record.kind == SourceKind::WebDav {
                let secret = secret.ok_or_else(|| {
                    anyhow::anyhow!("未配置 LLIBRARY_SECRET，无法解密 WebDAV 凭据")
                })?;
                Some(SecretBox::new(secret).decode_credentials(&encrypted)?)
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

async fn maybe_pdf_cover(state: &AppState, work_id: i64) -> Result<()> {
    if state.covers.is_ready(work_id) {
        return Ok(());
    }
    let db = state.db.clone();
    let files = blocking(move || db.list_files(work_id)).await?;
    let Some(pdf) = files
        .into_iter()
        .find(|file| file.format == "pdf" && !file.missing)
    else {
        state.covers.mark_ready(work_id)?;
        return Ok(());
    };
    let db = state.db.clone();
    let secret = state.config.secret.clone();
    let source_id = pdf.source_id;
    let config = blocking(move || load_source_config(&db, secret.as_deref(), source_id)).await?;
    let backend = SourceBackend::build(&config)?;
    let bytes = backend.read(&pdf.rel_path).await?;
    let covers = state.covers.clone();
    let path =
        tokio::task::spawn_blocking(move || covers.save_pdf_cover(work_id, &bytes)).await??;
    tracing::debug!(work_id, %path, "按需生成 PDF 封面完成");
    Ok(())
}

fn parse_range(header_value: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = header_value else {
        return Ok(None);
    };
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') || size == 0 {
        return Err(());
    }
    let (start_raw, end_raw) = value.split_once('-').ok_or(())?;
    if start_raw.is_empty() {
        let suffix: u64 = end_raw.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let start = size.saturating_sub(suffix);
        return Ok(Some((start, size - 1)));
    }
    let start: u64 = start_raw.parse().map_err(|_| ())?;
    let end: u64 = if end_raw.is_empty() {
        size - 1
    } else {
        end_raw.parse().map_err(|_| ())?
    };
    if start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end.min(size - 1))))
}

fn range_not_satisfiable(size: u64) -> Response {
    let mut response = (StatusCode::RANGE_NOT_SATISFIABLE, "范围不可满足").into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("bytes */{size}")) {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    response
}

fn opds_response(xml: String, kind: &str) -> Response {
    let mut response = Response::new(Body::from(xml));
    if let Ok(value) = HeaderValue::from_str(&format!(
        "application/atom+xml;profile=opds-catalog;kind={kind}; charset=utf-8"
    )) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    response
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn blocking<T, F>(task: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .context("阻塞任务失败")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewFile;
    use crate::models::BookMetadata;
    use crate::scanner::Scanner;
    use axum::http::{Request, StatusCode};
    use base64::Engine;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn basic() -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode("reader:secret");
        format!("Basic {encoded}")
    }

    fn request(uri: &str, authenticated: bool) -> Request<Body> {
        let mut builder = Request::builder().uri(uri);
        if authenticated {
            builder = builder.header(header::AUTHORIZATION, basic());
        }
        builder.body(Body::empty()).unwrap()
    }

    fn test_state() -> (tempfile::TempDir, AppState, i64, i64) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("library");
        let rel_path = "中华书局/论语译注.epub";
        let full_path = root.join(rel_path);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, b"0123456789").unwrap();

        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let _source = db
            .insert_source(
                "test",
                SourceKind::Local,
                root.to_str().unwrap(),
                None,
                true,
                true,
                None,
                "hash",
            )
            .unwrap();
        let work = db
            .insert_work(
                "test-key",
                &BookMetadata {
                    title: "论语译注".to_string(),
                    authors: "杨伯峻".to_string(),
                    publisher: "中华书局".to_string(),
                    isbn: "9787101000000".to_string(),
                    description: "简介".to_string(),
                    language: "zh".to_string(),
                    pub_date: "2024".to_string(),
                },
                None,
            )
            .unwrap();
        let file = db
            .upsert_file(&NewFile {
                work_id: work,
                source_id: _source,
                rel_path: rel_path.to_string(),
                basename: "论语译注".to_string(),
                format: "epub".to_string(),
                size: 10,
                mtime: 0,
                hash: "abc".to_string(),
                page_count: 0,
                error: None,
            })
            .unwrap();
        let covers = CoverStore::new(dir.path().join("covers")).unwrap();
        let cover = covers.ensure_placeholder(work).unwrap();
        db.set_work_cover(work, &cover).unwrap();

        let password_hash = auth::hash_password("secret").unwrap();
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("db.sqlite"),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            auth_user: "reader".to_string(),
            auth_password_hash: password_hash.clone(),
            secret: None,
            scan_cron: None,
        };
        let auth = Arc::new(Auth::new("reader", password_hash));
        let scanner = ScannerHandle::spawn(Scanner::new(db.clone(), covers.clone(), None));
        let state = AppState {
            config: Arc::new(config),
            db,
            auth,
            scanner,
            covers,
            static_dir: PathBuf::from("static"),
        };
        (dir, state, work, file)
    }

    async fn body_text(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn pages_search_and_detail() {
        let (_dir, state, work, _file) = test_state();
        let app = router(state);

        let response = app.clone().oneshot(request("/", true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("论语译注"));
        assert!(body.contains("中华书局"));

        let response = app
            .clone()
            .oneshot(request("/?q=%E8%AE%BA%E8%AF%AD", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("论语译注"));

        let response = app
            .clone()
            .oneshot(request("/?q=not-found", true))
            .await
            .unwrap();
        assert!(body_text(response).await.contains("没有找到作品"));

        let response = app
            .clone()
            .oneshot(request("/?source=1", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("论语译注"));

        let response = app
            .clone()
            .oneshot(request("/?source=999", true))
            .await
            .unwrap();
        assert!(body_text(response).await.contains("没有找到作品"));

        let response = app
            .clone()
            .oneshot(request("/works/1", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("杨伯峻"));
        assert!(body.contains("中华书局/论语译注.epub"));

        let response = app
            .clone()
            .oneshot(request(&format!("/works/{}", work + 1000), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn authentication_flow() {
        let (_dir, state, _work, _file) = test_state();
        let app = router(state);

        let response = app.clone().oneshot(request("/", false)).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");

        let response = app.clone().oneshot(request("/opds", false)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));

        let response = app.clone().oneshot(request("/login", false)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("登录"));

        let form = "username=reader&password=secret&next=%2F";
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(form))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/logout")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let response = app
            .clone()
            .oneshot(request("/health", false))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn files_support_range_and_mime() {
        let (_dir, state, work, file) = test_state();
        let app = router(state);

        let response = app
            .clone()
            .oneshot(request(&format!("/files/{file}"), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/epub+zip"
        );
        assert_eq!(
            response.headers().get(header::ACCEPT_RANGES).unwrap(),
            "bytes"
        );
        assert_eq!(body_text(response).await, "0123456789");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/files/{file}"))
                    .header(header::AUTHORIZATION, basic())
                    .header(header::RANGE, "bytes=2-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 2-5/10"
        );
        assert_eq!(body_text(response).await, "2345");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/files/{file}"))
                    .header(header::AUTHORIZATION, basic())
                    .header(header::RANGE, "bytes=99-100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes */10"
        );

        let response = app
            .clone()
            .oneshot(request(&format!("/covers/{work}.jpg"), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );

        let response = app
            .clone()
            .oneshot(request(&format!("/files/{}", file + 1000), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn opds_feeds_are_valid() {
        let (_dir, state, work, file) = test_state();
        let app = router(state);

        let response = app.clone().oneshot(request("/opds", true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("kind=navigation"));
        let body = body_text(response).await;
        assert!(body.contains("/opds/all"));
        assert!(body.contains("中华书局"));

        let response = app
            .clone()
            .oneshot(request("/opds/all", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("http://opds-spec.org/acquisition"));
        assert!(body.contains(&format!("/files/{file}")));
        assert!(body.contains("application/epub+zip"));
        assert!(body.contains(&format!("urn:llibrary:work:{work}")));

        let response = app
            .clone()
            .oneshot(request(
                "/opds/publisher/%E4%B8%AD%E5%8D%8E%E4%B9%A6%E5%B1%80",
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("论语译注"));
    }

    #[tokio::test]
    async fn pagination_boundary_and_static_assets() {
        let (_dir, state, _work, _file) = test_state();
        let db = state.db.clone();
        for index in 0..41 {
            let metadata = BookMetadata {
                title: format!("A{index:02}"),
                authors: "作者".to_string(),
                ..Default::default()
            };
            db.insert_work(&format!("page-{index}"), &metadata, None)
                .unwrap();
        }
        let app = router(state);

        let response = app.clone().oneshot(request("/", true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let first = body_text(response).await;
        assert!(first.contains("A00"));
        assert!(!first.contains("论语译注"));

        let response = app
            .clone()
            .oneshot(request("/?page=2", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("论语译注"));

        let response = app
            .clone()
            .oneshot(request("/?page=99", true))
            .await
            .unwrap();
        assert!(body_text(response).await.contains("没有找到作品"));

        let response = app
            .clone()
            .oneshot(request("/static/manifest.webmanifest", false))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("LLibrary"));

        let response = app
            .clone()
            .oneshot(request("/static/sw.js", false))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("addEventListener"));

        let response = app
            .clone()
            .oneshot(request("/static/icon-192.png", false))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
    }

    #[tokio::test]
    async fn detail_shows_missing_and_duplicate_status() {
        use std::collections::HashSet;

        let (_dir, state, work, _file) = test_state();
        let db = state.db.clone();
        let source_id = db.list_sources().unwrap()[0].id;
        db.upsert_file(&NewFile {
            work_id: work,
            source_id,
            rel_path: "中华书局/副本.epub".to_string(),
            basename: "论语译注".to_string(),
            format: "epub".to_string(),
            size: 10,
            mtime: 0,
            hash: "abc".to_string(),
            page_count: 0,
            error: None,
        })
        .unwrap();
        db.upsert_file(&NewFile {
            work_id: work,
            source_id,
            rel_path: "中华书局/缺失.epub".to_string(),
            basename: "论语译注".to_string(),
            format: "epub".to_string(),
            size: 10,
            mtime: 0,
            hash: "def".to_string(),
            page_count: 0,
            error: None,
        })
        .unwrap();
        let seen = HashSet::from([
            "中华书局/论语译注.epub".to_string(),
            "中华书局/副本.epub".to_string(),
        ]);
        db.mark_missing(source_id, &seen).unwrap();

        let app = router(state);
        let response = app
            .clone()
            .oneshot(request(&format!("/works/{work}"), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("缺失"));
        assert!(body.contains("重复"));
    }

    #[tokio::test]
    async fn scan_api_and_page() {
        let (_dir, state, _work, _file) = test_state();
        let app = router(state);

        let response = app
            .clone()
            .oneshot(request("/api/scans/current", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body == "null" || body.contains("source_id"));

        let response = app.clone().oneshot(request("/scans", true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/scan")
                    .header(header::AUTHORIZATION, basic())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
}
