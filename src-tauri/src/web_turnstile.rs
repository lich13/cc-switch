use crate::{web_auth, web_config::SecurityConfig, Database};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnstileLoginAction {
    Skip,
    Verify,
    FailClosed,
}

pub fn turnstile_login_action(enabled: bool, required: bool) -> TurnstileLoginAction {
    if enabled {
        TurnstileLoginAction::Verify
    } else if required {
        TurnstileLoginAction::FailClosed
    } else {
        TurnstileLoginAction::Skip
    }
}

#[derive(Debug, Deserialize)]
struct TurnstileResponse {
    success: bool,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

pub async fn verify_turnstile(
    db: &Database,
    config: &SecurityConfig,
    token: &str,
    remote_ip: Option<&str>,
) -> Result<()> {
    if token.trim().is_empty() {
        anyhow::bail!("turnstile token is empty");
    }
    if web_auth::turnstile_token_seen(db, token)? {
        anyhow::bail!("turnstile token has already been used");
    }
    let secret = config
        .turnstile_secret_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("turnstile secret is not configured")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .context("build turnstile client")?;
    let mut params = vec![
        ("secret", secret.to_string()),
        ("response", token.to_string()),
    ];
    if let Some(ip) = remote_ip.map(str::trim).filter(|value| !value.is_empty()) {
        params.push(("remoteip", ip.to_string()));
    }

    let response = client
        .post(&config.turnstile_verify_url)
        .form(&params)
        .send()
        .await
        .context("verify turnstile")?;
    let parsed = response
        .json::<TurnstileResponse>()
        .await
        .context("decode turnstile")?;

    web_auth::record_turnstile_attempt(
        db,
        token,
        parsed.action.as_deref().unwrap_or(""),
        parsed.hostname.as_deref(),
        remote_ip,
        parsed.success,
        &parsed.error_codes,
    )?;

    if !parsed.success {
        anyhow::bail!("{}", turnstile_failure_message(&parsed.error_codes));
    }
    if let Some(expected) = config
        .turnstile_expected_hostname
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if parsed.hostname.as_deref() != Some(expected) {
            anyhow::bail!("turnstile hostname mismatch");
        }
    }
    if let Some(expected) = config
        .turnstile_expected_action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if parsed.action.as_deref() != Some(expected) {
            anyhow::bail!("turnstile action mismatch");
        }
    }

    Ok(())
}

pub fn turnstile_failure_message(error_codes: &[String]) -> String {
    if error_codes
        .iter()
        .any(|code| code == "invalid-input-secret")
    {
        return "turnstile verification failed: Turnstile Secret 配置无效".to_string();
    }
    if error_codes
        .iter()
        .any(|code| code == "invalid-input-response" || code == "missing-input-response")
    {
        return "turnstile verification failed: Turnstile token 无效或缺失，请刷新页面后重试"
            .to_string();
    }
    if error_codes
        .iter()
        .any(|code| code == "timeout-or-duplicate")
    {
        return "turnstile verification failed: Turnstile token 已过期或重复使用，请重新验证"
            .to_string();
    }
    if error_codes.is_empty() {
        "turnstile verification failed: Cloudflare 未返回具体错误".to_string()
    } else {
        format!(
            "turnstile verification failed: Cloudflare 返回 {}",
            error_codes.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_auth;
    use axum::{routing::post, Json, Router};
    use serde_json::json;

    fn test_config(verify_url: String) -> SecurityConfig {
        SecurityConfig {
            turnstile_enabled: true,
            turnstile_required: false,
            turnstile_secret_key: Some("secret-one".to_string()),
            turnstile_verify_url: verify_url,
            ..SecurityConfig::default()
        }
    }

    #[test]
    fn login_action_matches_codex_cloud_panel_semantics() {
        assert_eq!(
            turnstile_login_action(false, false),
            TurnstileLoginAction::Skip
        );
        assert_eq!(
            turnstile_login_action(true, false),
            TurnstileLoginAction::Verify
        );
        assert_eq!(
            turnstile_login_action(true, true),
            TurnstileLoginAction::Verify
        );
        assert_eq!(
            turnstile_login_action(false, true),
            TurnstileLoginAction::FailClosed
        );
    }

    #[tokio::test]
    async fn verify_turnstile_accepts_success_and_records_replay_guard() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock");
        let addr = listener.local_addr().expect("local addr");
        let app = Router::new().route(
            "/siteverify",
            post(|| async {
                Json(json!({
                    "success": true,
                    "hostname": "661313.xyz",
                    "action": "login",
                    "error-codes": []
                }))
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server");
        });

        let db = Database::memory().expect("db");
        let config = test_config(format!("http://{addr}/siteverify"));

        verify_turnstile(&db, &config, "token-1", Some("198.51.100.1"))
            .await
            .expect("verify");
        assert!(web_auth::turnstile_token_seen(&db, "token-1").expect("seen"));
        let replay = verify_turnstile(&db, &config, "token-1", Some("198.51.100.1"))
            .await
            .expect_err("replay rejected");
        assert!(replay.to_string().contains("already been used"));

        server.abort();
    }
}
