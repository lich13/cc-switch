use crate::database::{lock_conn, Database};
use crate::AppError;
use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use rand::{distributions::Alphanumeric, Rng};
use rusqlite::{params, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE: &str = "cc_switch_webd_session";
pub const CSRF_HEADER: &str = "x-csrf-token";
const LOGIN_FAILURE_LIMIT: i64 = 5;
const LOGIN_FAILURE_WINDOW_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMe {
    pub username: String,
    pub csrf_token: String,
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub admin_id: String,
    pub username: String,
    pub session_id: String,
    pub csrf_token_hash: String,
}

pub fn ensure_tables(db: &Database) -> Result<()> {
    let conn = lock_conn!(db.conn);
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS web_admins (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_login_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS web_sessions (
            id TEXT PRIMARY KEY,
            admin_id TEXT NOT NULL,
            token_hash TEXT NOT NULL UNIQUE,
            csrf_token_hash TEXT NOT NULL,
            user_agent_hash TEXT,
            ip TEXT,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            revoked_at INTEGER,
            FOREIGN KEY (admin_id) REFERENCES web_admins(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS web_audit_logs (
            id TEXT PRIMARY KEY,
            admin_id TEXT,
            action TEXT NOT NULL,
            target TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS web_login_failures (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            client_key TEXT NOT NULL,
            failed_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_web_login_failures_lookup
            ON web_login_failures(username, client_key, failed_at);
        CREATE TABLE IF NOT EXISTS web_turnstile_attempts (
            token_hash TEXT PRIMARY KEY,
            action TEXT,
            hostname TEXT,
            remote_ip TEXT,
            success INTEGER NOT NULL,
            error_codes TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

pub fn admin_exists(db: &Database) -> Result<bool> {
    ensure_tables(db)?;
    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_admins",
        params![],
        |row: &Row<'_>| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn init_admin(db: &Database, username: &str, password: &str) -> Result<()> {
    ensure_tables(db)?;
    let username = username.trim();
    validate_admin_input(username, password)?;
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_admins",
        params![],
        |row: &Row<'_>| row.get(0),
    )?;
    if count > 0 {
        return Err(anyhow!("管理员已存在，请使用 reset-password"));
    }
    conn.execute(
        "INSERT INTO web_admins (id, username, password_hash, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![
            uuid::Uuid::new_v4().to_string(),
            username,
            hash_password(password)?,
            now
        ],
    )?;
    Ok(())
}

pub fn reset_password(db: &Database, username: &str, password: &str) -> Result<()> {
    ensure_tables(db)?;
    let username = username.trim();
    validate_admin_input(username, password)?;
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    let changed = conn.execute(
        "UPDATE web_admins SET password_hash = ?1, updated_at = ?2 WHERE username = ?3",
        params![hash_password(password)?, now, username],
    )?;
    if changed == 0 {
        return Err(anyhow!("管理员不存在: {username}"));
    }
    conn.execute(
        "UPDATE web_sessions SET revoked_at = ?1 WHERE revoked_at IS NULL",
        params![now],
    )?;
    Ok(())
}

pub fn login(
    db: &Database,
    username: &str,
    password: &str,
    ttl_seconds: u64,
    client_key: Option<&str>,
) -> Result<(AuthMe, String)> {
    ensure_tables(db)?;
    let username = username.trim();
    let now = Utc::now().timestamp();
    let client_key = normalize_client_key(client_key);
    let conn = lock_conn!(db.conn);
    prune_login_failures(&conn, now)?;
    ensure_login_not_limited(&conn, username, &client_key, now)?;
    let (admin_id, stored_username, password_hash): (String, String, String) = conn
        .query_row(
            "SELECT id, username, password_hash FROM web_admins WHERE username = ?1",
            params![username],
            |row: &Row<'_>| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| {
            let _ = record_login_failure(&conn, username, &client_key, now);
            anyhow!("用户名或密码错误")
        })?;

    if !verify_password(password, &password_hash) {
        record_login_failure(&conn, username, &client_key, now)?;
        return Err(anyhow!("用户名或密码错误"));
    }

    let token = random_token();
    let csrf = random_token();
    let session_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO web_sessions (
            id, admin_id, token_hash, csrf_token_hash, created_at, expires_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session_id,
            admin_id,
            hash_token(&token),
            hash_token(&csrf),
            now,
            now + ttl_seconds as i64
        ],
    )?;
    conn.execute(
        "UPDATE web_admins SET last_login_at = ?1 WHERE username = ?2",
        params![now, stored_username],
    )?;
    clear_login_failures(&conn, username, &client_key)?;

    Ok((
        AuthMe {
            username: stored_username,
            csrf_token: csrf,
        },
        token,
    ))
}

pub fn logout(db: &Database, headers: &HeaderMap) -> Result<()> {
    ensure_tables(db)?;
    let Some(token) = extract_cookie(headers, SESSION_COOKIE) else {
        return Ok(());
    };
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "UPDATE web_sessions SET revoked_at = ?1 WHERE token_hash = ?2",
        params![now, hash_token(&token)],
    )?;
    Ok(())
}

pub fn require_auth(db: &Database, headers: &HeaderMap) -> Result<AuthContext, StatusCode> {
    ensure_tables(db).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let token = extract_cookie(headers, SESSION_COOKIE).ok_or(StatusCode::UNAUTHORIZED)?;
    let now = Utc::now().timestamp();
    let conn = db.conn.lock().map_err(|_| StatusCode::UNAUTHORIZED)?;
    conn.query_row(
        "SELECT s.id, s.admin_id, a.username, s.csrf_token_hash
         FROM web_sessions s
         JOIN web_admins a ON a.id = s.admin_id
         WHERE s.token_hash = ?1
           AND s.revoked_at IS NULL
           AND s.expires_at > ?2",
        params![hash_token(&token), now],
        |row: &Row<'_>| {
            Ok(AuthContext {
                session_id: row.get(0)?,
                admin_id: row.get(1)?,
                username: row.get(2)?,
                csrf_token_hash: row.get(3)?,
            })
        },
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)
}

pub fn record_audit(
    db: &Database,
    admin_id: Option<&str>,
    action: &str,
    target: Option<&str>,
    metadata: serde_json::Value,
) -> Result<()> {
    ensure_tables(db)?;
    let conn = lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO web_audit_logs (id, admin_id, action, target, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid::Uuid::new_v4().to_string(),
            admin_id,
            action,
            target,
            serde_json::to_string(&metadata)?,
            Utc::now().timestamp()
        ],
    )?;
    Ok(())
}

pub fn require_csrf(headers: &HeaderMap, auth: &AuthContext) -> Result<(), StatusCode> {
    let csrf = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    if hash_token(csrf) == auth.csrf_token_hash {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

pub fn refresh_csrf(db: &Database, auth: &AuthContext) -> Result<String> {
    let csrf = random_token();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "UPDATE web_sessions SET csrf_token_hash = ?1 WHERE id = ?2 AND revoked_at IS NULL",
        params![hash_token(&csrf), auth.session_id],
    )?;
    Ok(csrf)
}

pub fn turnstile_token_seen(db: &Database, token: &str) -> Result<bool> {
    ensure_tables(db)?;
    prune_turnstile_attempts(db)?;
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_turnstile_attempts WHERE token_hash = ?1 AND expires_at > ?2",
        params![hash_token(token), now],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn record_turnstile_attempt(
    db: &Database,
    token: &str,
    action: &str,
    hostname: Option<&str>,
    remote_ip: Option<&str>,
    success: bool,
    error_codes: &[String],
) -> Result<()> {
    ensure_tables(db)?;
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    conn.execute(
        r#"
        INSERT OR IGNORE INTO web_turnstile_attempts
            (token_hash, action, hostname, remote_ip, success, error_codes, created_at, expires_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            hash_token(token),
            action,
            hostname,
            remote_ip,
            if success { 1 } else { 0 },
            serde_json::to_string(error_codes)?,
            now,
            now + 600
        ],
    )?;
    Ok(())
}

pub fn session_cookie(token: &str, secure: bool, max_age_seconds: u64) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn expired_session_cookie(secure: bool) -> String {
    let mut cookie = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn prune_turnstile_attempts(db: &Database) -> Result<()> {
    let now = Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "DELETE FROM web_turnstile_attempts WHERE expires_at <= ?1",
        params![now],
    )?;
    Ok(())
}

pub fn json_error(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        status,
        axum::Json(serde_json::json!({ "error": message.into() })),
    )
        .into_response()
}

fn validate_admin_input(username: &str, password: &str) -> Result<()> {
    if username.is_empty() {
        return Err(anyhow!("用户名不能为空"));
    }
    if password.len() < 12 {
        return Err(anyhow!("密码至少需要 12 个字符"));
    }
    Ok(())
}

fn normalize_client_key(client_key: Option<&str>) -> String {
    client_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .chars()
        .take(128)
        .collect()
}

fn prune_login_failures(conn: &rusqlite::Connection, now: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM web_login_failures WHERE failed_at < ?1",
        params![now - LOGIN_FAILURE_WINDOW_SECONDS],
    )?;
    Ok(())
}

fn ensure_login_not_limited(
    conn: &rusqlite::Connection,
    username: &str,
    client_key: &str,
    now: i64,
) -> Result<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_login_failures
         WHERE username = ?1 AND client_key = ?2 AND failed_at >= ?3",
        params![username, client_key, now - LOGIN_FAILURE_WINDOW_SECONDS],
        |row| row.get(0),
    )?;
    if count >= LOGIN_FAILURE_LIMIT {
        return Err(anyhow!("登录失败次数过多，请 15 分钟后再试"));
    }
    Ok(())
}

fn record_login_failure(
    conn: &rusqlite::Connection,
    username: &str,
    client_key: &str,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO web_login_failures (id, username, client_key, failed_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![uuid::Uuid::new_v4().to_string(), username, client_key, now],
    )?;
    Ok(())
}

fn clear_login_failures(
    conn: &rusqlite::Connection,
    username: &str,
    client_key: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM web_login_failures WHERE username = ?1 AND client_key = ?2",
        params![username, client_key],
    )?;
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("hash password: {e}"))?
        .to_string())
}

fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

fn random_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in value.split(';') {
        let part = part.trim();
        let (cookie_name, cookie_value) = part.split_once('=')?;
        if cookie_name == name {
            return Some(cookie_value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    fn temp_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::init_at(dir.path().join("cc-switch.db")).expect("init database");
        (dir, db)
    }

    fn cookie_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{SESSION_COOKIE}={token}").parse().expect("cookie"),
        );
        headers
    }

    #[test]
    fn admin_login_csrf_refresh_and_logout_lifecycle() {
        let (_dir, db) = temp_db();
        init_admin(&db, "admin", "very-secure-password").expect("init admin");
        assert!(admin_exists(&db).expect("admin exists"));

        let (me, token) = login(
            &db,
            "admin",
            "very-secure-password",
            3600,
            Some("127.0.0.1"),
        )
        .expect("login");
        assert_eq!(me.username, "admin");
        assert!(!me.csrf_token.is_empty());

        let mut headers = cookie_headers(&token);
        let auth = require_auth(&db, &headers).expect("auth");
        assert_eq!(auth.username, "admin");
        assert_eq!(require_csrf(&headers, &auth), Err(StatusCode::FORBIDDEN));

        headers.insert(CSRF_HEADER, me.csrf_token.parse().expect("csrf"));
        require_csrf(&headers, &auth).expect("csrf accepted");

        let refreshed = refresh_csrf(&db, &auth).expect("refresh csrf");
        let refreshed_auth = require_auth(&db, &headers).expect("auth after refresh");
        assert_eq!(
            require_csrf(&headers, &refreshed_auth),
            Err(StatusCode::FORBIDDEN)
        );
        headers.insert(CSRF_HEADER, refreshed.parse().expect("new csrf"));
        require_csrf(&headers, &refreshed_auth).expect("new csrf accepted");

        logout(&db, &headers).expect("logout");
        assert!(matches!(
            require_auth(&db, &headers),
            Err(StatusCode::UNAUTHORIZED)
        ));
    }

    #[test]
    fn reset_password_revokes_existing_sessions() {
        let (_dir, db) = temp_db();
        init_admin(&db, "admin", "very-secure-password").expect("init admin");
        let (_me, token) = login(
            &db,
            "admin",
            "very-secure-password",
            3600,
            Some("127.0.0.1"),
        )
        .expect("login");
        let headers = cookie_headers(&token);
        require_auth(&db, &headers).expect("auth before reset");

        reset_password(&db, "admin", "new-secure-password").expect("reset password");

        assert!(matches!(
            require_auth(&db, &headers),
            Err(StatusCode::UNAUTHORIZED)
        ));
        assert!(login(
            &db,
            "admin",
            "very-secure-password",
            3600,
            Some("127.0.0.1")
        )
        .is_err());
        assert!(login(&db, "admin", "new-secure-password", 3600, Some("127.0.0.1")).is_ok());
    }

    #[test]
    fn repeated_failed_logins_are_rate_limited_and_success_clears_failures() {
        let (_dir, db) = temp_db();
        init_admin(&db, "admin", "very-secure-password").expect("init admin");

        for _ in 0..LOGIN_FAILURE_LIMIT {
            assert!(login(&db, "admin", "wrong-password", 3600, Some("198.51.100.1")).is_err());
        }

        let limited = login(
            &db,
            "admin",
            "very-secure-password",
            3600,
            Some("198.51.100.1"),
        )
        .expect_err("rate limited");
        assert!(limited.to_string().contains("登录失败次数过多"));

        assert!(login(
            &db,
            "admin",
            "very-secure-password",
            3600,
            Some("198.51.100.2")
        )
        .is_ok());
    }

    #[test]
    fn turnstile_attempt_prevents_replay_and_hashes_token() -> Result<()> {
        let (_dir, db) = temp_db();

        assert!(!turnstile_token_seen(&db, "token-1").expect("first lookup"));
        record_turnstile_attempt(
            &db,
            "token-1",
            "login",
            Some("661313.xyz"),
            Some("198.51.100.1"),
            true,
            &[],
        )
        .expect("record attempt");

        assert!(turnstile_token_seen(&db, "token-1").expect("second lookup"));
        let conn = lock_conn!(db.conn);
        let raw_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM web_turnstile_attempts WHERE token_hash = ?1",
                params!["token-1"],
                |row| row.get(0),
            )
            .expect("raw token lookup");
        let hashed_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM web_turnstile_attempts WHERE token_hash = ?1",
                params![hash_token("token-1")],
                |row| row.get(0),
            )
            .expect("hashed token lookup");
        assert_eq!(raw_count, 0);
        assert_eq!(hashed_count, 1);
        Ok(())
    }
}
