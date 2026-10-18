use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use tower::ServiceExt;

use llibrary::auth::{self, Auth};
use llibrary::config::Config;
use llibrary::covers::CoverStore;
use llibrary::db::Db;
use llibrary::models::{SourceKind, WorkFilter};
use llibrary::scanner::{Scanner, ScannerHandle};
use llibrary::web::{router, AppState};

fn pdf_bytes() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object};
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
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

fn basic() -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode("reader:secret");
    format!("Basic {encoded}")
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

#[tokio::test]
async fn scan_search_detail_opds_range() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("library");
    std::fs::create_dir_all(root.join("中华书局"))?;
    std::fs::write(root.join("中华书局/论语译注.pdf"), pdf_bytes())?;

    let db = Db::open(dir.path().join("db.sqlite"))?;
    let source = db.insert_source(
        "test",
        SourceKind::Local,
        root.to_str().unwrap(),
        None,
        true,
        true,
        None,
        "hash",
    )?;
    let covers = CoverStore::new(dir.path().join("covers"))?;
    let scanner = Scanner::new(db.clone(), covers.clone(), None);
    let report = scanner.scan_source(source).await?;
    assert_eq!(report.stats.new, 1);

    let works = db.list_works(&WorkFilter::default(), 10, 0)?;
    assert_eq!(works.len(), 1);
    let work = &works[0];
    let files = db.list_files(work.id)?;
    assert_eq!(files.len(), 1);
    let file_id = files[0].id;

    let password_hash = auth::hash_password("secret")?;
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
    let handle = ScannerHandle::spawn(Scanner::new(db.clone(), covers.clone(), None));
    let state = AppState {
        config: Arc::new(config),
        db,
        auth,
        scanner: handle,
        covers,
        static_dir: PathBuf::from("static"),
    };
    let app = router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?q=%E8%AE%BA%E8%AF%AD")
                .header(header::AUTHORIZATION, basic())
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("论语译注"));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/works/{}", work.id))
                .header(header::AUTHORIZATION, basic())
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("中华书局/论语译注.pdf"));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/all")
                .header(header::AUTHORIZATION, basic())
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let feed = body_text(response).await;
    assert!(feed.contains(&format!("/files/{file_id}")));
    assert!(feed.contains("application/pdf"));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/files/{file_id}"))
                .header(header::AUTHORIZATION, basic())
                .header(header::RANGE, "bytes=0-4")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 5);
    Ok(())
}
