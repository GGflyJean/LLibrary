use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub bind_addr: SocketAddr,
    pub auth_user: String,
    pub auth_password_hash: String,
    pub secret: Option<String>,
    pub scan_cron: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("缺少必需的环境变量 {0}")]
    Missing(&'static str),
    #[error("环境变量 {0} 的值无效: {1}")]
    Invalid(&'static str, String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_map(&vars)
    }

    pub fn from_map(vars: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |key: &str| vars.get(key).cloned().filter(|value| !value.is_empty());

        let data_dir = PathBuf::from(get("DATA_DIR").unwrap_or_else(|| "./data".to_string()));
        let db_path = get("DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("library.db"));
        let bind_raw = get("BIND_ADDR").unwrap_or_else(|| "127.0.0.1:8080".to_string());
        let bind_addr = bind_raw
            .parse()
            .map_err(|error| ConfigError::Invalid("BIND_ADDR", format!("{error}")))?;

        let auth_user = get("AUTH_USER").ok_or(ConfigError::Missing("AUTH_USER"))?;
        let auth_password_hash =
            get("AUTH_PASSWORD_HASH").ok_or(ConfigError::Missing("AUTH_PASSWORD_HASH"))?;

        Ok(Self {
            data_dir,
            db_path,
            bind_addr,
            auth_user,
            auth_password_hash,
            secret: get("LLIBRARY_SECRET"),
            scan_cron: get("SCAN_CRON"),
        })
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.data_dir.join("covers")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                anyhow::anyhow!("数据目录不可写或无法创建 {}: {error}", parent.display())
            })?;
        }
        for dir in [&self.data_dir, &self.covers_dir(), &self.logs_dir()] {
            std::fs::create_dir_all(dir).map_err(|error| {
                anyhow::anyhow!("数据目录不可写或无法创建 {}: {error}", dir.display())
            })?;
        }
        verify_writable(&self.data_dir)?;
        if let Some(parent) = self.db_path.parent() {
            verify_writable(parent)?;
        }
        Ok(())
    }
}

fn verify_writable(dir: &Path) -> anyhow::Result<()> {
    let probe = dir.join(".llibrary-write-test");
    std::fs::write(&probe, b"ok")
        .map_err(|error| anyhow::anyhow!("数据目录不可写 {}: {error}", dir.display()))?;
    std::fs::remove_file(&probe)
        .map_err(|error| anyhow::anyhow!("无法清理写测试文件 {}: {error}", probe.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_map() -> HashMap<String, String> {
        HashMap::from([
            ("AUTH_USER".to_string(), "reader".to_string()),
            ("AUTH_PASSWORD_HASH".to_string(), "hash-value".to_string()),
        ])
    }

    #[test]
    fn defaults_are_applied() {
        let config = Config::from_map(&base_map()).expect("config");
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.db_path, PathBuf::from("./data/library.db"));
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:8080");
        assert!(config.secret.is_none());
        assert!(config.scan_cron.is_none());
    }

    #[test]
    fn overrides_are_applied() {
        let mut vars = base_map();
        vars.insert("DATA_DIR".to_string(), "/tmp/llibrary".to_string());
        vars.insert("DB_PATH".to_string(), "/tmp/llibrary/custom.db".to_string());
        vars.insert("BIND_ADDR".to_string(), "0.0.0.0:9000".to_string());
        vars.insert("LLIBRARY_SECRET".to_string(), "secret".to_string());
        vars.insert("SCAN_CRON".to_string(), "0 3 * * *".to_string());
        let config = Config::from_map(&vars).expect("config");
        assert_eq!(config.db_path, PathBuf::from("/tmp/llibrary/custom.db"));
        assert_eq!(config.bind_addr.to_string(), "0.0.0.0:9000");
        assert_eq!(config.secret.as_deref(), Some("secret"));
        assert_eq!(config.scan_cron.as_deref(), Some("0 3 * * *"));
    }

    #[test]
    fn missing_auth_user_fails() {
        let mut vars = base_map();
        vars.remove("AUTH_USER");
        assert_eq!(
            Config::from_map(&vars).unwrap_err(),
            ConfigError::Missing("AUTH_USER")
        );
    }

    #[test]
    fn missing_password_hash_fails() {
        let mut vars = base_map();
        vars.remove("AUTH_PASSWORD_HASH");
        assert_eq!(
            Config::from_map(&vars).unwrap_err(),
            ConfigError::Missing("AUTH_PASSWORD_HASH")
        );
    }

    #[test]
    fn invalid_bind_address_fails() {
        let mut vars = base_map();
        vars.insert("BIND_ADDR".to_string(), "not-an-address".to_string());
        assert!(matches!(
            Config::from_map(&vars).unwrap_err(),
            ConfigError::Invalid("BIND_ADDR", _)
        ));
    }

    #[test]
    fn ensure_dirs_creates_layout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vars = base_map();
        vars.insert("DATA_DIR".to_string(), dir.path().display().to_string());
        let config = Config::from_map(&vars).expect("config");
        config.ensure_dirs().expect("dirs");
        assert!(config.covers_dir().is_dir());
        assert!(config.logs_dir().is_dir());
        assert!(config.db_path.parent().unwrap().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_data_dir_reports_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("readonly");
        std::fs::create_dir(&path).expect("mkdir");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).expect("chmod");

        let mut vars = base_map();
        vars.insert("DATA_DIR".to_string(), path.display().to_string());
        let config = Config::from_map(&vars).expect("config");
        let result = config.ensure_dirs();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("restore");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("不可写"));
    }
}
