use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebdConfig {
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
    #[serde(default)]
    pub production: bool,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub limits: LimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    #[serde(default = "default_admin_listen")]
    pub listen: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
    #[serde(default)]
    pub cookie_secure: bool,
    #[serde(default)]
    pub turnstile_enabled: bool,
    #[serde(default)]
    pub turnstile_required: bool,
    #[serde(default = "default_turnstile_site_key")]
    pub turnstile_site_key: String,
    #[serde(default)]
    pub turnstile_secret_key: Option<String>,
    #[serde(default = "default_turnstile_expected_hostname")]
    pub turnstile_expected_hostname: Option<String>,
    #[serde(default = "default_turnstile_expected_action")]
    pub turnstile_expected_action: Option<String>,
    #[serde(default = "default_turnstile_verify_url")]
    pub turnstile_verify_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitConfig {
    #[serde(default = "default_admin_body_bytes")]
    pub admin_body_bytes: usize,
}

impl Default for WebdConfig {
    fn default() -> Self {
        Self {
            database_path: default_database_path(),
            static_dir: default_static_dir(),
            production: true,
            admin: AdminConfig::default(),
            security: SecurityConfig::default(),
            limits: LimitConfig::default(),
        }
    }
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            listen: default_admin_listen(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            session_ttl_seconds: default_session_ttl_seconds(),
            cookie_secure: true,
            turnstile_enabled: false,
            turnstile_required: false,
            turnstile_site_key: default_turnstile_site_key(),
            turnstile_secret_key: None,
            turnstile_expected_hostname: default_turnstile_expected_hostname(),
            turnstile_expected_action: default_turnstile_expected_action(),
            turnstile_verify_url: default_turnstile_verify_url(),
        }
    }
}

impl Default for LimitConfig {
    fn default() -> Self {
        Self {
            admin_body_bytes: default_admin_body_bytes(),
        }
    }
}

impl WebdConfig {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let mut config = if let Some(path) = path {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("读取配置文件 {}", path.display()))?;
            toml::from_str::<WebdConfig>(&text)
                .with_context(|| format!("解析配置文件 {}", path.display()))?
        } else {
            WebdConfig::default()
        };

        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_DB") {
            if !value.trim().is_empty() {
                config.database_path = PathBuf::from(value);
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_STATIC_DIR") {
            if !value.trim().is_empty() {
                config.static_dir = PathBuf::from(value);
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_ADMIN_LISTEN") {
            if !value.trim().is_empty() {
                config.admin.listen = value.parse().context("解析 CC_SWITCH_WEBD_ADMIN_LISTEN")?;
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_SESSION_TTL_SECONDS") {
            if !value.trim().is_empty() {
                config.security.session_ttl_seconds = value
                    .parse()
                    .context("解析 CC_SWITCH_WEBD_SESSION_TTL_SECONDS")?;
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_ENABLED") {
            if !value.trim().is_empty() {
                config.security.turnstile_enabled =
                    parse_bool(&value).context("解析 CC_SWITCH_WEBD_TURNSTILE_ENABLED")?;
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_REQUIRED") {
            if !value.trim().is_empty() {
                config.security.turnstile_required =
                    parse_bool(&value).context("解析 CC_SWITCH_WEBD_TURNSTILE_REQUIRED")?;
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_SITE_KEY") {
            if !value.trim().is_empty() {
                config.security.turnstile_site_key = value;
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_SECRET_KEY") {
            if !value.trim().is_empty() {
                config.security.turnstile_secret_key = Some(value);
            }
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_EXPECTED_HOSTNAME") {
            config.security.turnstile_expected_hostname = optional_trimmed(value);
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_EXPECTED_ACTION") {
            config.security.turnstile_expected_action = optional_trimmed(value);
        }
        if let Ok(value) = std::env::var("CC_SWITCH_WEBD_TURNSTILE_VERIFY_URL") {
            if !value.trim().is_empty() {
                config.security.turnstile_verify_url = value;
            }
        }

        if config.security.session_ttl_seconds < 300 {
            config.security.session_ttl_seconds = 300;
        }

        if config.production && !config.admin.listen.ip().is_loopback() {
            anyhow::bail!(
                "production=true 时 admin.listen 必须绑定到 loopback，再由 nginx 暴露 HTTPS"
            );
        }

        Ok(config)
    }
}

fn default_database_path() -> PathBuf {
    PathBuf::from("/var/lib/cc-switch-webd/cc-switch.db")
}

fn default_static_dir() -> PathBuf {
    PathBuf::from("/usr/share/cc-switch-webd/webui")
}

fn default_admin_listen() -> SocketAddr {
    "127.0.0.1:15722".parse().expect("valid admin listen")
}

fn default_session_ttl_seconds() -> u64 {
    31_536_000
}

fn default_admin_body_bytes() -> usize {
    1024 * 1024
}

fn default_turnstile_site_key() -> String {
    "0x4AAAAAADPfCPB_O-N3j6ON".to_string()
}

fn default_turnstile_expected_hostname() -> Option<String> {
    Some("661313.xyz".to_string())
}

fn default_turnstile_expected_action() -> Option<String> {
    Some("login".to_string())
}

fn default_turnstile_verify_url() -> String {
    "https://challenges.cloudflare.com/turnstile/v0/siteverify".to_string()
}

fn optional_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => anyhow::bail!("布尔值无效: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    const ENV_KEYS: &[&str] = &[
        "CC_SWITCH_WEBD_DB",
        "CC_SWITCH_WEBD_STATIC_DIR",
        "CC_SWITCH_WEBD_ADMIN_LISTEN",
        "CC_SWITCH_WEBD_SESSION_TTL_SECONDS",
        "CC_SWITCH_WEBD_TURNSTILE_ENABLED",
        "CC_SWITCH_WEBD_TURNSTILE_REQUIRED",
        "CC_SWITCH_WEBD_TURNSTILE_SITE_KEY",
        "CC_SWITCH_WEBD_TURNSTILE_SECRET_KEY",
        "CC_SWITCH_WEBD_TURNSTILE_EXPECTED_HOSTNAME",
        "CC_SWITCH_WEBD_TURNSTILE_EXPECTED_ACTION",
        "CC_SWITCH_WEBD_TURNSTILE_VERIFY_URL",
    ];

    struct EnvGuard(Vec<(&'static str, Option<String>)>);

    impl EnvGuard {
        fn clear() -> Self {
            let saved = ENV_KEYS
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for key in ENV_KEYS {
                std::env::remove_var(key);
            }
            Self(saved)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    #[serial]
    fn default_security_uses_cloud_session_ttl() {
        let _guard = EnvGuard::clear();
        let config = WebdConfig::load(None).expect("load config");

        assert_eq!(config.security.session_ttl_seconds, 31_536_000);
    }

    #[test]
    #[serial]
    fn production_rejects_non_loopback_admin_listener() {
        let _guard = EnvGuard::clear();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
production = true

[admin]
listen = "0.0.0.0:15722"
"#,
        )
        .expect("write config");

        let err = WebdConfig::load(Some(config_path)).expect_err("non-loopback must be rejected");
        assert!(err.to_string().contains("loopback"));
    }

    #[test]
    #[serial]
    fn env_overrides_default_paths_and_listener() {
        let _guard = EnvGuard::clear();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("cc-switch.db");
        let static_dir = dir.path().join("webui");
        std::env::set_var("CC_SWITCH_WEBD_DB", db.to_string_lossy().as_ref());
        std::env::set_var(
            "CC_SWITCH_WEBD_STATIC_DIR",
            static_dir.to_string_lossy().as_ref(),
        );
        std::env::set_var("CC_SWITCH_WEBD_ADMIN_LISTEN", "127.0.0.1:17000");

        let config = WebdConfig::load(None).expect("load config");

        assert_eq!(config.database_path, db);
        assert_eq!(config.static_dir, static_dir);
        assert_eq!(config.admin.listen.to_string(), "127.0.0.1:17000");
    }

    #[test]
    #[serial]
    fn env_overrides_turnstile_security() {
        let _guard = EnvGuard::clear();
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_ENABLED", "true");
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_REQUIRED", "false");
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_SITE_KEY", "site-key");
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_SECRET_KEY", "secret-key");
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_EXPECTED_HOSTNAME", "example.com");
        std::env::set_var("CC_SWITCH_WEBD_TURNSTILE_EXPECTED_ACTION", "login");
        std::env::set_var("CC_SWITCH_WEBD_SESSION_TTL_SECONDS", "31536000");

        let config = WebdConfig::load(None).expect("load config");

        assert!(config.security.turnstile_enabled);
        assert!(!config.security.turnstile_required);
        assert_eq!(config.security.turnstile_site_key, "site-key");
        assert_eq!(
            config.security.turnstile_secret_key.as_deref(),
            Some("secret-key")
        );
        assert_eq!(
            config.security.turnstile_expected_hostname.as_deref(),
            Some("example.com")
        );
        assert_eq!(
            config.security.turnstile_expected_action.as_deref(),
            Some("login")
        );
        assert_eq!(config.security.session_ttl_seconds, 31_536_000);
    }
}
