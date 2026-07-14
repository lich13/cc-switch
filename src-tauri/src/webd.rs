use crate::database::Sub2apiProviderSelection;
use crate::store::AppState;
use crate::web_auth::{self, AuthMe};
use crate::web_config::WebdConfig;
use crate::web_turnstile::{self, TurnstileLoginAction};
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower_http::{
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct WebdState {
    pub app_state: AppState,
    pub config: WebdConfig,
}

#[derive(Debug, Serialize)]
struct PublicSettings {
    version: &'static str,
    production: bool,
    #[serde(rename = "appName")]
    app_name: &'static str,
    #[serde(rename = "adminInitialized")]
    admin_initialized: bool,
    #[serde(rename = "admin_configured")]
    admin_configured: bool,
    #[serde(rename = "turnstile_enabled")]
    turnstile_enabled: bool,
    #[serde(rename = "turnstile_required")]
    turnstile_required: bool,
    #[serde(rename = "turnstile_site_key")]
    turnstile_site_key: String,
    #[serde(rename = "turnstile_action")]
    turnstile_action: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
    turnstile_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sub2apiSelectedExportRequest {
    selected_providers: Vec<Sub2apiProviderSelection>,
}

pub async fn serve(config: WebdConfig, app_state: AppState) -> Result<()> {
    let addr = config.admin.listen;
    let listener = TcpListener::bind(addr).await?;
    log::info!("cc-switch-webd admin listener bound on {addr}");

    axum::serve(listener, router(config, app_state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

pub fn router(config: WebdConfig, app_state: AppState) -> Router {
    let body_limit = config.limits.admin_body_bytes;
    let static_dir = config.static_dir.clone();
    let index = static_dir.join("index.html");
    let state = WebdState { app_state, config };

    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/public/settings", get(public_settings))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/admin/me", get(me))
        .route("/api/admin/rpc/:command", post(rpc))
        .route("/api/admin/providers/export", get(export_providers))
        .route(
            "/api/admin/providers/export/sub2api/candidates",
            get(list_sub2api_export_candidates),
        )
        .route(
            "/api/admin/providers/export/sub2api",
            get(export_providers_sub2api).post(export_providers_sub2api_selected),
        )
        .route("/api/admin/providers/import", post(import_providers))
        .layer(RequestBodyLimitLayer::new(body_limit));

    let spa = ServeDir::new(static_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .merge(api)
        .fallback_service(spa)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' https://challenges.cloudflare.com; connect-src 'self' https://challenges.cloudflare.com; frame-src https://challenges.cloudflare.com; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
            ),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        log::warn!("failed to listen for shutdown signal: {err}");
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<WebdState>) -> Response {
    if let Err(err) = web_auth::ensure_tables(&state.app_state.db) {
        return web_auth::json_error(StatusCode::SERVICE_UNAVAILABLE, err.to_string());
    }
    match state.app_state.proxy_service.get_config().await {
        Ok(_) => "ready".into_response(),
        Err(err) => web_auth::json_error(StatusCode::SERVICE_UNAVAILABLE, err),
    }
}

async fn public_settings(State(state): State<WebdState>) -> Response {
    match web_auth::admin_exists(&state.app_state.db) {
        Ok(admin_initialized) => Json(PublicSettings {
            version: env!("CARGO_PKG_VERSION"),
            production: state.config.production,
            app_name: "CC Switch WebUI",
            admin_initialized,
            admin_configured: admin_initialized,
            turnstile_enabled: state.config.security.turnstile_enabled,
            turnstile_required: state.config.security.turnstile_required,
            turnstile_site_key: state.config.security.turnstile_site_key.clone(),
            turnstile_action: state
                .config
                .security
                .turnstile_expected_action
                .clone()
                .unwrap_or_else(|| "login".to_string()),
        })
        .into_response(),
        Err(err) => web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn login(
    State(state): State<WebdState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let client_key = login_client_key(&headers);
    match web_turnstile::turnstile_login_action(
        state.config.security.turnstile_enabled,
        state.config.security.turnstile_required,
    ) {
        TurnstileLoginAction::Skip => {}
        TurnstileLoginAction::FailClosed => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                None,
                "login.turnstile_required",
                Some("admin"),
                json!({ "username": payload.username }),
            );
            return web_auth::json_error(StatusCode::FORBIDDEN, "turnstile is required");
        }
        TurnstileLoginAction::Verify => {
            let token = payload.turnstile_token.as_deref().unwrap_or("");
            let remote_ip = turnstile_remote_ip(&client_key);
            if let Err(err) = web_turnstile::verify_turnstile(
                &state.app_state.db,
                &state.config.security,
                token,
                remote_ip,
            )
            .await
            {
                let _ = web_auth::record_audit(
                    &state.app_state.db,
                    None,
                    "login.turnstile_failed",
                    Some("admin"),
                    json!({ "username": payload.username, "error": err.to_string() }),
                );
                return web_auth::json_error(StatusCode::UNAUTHORIZED, err.to_string());
            }
        }
    }
    match web_auth::login(
        &state.app_state.db,
        &payload.username,
        &payload.password,
        state.config.security.session_ttl_seconds,
        Some(client_key.as_str()),
    ) {
        Ok((me, token)) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                None,
                "login.success",
                Some("admin"),
                json!({ "username": me.username }),
            );
            let mut response = Json(me).into_response();
            match HeaderValue::from_str(&web_auth::session_cookie(
                &token,
                state.config.security.cookie_secure,
                state.config.security.session_ttl_seconds,
            )) {
                Ok(cookie) => {
                    response.headers_mut().insert(header::SET_COOKIE, cookie);
                    response
                }
                Err(err) => {
                    web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                }
            }
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                None,
                "login.failed",
                Some("admin"),
                json!({ "username": payload.username }),
            );
            web_auth::json_error(StatusCode::UNAUTHORIZED, err.to_string())
        }
    }
}

fn login_client_key(headers: &HeaderMap) -> String {
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.rsplit(',').next())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("direct")
        .chars()
        .take(128)
        .collect()
}

fn turnstile_remote_ip(client_key: &str) -> Option<&str> {
    client_key
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|_| client_key)
}

async fn logout(State(state): State<WebdState>, headers: HeaderMap) -> Response {
    match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => {
            if let Err(status) = web_auth::require_csrf(&headers, &auth) {
                return web_auth::json_error(status, "CSRF 校验失败");
            }
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "logout",
                Some("admin"),
                json!({ "username": auth.username }),
            );
        }
        Err(StatusCode::UNAUTHORIZED) => {}
        Err(status) => return web_auth::json_error(status, "认证失败"),
    }

    if let Err(err) = web_auth::logout(&state.app_state.db, &headers) {
        return web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    if let Ok(cookie) = HeaderValue::from_str(&web_auth::expired_session_cookie(
        state.config.security.cookie_secure,
    )) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

async fn me(State(state): State<WebdState>, headers: HeaderMap) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };
    match web_auth::refresh_csrf(&state.app_state.db, &auth) {
        Ok(csrf_token) => Json(AuthMe {
            username: auth.username,
            csrf_token,
        })
        .into_response(),
        Err(err) => web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn rpc(
    State(state): State<WebdState>,
    Path(command): Path<String>,
    headers: HeaderMap,
    Json(args): Json<Value>,
) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };
    if let Err(status) = web_auth::require_csrf(&headers, &auth) {
        return web_auth::json_error(status, "CSRF 校验失败");
    }

    match crate::web_rpc::dispatch(&state.app_state, &command, args, state.config.production).await
    {
        Ok(data) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "rpc",
                Some(&command),
                json!({ "username": auth.username }),
            );
            Json(json!({ "data": data })).into_response()
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "rpc.failed",
                Some(&command),
                json!({ "username": auth.username, "error": err }),
            );
            web_auth::json_error(StatusCode::BAD_REQUEST, err)
        }
    }
}

async fn export_providers(State(state): State<WebdState>, headers: HeaderMap) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };

    match state.app_state.db.export_providers_json_string() {
        Ok(body) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export",
                Some("admin"),
                json!({ "username": auth.username }),
            );
            let filename = format!(
                "cc-switch-providers-{}.json",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            );
            let mut response = body.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            if let Ok(value) =
                HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.failed",
                Some("admin"),
                json!({ "username": auth.username, "error": err.to_string() }),
            );
            web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        }
    }
}

async fn export_providers_sub2api(State(state): State<WebdState>, headers: HeaderMap) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };

    match state.app_state.db.export_providers_sub2api_json_string() {
        Ok(body) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api",
                Some("admin"),
                json!({ "username": auth.username }),
            );
            let filename = format!(
                "sub2api-account-{}.json",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            );
            let mut response = body.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            if let Ok(value) =
                HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api.failed",
                Some("admin"),
                json!({ "username": auth.username, "error": err.to_string() }),
            );
            web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        }
    }
}

async fn list_sub2api_export_candidates(
    State(state): State<WebdState>,
    headers: HeaderMap,
) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };

    match state.app_state.db.list_sub2api_export_candidates() {
        Ok(candidates) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api.candidates",
                Some("admin"),
                json!({ "username": auth.username, "candidateCount": candidates.len() }),
            );
            Json(json!({ "candidates": candidates })).into_response()
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api.candidates.failed",
                Some("admin"),
                json!({ "username": auth.username, "error": err.to_string() }),
            );
            web_auth::json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        }
    }
}

async fn export_providers_sub2api_selected(
    State(state): State<WebdState>,
    headers: HeaderMap,
    Json(payload): Json<Sub2apiSelectedExportRequest>,
) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };
    if let Err(status) = web_auth::require_csrf(&headers, &auth) {
        return web_auth::json_error(status, "CSRF 校验失败");
    }

    match state
        .app_state
        .db
        .export_providers_sub2api_json_string_for_selection(&payload.selected_providers)
    {
        Ok(body) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api.selected",
                Some("admin"),
                json!({
                    "username": auth.username,
                    "selectedCount": payload.selected_providers.len(),
                }),
            );
            sub2api_download_response(body)
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.export.sub2api.selected.failed",
                Some("admin"),
                json!({ "username": auth.username, "error": err.to_string() }),
            );
            web_auth::json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
    }
}

fn sub2api_download_response(body: String) -> Response {
    let filename = format!(
        "sub2api-account-{}.json",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    );
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

async fn import_providers(
    State(state): State<WebdState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let auth = match web_auth::require_auth(&state.app_state.db, &headers) {
        Ok(auth) => auth,
        Err(status) => return web_auth::json_error(status, "认证失败"),
    };
    if let Err(status) = web_auth::require_csrf(&headers, &auth) {
        return web_auth::json_error(status, "CSRF 校验失败");
    }

    let raw = match std::str::from_utf8(&body) {
        Ok(raw) => raw,
        Err(err) => {
            return web_auth::json_error(
                StatusCode::BAD_REQUEST,
                format!("供应商导入文件必须是 UTF-8 JSON: {err}"),
            )
        }
    };

    match state.app_state.db.import_providers_json_string(raw) {
        Ok(summary) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.import",
                Some("admin"),
                json!({
                    "username": auth.username,
                    "providerCount": summary.provider_count,
                    "providerEndpointCount": summary.provider_endpoint_count,
                    "universalProviderCount": summary.universal_provider_count,
                }),
            );
            Json(json!({
                "ok": true,
                "backupId": summary.backup_id,
                "providerCount": summary.provider_count,
                "providerEndpointCount": summary.provider_endpoint_count,
                "universalProviderCount": summary.universal_provider_count
            }))
            .into_response()
        }
        Err(err) => {
            let _ = web_auth::record_audit(
                &state.app_state.db,
                Some(&auth.admin_id),
                "providers.import.failed",
                Some("admin"),
                json!({ "username": auth.username, "error": err.to_string() }),
            );
            web_auth::json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{web_auth, Database, Provider};
    use axum::{
        body::Body,
        http::{header, Method, Request},
    };
    use http_body_util::BodyExt as _;
    use serde_json::Value;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_router() -> (tempfile::TempDir, Router) {
        test_router_with_config(|_| {})
    }

    fn test_router_with_config(
        configure: impl FnOnce(&mut WebdConfig),
    ) -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("index.html"), "ok").expect("index");
        let db = Arc::new(Database::init_at(dir.path().join("cc-switch.db")).expect("init db"));
        web_auth::init_admin(&db, "admin", "very-secure-password").expect("init admin");

        let mut config = WebdConfig {
            production: false,
            static_dir: dir.path().to_path_buf(),
            security: crate::web_config::SecurityConfig {
                cookie_secure: false,
                ..Default::default()
            },
            ..Default::default()
        };
        configure(&mut config);

        (dir, router(config, AppState::new(db)))
    }

    fn test_router_with_db(
        configure: impl FnOnce(&mut WebdConfig),
    ) -> (tempfile::TempDir, Router, Arc<Database>) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("index.html"), "ok").expect("index");
        let db = Arc::new(Database::init_at(dir.path().join("cc-switch.db")).expect("init db"));
        web_auth::init_admin(&db, "admin", "very-secure-password").expect("init admin");

        let mut config = WebdConfig {
            production: false,
            static_dir: dir.path().to_path_buf(),
            security: crate::web_config::SecurityConfig {
                cookie_secure: false,
                ..Default::default()
            },
            ..Default::default()
        };
        configure(&mut config);

        let app = router(config, AppState::new(db.clone()));
        (dir, app, db)
    }

    fn json_request(method: Method, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    fn authed_json_request(
        method: Method,
        uri: &str,
        body: Value,
        cookie: Option<&str>,
        csrf: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            builder = builder.header(web_auth::CSRF_HEADER, csrf);
        }
        builder.body(Body::from(body.to_string())).expect("request")
    }

    async fn json_response(response: Response) -> (StatusCode, HeaderMap, Value) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!({ "text": String::from_utf8_lossy(&bytes).to_string() }))
        };
        (status, headers, value)
    }

    async fn raw_response(response: Response) -> (StatusCode, HeaderMap, Vec<u8>) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, headers, bytes)
    }

    fn session_cookie(headers: &HeaderMap) -> String {
        headers
            .get(header::SET_COOKIE)
            .expect("set-cookie")
            .to_str()
            .expect("cookie str")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string()
    }

    #[tokio::test]
    async fn router_enforces_login_csrf_and_rpc_allowlist() {
        let (_dir, app) = test_router();

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/admin/rpc/get_settings",
                json!({}),
            ))
            .await
            .expect("unauth rpc");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let cookie = session_cookie(&headers);
        let login_csrf = body
            .get("csrfToken")
            .and_then(Value::as_str)
            .expect("login csrf")
            .to_string();

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/get_settings",
                json!({}),
                Some(&cookie),
                None,
            ))
            .await
            .expect("missing csrf");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/me")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("me request"),
            )
            .await
            .expect("me");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("username").and_then(Value::as_str), Some("admin"));
        let refreshed_csrf = body
            .get("csrfToken")
            .and_then(Value::as_str)
            .expect("refreshed csrf")
            .to_string();
        assert_ne!(login_csrf, refreshed_csrf);

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/get_settings",
                json!({}),
                Some(&cookie),
                Some(&refreshed_csrf),
            ))
            .await
            .expect("allowed rpc");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("data").is_some());

        let response = app
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/open_external",
                json!({ "url": "https://example.com" }),
                Some(&cookie),
                Some(&refreshed_csrf),
            ))
            .await
            .expect("desktop rpc");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body
            .get("error")
            .and_then(Value::as_str)
            .expect("error")
            .contains("桌面专属命令"));
    }

    #[tokio::test]
    async fn v317_rpc_commands_keep_web_security_boundaries() {
        let (_dir, app) = test_router();

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/admin/rpc/ensure_codex_official_provider",
                json!({}),
            ))
            .await
            .expect("unauthenticated v3.17 RPC");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let cookie = session_cookie(&headers);
        let csrf = body
            .get("csrfToken")
            .and_then(Value::as_str)
            .expect("login csrf")
            .to_string();

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/update_toml_common_config_snippet",
                json!({
                    "configToml": "model = \"gpt-5.6\"\n",
                    "snippetToml": "[tui]\nnotifications = false\n",
                    "enabled": true
                }),
                Some(&cookie),
                None,
            ))
            .await
            .expect("v3.17 RPC without CSRF");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/ensure_codex_official_provider",
                json!({}),
                Some(&cookie),
                Some(&csrf),
            ))
            .await
            .expect("ensure Codex official provider");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("data"), Some(&Value::Bool(true)));

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/update_toml_common_config_snippet",
                json!({
                    "configToml": "experimental_bearer_token = \"sk-http-secret\"\n",
                    "snippetToml": "[tui]\nnotifications = false\n",
                    "enabled": true
                }),
                Some(&cookie),
                Some(&csrf),
            ))
            .await
            .expect("update TOML common config snippet");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let updated = body
            .get("data")
            .and_then(Value::as_str)
            .expect("updated TOML response");
        assert!(updated.contains("[tui]"));
        assert!(updated.contains("secret_configured"));
        assert!(!updated.contains("sk-http-secret"));

        let response = app
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/rpc/list_profiles",
                json!({}),
                Some(&cookie),
                Some(&csrf),
            ))
            .await
            .expect("Project Profiles RPC");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body
            .get("error")
            .and_then(Value::as_str)
            .expect("Project Profiles rejection")
            .contains("桌面专属命令"));
    }

    #[tokio::test]
    async fn providers_export_import_endpoints_require_auth_and_csrf() {
        let (_dir, app, db) = test_router_with_db(|_| {});
        let provider = Provider::with_id(
            "web-provider".to_string(),
            "Web Provider".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://web.example", "ANTHROPIC_AUTH_TOKEN": "key" } }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("seed provider");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export")
                    .body(Body::empty())
                    .expect("export request"),
            )
            .await
            .expect("unauth export");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let cookie = session_cookie(&headers);
        let csrf = body
            .get("csrfToken")
            .and_then(Value::as_str)
            .expect("csrf")
            .to_string();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("export request"),
            )
            .await
            .expect("export");
        let (status, headers, body) = raw_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
        assert!(headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .expect("content disposition")
            .starts_with("attachment; filename=\"cc-switch-providers-"));
        let envelope: Value = serde_json::from_slice(&body).expect("export json");
        assert_eq!(
            envelope.get("format").and_then(Value::as_str),
            Some("cc-switch-providers-export")
        );
        let provider_export_body = body;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export/sub2api")
                    .body(Body::empty())
                    .expect("unauth sub2api export request"),
            )
            .await
            .expect("unauth sub2api export");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export/sub2api")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("sub2api export request"),
            )
            .await
            .expect("sub2api export");
        let (status, headers, sub2api_body) = raw_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
        assert!(headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .expect("sub2api content disposition")
            .starts_with("attachment; filename=\"sub2api-account-"));
        let sub2api: Value = serde_json::from_slice(&sub2api_body).expect("sub2api json");
        assert!(sub2api.get("exported_at").and_then(Value::as_str).is_some());
        assert_eq!(
            sub2api
                .get("proxies")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );
        let accounts = sub2api
            .get("accounts")
            .and_then(Value::as_array)
            .expect("sub2api accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].get("name").and_then(Value::as_str),
            Some("Web Provider")
        );
        assert_eq!(
            accounts[0]
                .pointer("/credentials/api_key")
                .and_then(Value::as_str),
            Some("key")
        );
        assert_eq!(
            accounts[0]
                .pointer("/credentials/base_url")
                .and_then(Value::as_str),
            Some("https://web.example")
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/providers/import")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(provider_export_body.clone()))
                    .expect("import request"),
            )
            .await
            .expect("missing csrf import");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/providers/import")
                    .header(header::COOKIE, &cookie)
                    .header(web_auth::CSRF_HEADER, csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(provider_export_body))
                    .expect("import request"),
            )
            .await
            .expect("import");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("ok"), Some(&Value::Bool(true)));
        assert_eq!(body.get("providerCount").and_then(Value::as_u64), Some(1));
    }

    #[tokio::test]
    async fn providers_sub2api_candidate_and_selected_export_endpoints_are_authenticated() {
        let (_dir, app, db) = test_router_with_db(|_| {});
        let first_provider = Provider::with_id(
            "first-provider".to_string(),
            "First Provider".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://first.example/v1", "ANTHROPIC_AUTH_TOKEN": "first-key" } }),
            None,
        );
        let second_provider = Provider::with_id(
            "second-provider".to_string(),
            "Second Provider".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://second.example/v1", "ANTHROPIC_AUTH_TOKEN": "second-key" } }),
            None,
        );
        let empty_key_provider = Provider::with_id(
            "empty-key".to_string(),
            "Empty Key".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://empty.example/v1", "ANTHROPIC_AUTH_TOKEN": "" } }),
            None,
        );
        db.save_provider("claude", &first_provider)
            .expect("seed first provider");
        db.save_provider("claude", &second_provider)
            .expect("seed second provider");
        db.save_provider("claude", &empty_key_provider)
            .expect("seed empty provider");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export/sub2api/candidates")
                    .body(Body::empty())
                    .expect("unauth candidates request"),
            )
            .await
            .expect("unauth candidates");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/admin/providers/export/sub2api",
                json!({ "selectedProviders": [{ "appType": "claude", "providerId": "first-provider" }] }),
            ))
            .await
            .expect("unauth selected export");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let cookie = session_cookie(&headers);
        let csrf = body
            .get("csrfToken")
            .and_then(Value::as_str)
            .expect("csrf")
            .to_string();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/admin/providers/export/sub2api/candidates")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("candidates request"),
            )
            .await
            .expect("candidates");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::OK);
        let candidates = body
            .get("candidates")
            .and_then(Value::as_array)
            .expect("candidates array");
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates
                .iter()
                .filter_map(|candidate| candidate.get("providerId").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            vec!["first-provider", "second-provider"]
        );
        assert_eq!(
            candidates[0].get("appType").and_then(Value::as_str),
            Some("claude")
        );
        assert_eq!(
            candidates[0].get("baseUrl").and_then(Value::as_str),
            Some("https://first.example")
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.get("apiKey").is_none()
                && candidate.get("api_key").is_none()
                && candidate.to_string().contains("key") == false
        }));

        let response = app
            .clone()
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/providers/export/sub2api",
                json!({ "selectedProviders": [] }),
                Some(&cookie),
                Some(&csrf),
            ))
            .await
            .expect("empty selection selected export");
        let (status, _headers, body) = json_response(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.contains("empty selection")));

        let response = app
            .oneshot(authed_json_request(
                Method::POST,
                "/api/admin/providers/export/sub2api",
                json!({
                    "selectedProviders": [
                        { "appType": "claude", "providerId": "second-provider" }
                    ]
                }),
                Some(&cookie),
                Some(&csrf),
            ))
            .await
            .expect("selected export");
        let (status, headers, body) = raw_response(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
        assert!(headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .expect("content disposition")
            .starts_with("attachment; filename=\"sub2api-account-"));
        let sub2api: Value = serde_json::from_slice(&body).expect("selected sub2api json");
        let accounts = sub2api
            .get("accounts")
            .and_then(Value::as_array)
            .expect("accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].get("name").and_then(Value::as_str),
            Some("Second Provider")
        );
        assert_eq!(
            accounts[0]
                .pointer("/credentials/api_key")
                .and_then(Value::as_str),
            Some("second-key")
        );
        assert_eq!(
            accounts[0]
                .pointer("/credentials/base_url")
                .and_then(Value::as_str),
            Some("https://second.example")
        );
    }

    #[tokio::test]
    async fn public_settings_expose_turnstile_without_secret() {
        let (_dir, app) = test_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/public/settings")
                    .body(Body::empty())
                    .expect("settings request"),
            )
            .await
            .expect("settings");
        let (status, _headers, body) = json_response(response).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("turnstile_enabled"), Some(&Value::Bool(false)));
        assert_eq!(body.get("turnstile_required"), Some(&Value::Bool(false)));
        assert_eq!(
            body.get("turnstile_site_key").and_then(Value::as_str),
            Some("0x4AAAAAADPfCPB_O-N3j6ON")
        );
        assert_eq!(
            body.get("turnstile_action").and_then(Value::as_str),
            Some("login")
        );
        assert_eq!(body.get("admin_configured"), Some(&Value::Bool(true)));
        assert!(body.get("turnstile_secret_key").is_none());
    }

    #[tokio::test]
    async fn login_cookie_uses_365_day_ttl() {
        let (_dir, app) = test_router();

        let response = app
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, headers, _body) = json_response(response).await;

        assert_eq!(status, StatusCode::OK);
        let cookie = headers
            .get(header::SET_COOKIE)
            .expect("set-cookie")
            .to_str()
            .expect("cookie str");
        assert!(cookie.contains("Max-Age=31536000"), "cookie={cookie}");
    }

    #[test]
    fn login_client_key_prefers_trusted_real_ip_over_spoofable_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.10"));
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.99, 203.0.113.10"),
        );

        assert_eq!(login_client_key(&headers), "203.0.113.10");
    }

    #[test]
    fn login_client_key_uses_forwarded_for_rightmost_when_real_ip_absent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.99, 203.0.113.10"),
        );

        assert_eq!(login_client_key(&headers), "203.0.113.10");
    }

    #[tokio::test]
    async fn login_fails_closed_when_turnstile_required_without_enabled() {
        let (_dir, app) = test_router_with_config(|config| {
            config.security.turnstile_enabled = false;
            config.security.turnstile_required = true;
        });

        let response = app
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, _headers, body) = json_response(response).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("turnstile is required")
        );
    }

    #[tokio::test]
    async fn login_requires_token_when_turnstile_enabled() {
        let (_dir, app) = test_router_with_config(|config| {
            config.security.turnstile_enabled = true;
            config.security.turnstile_secret_key = Some("secret-one".to_string());
        });

        let response = app
            .oneshot(json_request(
                Method::POST,
                "/api/auth/login",
                json!({ "username": "admin", "password": "very-secure-password" }),
            ))
            .await
            .expect("login");
        let (status, _headers, body) = json_response(response).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body
            .get("error")
            .and_then(Value::as_str)
            .expect("error")
            .contains("turnstile token is empty"));
    }
}
