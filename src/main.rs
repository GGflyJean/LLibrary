use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use llibrary::auth::{self, Auth};
use llibrary::config::Config;
use llibrary::covers::CoverStore;
use llibrary::crypto::SecretBox;
use llibrary::db::Db;
use llibrary::models::{SourceKind, WebDavCredentials};
use llibrary::scanner::{spawn_cron, spawn_source_cron, Scanner, ScannerHandle};
use llibrary::web::{self, AppState};

#[derive(Debug, Parser)]
#[command(name = "llibrary", version, about = "私人书库目录与发现服务")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 启动 Web/OPDS 服务
    Serve,
    /// 扫描全部来源或指定来源
    Scan {
        #[arg(long)]
        source: Option<i64>,
    },
    /// 管理扫描来源
    Sources {
        #[command(subcommand)]
        command: SourcesCommand,
    },
    /// 生成 Argon2 密码哈希
    HashPassword { password: String },
}

#[derive(Debug, Subcommand)]
enum SourcesCommand {
    /// 列出所有来源
    List,
    /// 添加来源
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, value_parser = ["local", "webdav"])]
        kind: String,
        #[arg(long)]
        location: String,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        /// 添加后不立即扫描
        #[arg(long)]
        no_scan: bool,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        hash_policy: Option<String>,
    },
    /// 删除来源
    Remove { id: i64 },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
        Command::Scan { source } => scan(source).await,
        Command::Sources { command } => sources(command).await,
        Command::HashPassword { password } => {
            println!("{}", auth::hash_password(&password)?);
            Ok(())
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn serve() -> Result<()> {
    init_tracing();
    let config = Config::from_env()?;
    config.ensure_dirs()?;
    let db = Db::open(&config.db_path)?;
    let covers = CoverStore::new(config.covers_dir())?;
    let auth = Arc::new(Auth::new(
        config.auth_user.clone(),
        config.auth_password_hash.clone(),
    ));
    let scanner = Scanner::new(db.clone(), covers.clone(), config.secret.clone());
    let scanner_handle = ScannerHandle::spawn(scanner);
    if let Some(expression) = &config.scan_cron {
        if spawn_cron(scanner_handle.clone(), expression).is_some() {
            tracing::info!(schedule = %expression, "已启用全局定时扫描");
        } else {
            tracing::warn!(schedule = %expression, "全局定时扫描表达式无效，已忽略");
        }
    }
    for source in db.list_sources()? {
        if let Some(expression) = &source.schedule {
            if spawn_source_cron(scanner_handle.clone(), source.id, expression).is_some() {
                tracing::info!(source = %source.name, schedule = %expression, "已启用来源定时扫描");
            } else {
                tracing::warn!(source = %source.name, schedule = %expression, "来源定时扫描表达式无效");
            }
        }
    }
    let state = AppState {
        config: Arc::new(config.clone()),
        db,
        auth,
        scanner: scanner_handle,
        covers,
        static_dir: PathBuf::from("static"),
    };
    let app = web::router(state);
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("无法监听 {}", config.bind_addr))?;
    tracing::info!(address = %config.bind_addr, "LLibrary 已启动");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn scan(source: Option<i64>) -> Result<()> {
    init_tracing();
    let config = Config::from_env()?;
    config.ensure_dirs()?;
    let db = Db::open(&config.db_path)?;
    let covers = CoverStore::new(config.covers_dir())?;
    let scanner = Scanner::new(db, covers, config.secret.clone());
    if let Some(source_id) = source {
        let report = scanner.scan_source(source_id).await?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let reports = scanner.scan_all().await;
        println!("{}", serde_json::to_string_pretty(&reports)?);
    }
    Ok(())
}

async fn sources(command: SourcesCommand) -> Result<()> {
    init_tracing();
    let config = Config::from_env()?;
    config.ensure_dirs()?;
    let db = Db::open(&config.db_path)?;
    match command {
        SourcesCommand::List => {
            let sources = db.list_sources()?;
            for source in sources {
                println!(
                    "{}\t{}\t{}\t{}\tenabled={}\tscan_on_add={}\tschedule={}",
                    source.id,
                    source.name,
                    source.kind.as_str(),
                    source.location,
                    source.enabled,
                    source.scan_on_add,
                    source.schedule.unwrap_or_default()
                );
            }
        }
        SourcesCommand::Add {
            name,
            kind,
            location,
            username,
            password,
            no_scan,
            schedule,
            hash_policy,
        } => {
            let scan_on_add = !no_scan;
            let kind = SourceKind::parse(&kind).context("未知来源类型")?;
            let credentials = match (username, password) {
                (Some(username), Some(password)) => Some(WebDavCredentials { username, password }),
                _ => None,
            };
            let encrypted = match &credentials {
                Some(credentials) => {
                    let secret = config.secret.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("添加 WebDAV 凭据前必须配置 LLIBRARY_SECRET")
                    })?;
                    Some(SecretBox::new(secret).encode_credentials(credentials)?)
                }
                None => None,
            };
            let hash_policy = hash_policy.unwrap_or_else(|| match kind {
                SourceKind::WebDav => "none".to_string(),
                SourceKind::Local => "hash".to_string(),
            });
            let id = db.insert_source(
                &name,
                kind,
                &location,
                encrypted.as_deref(),
                true,
                scan_on_add,
                schedule.as_deref(),
                &hash_policy,
            )?;
            println!("已添加来源 {id}: {name}");
            if scan_on_add {
                let covers = CoverStore::new(config.covers_dir())?;
                let scanner = Scanner::new(db, covers, config.secret.clone());
                let report = scanner.scan_source(id).await?;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        SourcesCommand::Remove { id } => {
            if db.delete_source(id)? {
                println!("已删除来源 {id}");
            } else {
                println!("来源 {id} 不存在");
            }
        }
    }
    Ok(())
}
