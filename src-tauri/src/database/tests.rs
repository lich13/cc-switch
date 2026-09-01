//! 数据库模块测试
//!
//! 包含 Schema 迁移和基本功能的测试。

use super::*;
use crate::app_config::MultiAppConfig;
use crate::provider::{Provider, ProviderManager, ProviderMeta, UniversalProvider};
use indexmap::IndexMap;
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashMap;
use tempfile::NamedTempFile;

const LEGACY_SCHEMA_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

// v3.8.x（schema v1）的真实表结构快照：用于验证从 v3.8.* 升级到当前版本的迁移链路
// 参考：tag v3.8.3 的 src-tauri/src/database/schema.rs
pub(super) const V3_8_SCHEMA_V1_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        website_url TEXT,
        category TEXT,
        created_at INTEGER,
        sort_index INTEGER,
        notes TEXT,
        icon TEXT,
        icon_color TEXT,
        meta TEXT NOT NULL DEFAULT '{}',
        is_current BOOLEAN NOT NULL DEFAULT 0,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL,
        added_at INTEGER,
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL,
        description TEXT,
        homepage TEXT,
        docs TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        enabled_claude BOOLEAN NOT NULL DEFAULT 0,
        enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        description TEXT,
        enabled BOOLEAN NOT NULL DEFAULT 1,
        created_at INTEGER,
        updated_at INTEGER,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0,
        installed_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        branch TEXT NOT NULL DEFAULT 'main',
        enabled BOOLEAN NOT NULL DEFAULT 1,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

#[derive(Debug)]
struct ColumnInfo {
    r#type: String,
    notnull: i64,
    default: Option<String>,
}

fn get_column_info(conn: &Connection, table: &str, column: &str) -> ColumnInfo {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\");"))
        .expect("prepare pragma");
    let mut rows = stmt.query([]).expect("query pragma");
    while let Some(row) = rows.next().expect("read row") {
        let column_name: String = row.get(1).expect("name");
        if column_name.eq_ignore_ascii_case(column) {
            return ColumnInfo {
                r#type: row.get::<_, String>(2).expect("type"),
                notnull: row.get::<_, i64>(3).expect("notnull"),
                default: row.get::<_, Option<String>>(4).ok().flatten(),
            };
        }
    }
    panic!("column {table}.{column} not found");
}

fn normalize_default(default: &Option<String>) -> Option<String> {
    default
        .as_ref()
        .map(|s| s.trim_matches('\'').trim_matches('"').to_string())
}

fn make_provider(id: &str, name: &str, base_url: &str) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": format!("{id}-token")
            }
        }),
        Some(format!("https://{id}.example")),
    );
    provider.category = Some("custom".to_string());
    provider.created_at = Some(1_725_000_000_000);
    provider.sort_index = Some(7);
    provider.notes = Some(format!("{name} notes"));
    provider.icon = Some("box".to_string());
    provider.icon_color = Some("#336699".to_string());
    provider
}

fn make_xai_oauth_provider(id: &str) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        "xAI OAuth Managed".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "xai-oauth-managed-secret" },
            "config": "model_provider = \"xai\"\n[model_providers.xai]\nbase_url = \"https://api.x.ai/v1\"\n"
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("xai_oauth".to_string()),
        ..Default::default()
    });
    provider
}

fn make_universal_provider(id: &str, name: &str) -> UniversalProvider {
    let mut provider = UniversalProvider::new(
        id.to_string(),
        name.to_string(),
        "newapi".to_string(),
        format!("https://{id}.universal.example/v1"),
        format!("{id}-universal-key"),
    );
    provider.apps.claude = true;
    provider.apps.codex = true;
    provider.notes = Some("universal notes".to_string());
    provider.sort_index = Some(3);
    provider
}

fn scalar_count(db: &Database, sql: &str) -> i64 {
    let conn = db.conn.lock().expect("lock db");
    conn.query_row(sql, [], |row| row.get(0)).expect("count")
}

fn provider_name(db: &Database, app_type: &str, id: &str) -> Option<String> {
    let conn = db.conn.lock().expect("lock db");
    conn.query_row(
        "SELECT name FROM providers WHERE app_type = ?1 AND id = ?2",
        params![app_type, id],
        |row| row.get(0),
    )
    .ok()
}

fn seed_non_provider_rows(db: &Database) {
    let conn = db.conn.lock().expect("lock db");
    conn.execute(
        "INSERT OR REPLACE INTO mcp_servers
         (id, name, server_config, tags, enabled_claude, enabled_codex, enabled_gemini)
         VALUES ('mcp-keep', 'Keep MCP', '{}', '[]', 1, 0, 0)",
        [],
    )
    .expect("seed mcp");
    conn.execute(
        "INSERT OR REPLACE INTO prompts
         (id, app_type, name, content, enabled)
         VALUES ('prompt-keep', 'claude', 'Keep Prompt', 'content', 1)",
        [],
    )
    .expect("seed prompt");
    conn.execute(
        "INSERT OR REPLACE INTO skills
         (id, name, directory, enabled_claude, installed_at, updated_at)
         VALUES ('skill-keep', 'Keep Skill', '/tmp/skill', 1, 1, 1)",
        [],
    )
    .expect("seed skill");
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('global_proxy_url', 'http://127.0.0.1:7890')",
        [],
    )
    .expect("seed setting");
    conn.execute(
        "INSERT OR REPLACE INTO proxy_live_backup (app_type, original_config, backed_up_at)
         VALUES ('claude', '{}', '2026-06-15T00:00:00Z')",
        [],
    )
    .expect("seed live backup");
    conn.execute(
        "INSERT OR REPLACE INTO proxy_request_logs
         (request_id, provider_id, app_type, model, latency_ms, status_code, created_at)
         VALUES ('req-keep', 'old-provider', 'claude', 'model', 11, 200, 1725000000000)",
        [],
    )
    .expect("seed usage log");
}

#[test]
fn providers_json_export_import_replaces_only_providers_and_universal_providers() {
    let source = Database::memory().expect("source db");
    let mut source_provider = make_provider(
        "source-provider",
        "Source Provider",
        "https://source.example",
    );
    source_provider.in_failover_queue = true;
    source
        .save_provider("claude", &source_provider)
        .expect("save source provider");
    source
        .set_current_provider("claude", "source-provider")
        .expect("set current");
    source
        .add_custom_endpoint("claude", "source-provider", "https://source-alt.example")
        .expect("source endpoint");
    source
        .save_universal_provider(&make_universal_provider(
            "universal-source",
            "Universal Source",
        ))
        .expect("source universal");

    let exported = source
        .export_providers_json_string()
        .expect("export providers json");
    let envelope: serde_json::Value = serde_json::from_str(&exported).expect("json envelope");
    assert_eq!(
        envelope.get("format").and_then(serde_json::Value::as_str),
        Some("cc-switch-providers-export")
    );
    assert_eq!(
        envelope.get("version").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    assert!(envelope
        .get("exportedAt")
        .and_then(serde_json::Value::as_str)
        .is_some());
    assert!(envelope
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .is_some());
    assert!(envelope
        .get("providerEndpoints")
        .and_then(serde_json::Value::as_array)
        .is_some());
    assert!(envelope
        .get("universalProviders")
        .and_then(serde_json::Value::as_object)
        .is_some());

    let target = Database::memory().expect("target db");
    target
        .save_provider(
            "claude",
            &make_provider("old-provider", "Old Provider", "https://old.example"),
        )
        .expect("save old provider");
    target
        .save_provider(
            "codex",
            &make_provider("codex-old", "Old Codex", "https://codex-old.example"),
        )
        .expect("save old codex");
    target
        .save_universal_provider(&make_universal_provider("universal-old", "Universal Old"))
        .expect("old universal");
    target
        .set_setting("global_proxy_url", "http://127.0.0.1:9999")
        .expect("proxy setting");
    seed_non_provider_rows(&target);
    {
        let conn = target.conn.lock().expect("lock db");
        conn.execute(
            "INSERT OR REPLACE INTO provider_health
             (provider_id, app_type, is_healthy, consecutive_failures, updated_at)
             VALUES ('old-provider', 'claude', 0, 4, '2026-06-15T00:00:00Z')",
            [],
        )
        .expect("seed health");
    }

    let summary = target
        .import_providers_json_string(&exported)
        .expect("import providers");

    assert_eq!(summary.provider_count, 1);
    assert_eq!(summary.provider_endpoint_count, 1);
    assert_eq!(summary.universal_provider_count, 1);
    assert_eq!(
        provider_name(&target, "claude", "source-provider").as_deref(),
        Some("Source Provider")
    );
    assert_eq!(provider_name(&target, "claude", "old-provider"), None);
    assert_eq!(provider_name(&target, "codex", "codex-old"), None);
    assert_eq!(
        target
            .get_current_provider("claude")
            .expect("current provider")
            .as_deref(),
        Some("source-provider")
    );
    assert_eq!(
        scalar_count(&target, "SELECT COUNT(*) FROM provider_endpoints"),
        1
    );
    assert_eq!(
        scalar_count(&target, "SELECT COUNT(*) FROM provider_health"),
        0
    );
    assert!(target
        .get_universal_provider("universal-source")
        .expect("read universal")
        .is_some());
    assert!(target
        .get_universal_provider("universal-old")
        .expect("read old universal")
        .is_none());

    assert_eq!(
        scalar_count(
            &target,
            "SELECT COUNT(*) FROM mcp_servers WHERE id = 'mcp-keep'"
        ),
        1
    );
    assert_eq!(
        scalar_count(
            &target,
            "SELECT COUNT(*) FROM prompts WHERE id = 'prompt-keep'"
        ),
        1
    );
    assert_eq!(
        scalar_count(
            &target,
            "SELECT COUNT(*) FROM skills WHERE id = 'skill-keep'"
        ),
        1
    );
    assert_eq!(
        target
            .get_setting("global_proxy_url")
            .expect("proxy setting")
            .as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        scalar_count(&target, "SELECT COUNT(*) FROM proxy_live_backup"),
        1
    );
    assert_eq!(
        scalar_count(&target, "SELECT COUNT(*) FROM proxy_request_logs"),
        1
    );
}

#[test]
fn providers_sub2api_export_uses_sample_shape_and_strips_v1_suffix() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("hanhe", "hh2", "https://api.hanhegufei.online/v1/"),
    )
    .expect("save claude provider");
    db.save_provider(
        "codex",
        &Provider::with_id(
            "codex-provider".to_string(),
            "codex one".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-codex" },
                "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://codex.example.com/v1\"\n"
            }),
            None,
        ),
    )
    .expect("save codex provider");
    db.save_provider(
        "claude",
        &Provider::with_id(
            "empty-key".to_string(),
            "empty key".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://empty.example.com/v1",
                    "ANTHROPIC_AUTH_TOKEN": ""
                }
            }),
            None,
        ),
    )
    .expect("save empty provider");

    let exported = db
        .export_providers_sub2api_json_string()
        .expect("export sub2api");
    let envelope: serde_json::Value = serde_json::from_str(&exported).expect("sub2api json");

    assert!(envelope
        .get("exported_at")
        .and_then(serde_json::Value::as_str)
        .is_some());
    assert_eq!(
        envelope
            .get("proxies")
            .and_then(serde_json::Value::as_array)
            .expect("proxies")
            .len(),
        0
    );

    let accounts = envelope
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .expect("accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts.iter().all(
        |account| account.get("name").and_then(serde_json::Value::as_str) != Some("empty key")
    ));

    let account = accounts
        .iter()
        .find(|account| account.get("name").and_then(serde_json::Value::as_str) == Some("hh2"))
        .expect("hh2 account");
    assert_eq!(
        account.get("platform").and_then(serde_json::Value::as_str),
        Some("openai")
    );
    assert_eq!(
        account.get("type").and_then(serde_json::Value::as_str),
        Some("apikey")
    );
    assert_eq!(
        account
            .pointer("/credentials/api_key")
            .and_then(serde_json::Value::as_str),
        Some("hanhe-token")
    );
    assert_eq!(
        account
            .pointer("/credentials/base_url")
            .and_then(serde_json::Value::as_str),
        Some("https://api.hanhegufei.online")
    );
    assert_eq!(
        account
            .pointer("/credentials/pool_mode")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        account
            .pointer("/credentials/pool_mode_retry_count")
            .and_then(serde_json::Value::as_i64),
        Some(3)
    );
    assert_eq!(
        account
            .pointer("/extra/openai_apikey_responses_websockets_v2_enabled")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        account
            .pointer("/extra/openai_apikey_responses_websockets_v2_mode")
            .and_then(serde_json::Value::as_str),
        Some("off")
    );
    assert_eq!(
        account
            .pointer("/extra/openai_passthrough")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        account
            .pointer("/extra/openai_responses_supported")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        account
            .get("concurrency")
            .and_then(serde_json::Value::as_i64),
        Some(10)
    );
    assert_eq!(
        account.get("priority").and_then(serde_json::Value::as_i64),
        Some(2)
    );
    assert_eq!(
        account
            .get("rate_multiplier")
            .and_then(serde_json::Value::as_i64),
        Some(1)
    );
    assert_eq!(
        account
            .get("auto_pause_on_expired")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let codex = accounts
        .iter()
        .find(|account| {
            account.get("name").and_then(serde_json::Value::as_str) == Some("codex one")
        })
        .expect("codex account");
    assert_eq!(
        codex
            .pointer("/credentials/api_key")
            .and_then(serde_json::Value::as_str),
        Some("sk-codex")
    );
    assert_eq!(
        codex
            .pointer("/credentials/base_url")
            .and_then(serde_json::Value::as_str),
        Some("https://codex.example.com")
    );
}

#[test]
fn providers_sub2api_selected_export_exports_only_selected_accounts_in_stable_order() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("first", "First", "https://first.example/v1"),
    )
    .expect("save first provider");
    db.save_provider(
        "claude",
        &make_provider("second", "Second", "https://second.example/v1"),
    )
    .expect("save second provider");
    db.save_provider(
        "codex",
        &Provider::with_id(
            "codex-provider".to_string(),
            "Codex Provider".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-codex" },
                "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://codex.example.com/v1\"\n"
            }),
            None,
        ),
    )
    .expect("save codex provider");

    let exported = db
        .export_providers_sub2api_json_string_for_selection(&[
            Sub2apiProviderSelection::new("codex", "codex-provider"),
            Sub2apiProviderSelection::new("claude", "first"),
        ])
        .expect("export selected sub2api");
    let envelope: serde_json::Value = serde_json::from_str(&exported).expect("sub2api json");
    let accounts = envelope
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .expect("accounts");

    assert_eq!(accounts.len(), 2);
    assert_eq!(
        accounts
            .iter()
            .filter_map(|account| account.get("name").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>(),
        vec!["First", "Codex Provider"]
    );
    assert!(accounts.iter().all(|account| {
        account.get("name").and_then(serde_json::Value::as_str) != Some("Second")
    }));
}

#[test]
fn providers_sub2api_selected_export_rejects_empty_unknown_and_non_exportable_selection() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("exportable", "Exportable", "https://exportable.example/v1"),
    )
    .expect("save exportable provider");
    db.save_provider(
        "claude",
        &Provider::with_id(
            "empty-key".to_string(),
            "Empty Key".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://empty.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": ""
                }
            }),
            None,
        ),
    )
    .expect("save non-exportable provider");
    db.save_provider("codex", &make_xai_oauth_provider("xai-oauth"))
        .expect("save xAI OAuth provider");

    let empty_err = db
        .export_providers_sub2api_json_string_for_selection(&[])
        .expect_err("empty selection should fail");
    assert!(
        empty_err.to_string().contains("empty selection"),
        "unexpected empty selection error: {empty_err}"
    );

    let missing_err = db
        .export_providers_sub2api_json_string_for_selection(&[Sub2apiProviderSelection::new(
            "claude", "missing",
        )])
        .expect_err("missing provider should fail");
    assert!(
        missing_err.to_string().contains("not found")
            || missing_err.to_string().contains("not exportable"),
        "unexpected missing provider error: {missing_err}"
    );

    let non_exportable_err = db
        .export_providers_sub2api_json_string_for_selection(&[Sub2apiProviderSelection::new(
            "claude",
            "empty-key",
        )])
        .expect_err("non-exportable provider should fail");
    assert!(
        non_exportable_err.to_string().contains("not exportable"),
        "unexpected non-exportable provider error: {non_exportable_err}"
    );

    let xai_oauth_err = db
        .export_providers_sub2api_json_string_for_selection(&[Sub2apiProviderSelection::new(
            "codex",
            "xai-oauth",
        )])
        .expect_err("xAI OAuth provider should not be exportable");
    assert!(
        xai_oauth_err.to_string().contains("not exportable"),
        "unexpected xAI OAuth provider error: {xai_oauth_err}"
    );
}

#[test]
fn providers_sub2api_candidates_expose_metadata_without_secrets() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("exportable", "Exportable", "https://exportable.example/v1/"),
    )
    .expect("save exportable provider");
    db.save_provider(
        "claude",
        &Provider::with_id(
            "empty-key".to_string(),
            "Empty Key".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://empty.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": ""
                }
            }),
            None,
        ),
    )
    .expect("save non-exportable provider");
    db.save_provider("codex", &make_xai_oauth_provider("xai-oauth"))
        .expect("save xAI OAuth provider");

    let candidates = db
        .list_sub2api_export_candidates()
        .expect("list candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].app_type, "claude");
    assert_eq!(candidates[0].provider_id, "exportable");
    assert_eq!(candidates[0].name, "Exportable");
    assert_eq!(candidates[0].base_url, "https://exportable.example");
    assert!(candidates
        .iter()
        .all(|candidate| candidate.provider_id != "xai-oauth"));

    let serialized = serde_json::to_value(&candidates[0]).expect("candidate json");
    assert!(serialized.get("appType").is_some());
    assert!(serialized.get("providerId").is_some());
    assert!(serialized.get("apiKey").is_none());
    assert!(serialized.get("api_key").is_none());
    assert!(!serialized.to_string().contains("exportable-token"));
}

#[test]
fn providers_json_import_rejects_bad_format() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("keep", "Keep", "https://keep.example"),
    )
    .expect("seed provider");

    let err = db
        .import_providers_json_string(r#"{"format":"not-cc-switch","version":1}"#)
        .expect_err("bad envelope should fail");

    assert!(
        err.to_string().contains("供应商导入文件格式无效")
            || err.to_string().contains("invalid provider export"),
        "unexpected error: {err}"
    );
    assert_eq!(
        provider_name(&db, "claude", "keep").as_deref(),
        Some("Keep")
    );
}

#[test]
fn providers_json_import_rolls_back_on_transaction_failure() {
    let db = Database::memory().expect("db");
    db.save_provider(
        "claude",
        &make_provider("keep", "Keep", "https://keep.example"),
    )
    .expect("seed provider");
    db.save_universal_provider(&make_universal_provider("universal-keep", "Universal Keep"))
        .expect("seed universal");
    {
        let conn = db.conn.lock().expect("lock db");
        conn.execute(
            "INSERT OR REPLACE INTO provider_health
             (provider_id, app_type, is_healthy, consecutive_failures, updated_at)
             VALUES ('keep', 'claude', 0, 3, '2026-06-15T00:00:00Z')",
            [],
        )
        .expect("seed health");
    }

    let mut bad_envelope: serde_json::Value =
        serde_json::from_str(&db.export_providers_json_string().expect("export")).expect("json");
    bad_envelope["providerEndpoints"] = json!([
        {
            "providerId": "keep",
            "appType": "claude",
            "url": "https://dup.example",
            "addedAt": 1
        },
        {
            "providerId": "keep",
            "appType": "claude",
            "url": "https://dup.example",
            "addedAt": 2
        }
    ]);

    let err = db
        .import_providers_json_string(&bad_envelope.to_string())
        .expect_err("duplicate endpoints should fail");

    assert!(
        err.to_string().contains("UNIQUE")
            || err.to_string().contains("provider_endpoints")
            || err.to_string().contains("duplicate provider endpoint")
            || err.to_string().contains("导入供应商"),
        "unexpected error: {err}"
    );
    assert_eq!(
        provider_name(&db, "claude", "keep").as_deref(),
        Some("Keep")
    );
    assert!(db
        .get_universal_provider("universal-keep")
        .expect("universal")
        .is_some());
    assert_eq!(scalar_count(&db, "SELECT COUNT(*) FROM provider_health"), 1);
}

#[test]
fn providers_json_file_import_creates_database_backup_before_replace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("cc-switch.db");
    let db = Database::init_at(&db_path).expect("db");
    db.save_provider(
        "claude",
        &make_provider("file-provider", "File Provider", "https://file.example"),
    )
    .expect("seed provider");
    let exported = db.export_providers_json_string().expect("export");

    db.import_providers_json_string(&exported)
        .expect("import should backup");

    let backups_dir = temp.path().join("backups");
    let backup_count = std::fs::read_dir(&backups_dir)
        .expect("backups dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("db"))
        .count();
    assert!(
        backup_count >= 1,
        "expected at least one backup in {}",
        backups_dir.display()
    );
}

#[test]
fn deleted_default_skill_repo_is_not_restored() {
    let db = Database::memory().expect("create memory db");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 4);
    for repo in db.get_skill_repos().expect("get initialized repos") {
        db.delete_skill_repo(&repo.owner, &repo.name)
            .expect("delete repo");
    }
    assert!(db.get_skill_repos().expect("get deleted repos").is_empty());

    assert_eq!(
        db.init_default_skill_repos().expect("reinitialize repos"),
        0
    );
    assert!(db.get_skill_repos().expect("get repos").is_empty());
}

#[test]
fn existing_skill_repo_selection_is_not_supplemented() {
    let db = Database::memory().expect("create memory db");
    let default_store = crate::services::skill::SkillStore::default();
    db.save_skill_repo(&default_store.repos[0])
        .expect("save existing repo");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 0);
    assert_eq!(db.get_skill_repos().expect("get repos").len(), 1);
    assert!(db
        .get_bool_flag("default_skill_repos_initialized")
        .expect("get initialized flag"));
}

#[test]
fn schema_migration_sets_user_version_when_missing() {
    let conn = Connection::open_in_memory().expect("open memory db");

    Database::create_tables_on_conn(&conn).expect("create tables");
    assert_eq!(
        Database::get_user_version(&conn).expect("read version before"),
        0
    );

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migration");

    assert_eq!(
        Database::get_user_version(&conn).expect("read version after"),
        SCHEMA_VERSION
    );
}

#[test]
fn webd_database_init_preserves_old_proxy_request_details() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("cc-switch.db");
    let db = Database::init_at(&db_path).expect("initialize database");
    let old_created_at = chrono::Utc::now().timestamp() - 45 * 24 * 60 * 60;

    {
        let conn = db.conn.lock().expect("lock database");
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, latency_ms,
                status_code, created_at, data_source
             ) VALUES ('old-proxy-detail', 'provider', 'codex', 'gpt-test', 1, 200, ?1, 'proxy')",
            [old_created_at],
        )
        .expect("insert old proxy detail");
    }
    drop(db);

    let reopened = Database::init_at_for_webd(&db_path).expect("reopen webd database");
    assert_eq!(
        scalar_count(
            &reopened,
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'old-proxy-detail'",
        ),
        1,
        "webd startup must not prune proxy request details",
    );
    drop(reopened);

    let desktop_reopened = Database::init_at(&db_path).expect("reopen desktop database");
    assert_eq!(
        scalar_count(
            &desktop_reopened,
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'old-proxy-detail'",
        ),
        0,
        "desktop startup must retain the official usage rollup policy",
    );
}

#[test]
fn schema_migration_rejects_future_version() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::set_user_version(&conn, SCHEMA_VERSION + 1).expect("set future version");

    let err =
        Database::apply_schema_migrations_on_conn(&conn).expect_err("should reject higher version");
    assert!(
        err.to_string().contains("数据库版本过新"),
        "unexpected error: {err}"
    );
}

#[test]
fn schema_migration_adds_missing_columns_for_providers() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 创建旧版 providers 表，缺少新增列
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 验证关键新增列已补齐
    for (table, column) in [
        ("providers", "meta"),
        ("providers", "is_current"),
        ("provider_endpoints", "added_at"),
        ("mcp_servers", "enabled_gemini"),
        ("prompts", "updated_at"),
        ("skills", "installed_at"),
        ("skill_repos", "enabled"),
    ] {
        assert!(
            Database::has_column(&conn, table, column).expect("check column"),
            "{table}.{column} should exist after migration"
        );
    }

    // 验证 meta 列约束保持一致
    let meta = get_column_info(&conn, "providers", "meta");
    assert_eq!(meta.notnull, 1, "meta should be NOT NULL");
    assert_eq!(
        normalize_default(&meta.default).as_deref(),
        Some("{}"),
        "meta default should be '{{}}'"
    );

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_aligns_column_defaults_and_types() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let is_current = get_column_info(&conn, "providers", "is_current");
    assert_eq!(is_current.r#type, "BOOLEAN");
    assert_eq!(is_current.notnull, 1);
    assert_eq!(normalize_default(&is_current.default).as_deref(), Some("0"));

    let tags = get_column_info(&conn, "mcp_servers", "tags");
    assert_eq!(tags.r#type, "TEXT");
    assert_eq!(tags.notnull, 1);
    assert_eq!(normalize_default(&tags.default).as_deref(), Some("[]"));

    let enabled = get_column_info(&conn, "prompts", "enabled");
    assert_eq!(enabled.r#type, "BOOLEAN");
    assert_eq!(enabled.notnull, 1);
    assert_eq!(normalize_default(&enabled.default).as_deref(), Some("1"));

    let installed_at = get_column_info(&conn, "skills", "installed_at");
    assert_eq!(installed_at.r#type, "INTEGER");
    assert_eq!(installed_at.notnull, 1);
    assert_eq!(
        normalize_default(&installed_at.default).as_deref(),
        Some("0")
    );

    let branch = get_column_info(&conn, "skill_repos", "branch");
    assert_eq!(branch.r#type, "TEXT");
    assert_eq!(normalize_default(&branch.default).as_deref(), Some("main"));

    let skill_repo_enabled = get_column_info(&conn, "skill_repos", "enabled");
    assert_eq!(skill_repo_enabled.r#type, "BOOLEAN");
    assert_eq!(skill_repo_enabled.notnull, 1);
    assert_eq!(
        normalize_default(&skill_repo_enabled.default).as_deref(),
        Some("1")
    );
}

#[test]
fn schema_create_tables_include_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);
}

#[test]
fn schema_migration_v4_adds_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE providers (
            id TEXT NOT NULL,
            app_type TEXT NOT NULL,
            name TEXT NOT NULL,
            settings_config TEXT NOT NULL DEFAULT '{}',
            meta TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY (id, app_type)
        );
        CREATE TABLE proxy_config (app_type TEXT PRIMARY KEY);
        CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY, model TEXT NOT NULL);
        CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            server_config TEXT NOT NULL,
            enabled_claude INTEGER NOT NULL DEFAULT 0,
            enabled_codex INTEGER NOT NULL DEFAULT 0,
            enabled_gemini INTEGER NOT NULL DEFAULT 0,
            enabled_opencode INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .expect("seed v4 schema");

    Database::set_user_version(&conn, 4).expect("set user_version=4");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn migration_v10_to_v11_rebuilds_rollups_with_request_model_dimension() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟 v10 形状的 rollup 表（主键不含 request_model）+ 一行历史聚合数据，
    // 以及 v10 形状的明细表（无 pricing_model 列）
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_request_logs (
            request_id TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            request_model TEXT
        );
        CREATE TABLE usage_daily_rollups (
            date TEXT NOT NULL,
            app_type TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            model TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            success_count INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            total_cost_usd TEXT NOT NULL DEFAULT '0',
            avg_latency_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (date, app_type, provider_id, model)
        );
        INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_count, success_count,
             input_tokens, output_tokens, total_cost_usd, avg_latency_ms)
        VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 7, 7, 1000, 500, '0.07', 120);
        "#,
    )
    .expect("seed v10 rollup table");

    Database::set_user_version(&conn, 10).expect("set user_version=10");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 新列存在且 NOT NULL DEFAULT ''
    let request_model = get_column_info(&conn, "usage_daily_rollups", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 1);
    let rollup_pricing_model = get_column_info(&conn, "usage_daily_rollups", "pricing_model");
    assert_eq!(rollup_pricing_model.r#type, "TEXT");
    assert_eq!(rollup_pricing_model.notnull, 1);

    // 明细表补上 pricing_model 列（可空，历史行 NULL）
    let pricing_model = get_column_info(&conn, "proxy_request_logs", "pricing_model");
    assert_eq!(pricing_model.r#type, "TEXT");
    assert_eq!(pricing_model.notnull, 0);

    // 历史行保留，request_model 填 ''（未知）
    let (rm, count, input, cost): (String, i64, i64, String) = conn
        .query_row(
            "SELECT request_model, request_count, input_tokens, total_cost_usd
             FROM usage_daily_rollups WHERE model = 'kimi-k2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("migrated row");
    assert_eq!(rm, "");
    assert_eq!(count, 7);
    assert_eq!(input, 1000);
    assert_eq!(cost, "0.07");

    // 主键包含 request_model：同 model 不同别名可共存
    conn.execute(
        "INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_model, request_count)
         VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 'claude-sonnet-4-6', 1)",
        [],
    )
    .expect("insert row with same model but different request_model");

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_create_tables_repairs_dev_global_profile_marker() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟跑过未发布开发版的库：user_version 已是 12（迁移不会再跑），
    // 但 current 标记还是全局 key（现按应用分组）
    conn.execute_batch(
        r#"
        CREATE TABLE profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            payload TEXT NOT NULL,
            sort_order INTEGER,
            created_at INTEGER,
            updated_at INTEGER
        );
        INSERT INTO profiles (id, name, payload) VALUES ('p1', 'Project A', '{}');
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
        INSERT INTO settings (key, value) VALUES ('current_profile_id', 'p1');
        "#,
    )
    .expect("seed dev v12 shape");
    Database::set_user_version(&conn, 12).expect("set user_version=12");

    Database::create_tables_on_conn(&conn).expect("create tables should repair marker");

    // 全局 current 标记改名为 claude 组标记，旧 key 删除
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker");
    assert_eq!(claude_marker, "p1");
    let old_marker: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'current_profile_id'",
            [],
            |row| row.get(0),
        )
        .expect("count old marker");
    assert_eq!(old_marker, 0);

    // 修复必须幂等：再跑一遍不应破坏已迁移的标记
    Database::create_tables_on_conn(&conn).expect("repair is idempotent");
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker survives");
    assert_eq!(claude_marker, "p1");
}

#[test]
fn schema_create_tables_repairs_legacy_proxy_config_singleton_to_per_app() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟测试版 v2：user_version=2，但 proxy_config 仍是单例结构（无 app_type）
    Database::set_user_version(&conn, 2).expect("set user_version");
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_config (
            id INTEGER PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 5000,
            max_retries INTEGER NOT NULL DEFAULT 3,
            request_timeout INTEGER NOT NULL DEFAULT 300,
            enable_logging INTEGER NOT NULL DEFAULT 1,
            target_app TEXT NOT NULL DEFAULT 'claude',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO proxy_config (id, enabled) VALUES (1, 1);
        "#,
    )
    .expect("seed legacy proxy_config");

    Database::create_tables_on_conn(&conn).expect("create tables should repair proxy_config");

    assert!(
        Database::has_column(&conn, "proxy_config", "app_type").expect("check app_type"),
        "proxy_config should be migrated to per-app structure"
    );

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count rows");
    assert_eq!(count, 4, "per-app proxy_config should have 4 rows");

    // 新结构下应能按 app_type 查询
    let _: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("query by app_type");
}

#[test]
fn migration_from_v3_8_schema_v1_to_current_schema_v3() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute("PRAGMA foreign_keys = ON;", [])
        .expect("enable foreign keys");

    // 模拟 v3.8.* 用户的数据库（schema v1）
    conn.execute_batch(V3_8_SCHEMA_V1_SQL)
        .expect("seed v3.8 schema v1");
    Database::set_user_version(&conn, 1).expect("set user_version=1");

    // 插入一条旧版 Provider + Skill（用于验证迁移不会破坏既有数据）
    conn.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta, is_current
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            "p1",
            "claude",
            "Test Provider",
            serde_json::to_string(&json!({ "anthropicApiKey": "sk-test" })).unwrap(),
            Option::<String>::None,
            Option::<String>::None,
            Option::<i64>::None,
            Option::<usize>::None,
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            "{}",
            1,
        ],
    )
    .expect("seed provider");

    conn.execute(
        "INSERT INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
        params!["claude:demo-skill", 1, 1700000000i64],
    )
    .expect("seed legacy skill");

    // 按应用启动流程：先 create_tables（补齐新增表），再 apply_schema_migrations（按 user_version 迁移）
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    assert_eq!(
        Database::get_user_version(&conn).expect("user_version after migration"),
        SCHEMA_VERSION
    );

    // v1 -> v2：providers 新增字段必须补齐
    for column in [
        "cost_multiplier",
        "limit_daily_usd",
        "limit_monthly_usd",
        "provider_type",
        "in_failover_queue",
    ] {
        assert!(
            Database::has_column(&conn, "providers", column).expect("check column"),
            "providers.{column} should exist after migration"
        );
    }

    // 旧 provider 不应丢失，且新增字段应有默认值
    let provider_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("count providers");
    assert_eq!(provider_count, 1);

    let cost_multiplier: String = conn
        .query_row(
            "SELECT cost_multiplier FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("read cost_multiplier");
    assert_eq!(cost_multiplier, "1.0");

    // v2 -> v3：skills 表重建为统一结构，并设置 pending 标记（后续由启动时扫描文件系统重建数据）
    assert!(
        Database::has_column(&conn, "skills", "enabled_claude").expect("check skills v3 column"),
        "skills table should be migrated to v3 structure"
    );
    let skills_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
        .expect("count skills");
    assert_eq!(skills_count, 0, "skills table should be rebuilt empty");

    let pending: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_pending'",
            [],
            |r| r.get(0),
        )
        .ok();
    assert!(
        matches!(pending.as_deref(), Some("true") | Some("1")),
        "skills_ssot_migration_pending should be set after v2->v3 migration"
    );
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_snapshot'",
            [],
            |r| r.get(0),
        )
        .ok();
    let snapshot = snapshot.expect("skills migration snapshot should be recorded");
    let snapshot_rows: serde_json::Value =
        serde_json::from_str(&snapshot).expect("parse skills migration snapshot");
    assert!(
        snapshot_rows
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| {
                row.get("directory").and_then(|v| v.as_str()) == Some("demo-skill")
                    && row.get("app_type").and_then(|v| v.as_str()) == Some("claude")
            })),
        "skills migration snapshot should preserve legacy app mapping"
    );

    // v3.9+ 新增：proxy_config 三行 seed 必须存在（否则 UI 会查不到默认值）
    let proxy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count proxy_config rows");
    assert_eq!(proxy_rows, 4);

    // model_pricing 应具备默认数据（迁移时会 seed）
    let pricing_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |r| r.get(0))
        .expect("count model_pricing rows");
    assert!(pricing_rows > 0, "model_pricing should be seeded");
}

#[test]
fn schema_dry_run_does_not_write_to_disk() {
    // Create minimal valid config for migration
    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), ProviderManager::default());

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should succeed without any file I/O errors
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with valid config: {result:?}"
    );
}

#[test]
fn dry_run_validates_schema_compatibility() {
    // Create config with actual provider data
    let mut providers = IndexMap::new();
    providers.insert(
        "test-provider".to_string(),
        Provider {
            id: "test-provider".to_string(),
            name: "Test Provider".to_string(),
            settings_config: json!({
                "anthropicApiKey": "sk-test-123",
            }),
            website_url: None,
            category: None,
            created_at: Some(1234567890),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        },
    );

    let manager = ProviderManager {
        providers,
        current: "test-provider".to_string(),
    };

    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), manager);

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should validate the full migration path
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with provider data: {result:?}"
    );
}

#[test]
fn schema_model_pricing_is_seeded_on_init() {
    let db = Database::memory().expect("create memory db");

    let conn = db.conn.lock().expect("lock conn");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |row| row.get(0))
        .expect("count pricing");

    assert!(
        count > 0,
        "模型定价数据应该在初始化时自动填充，实际数量: {}",
        count
    );

    // 验证包含 Claude 模型
    let claude_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'claude-%'",
            [],
            |row| row.get(0),
        )
        .expect("check claude");
    assert!(
        claude_count > 0,
        "应该包含 Claude 模型定价，实际数量: {}",
        claude_count
    );

    // 验证包含 GPT 模型
    let gpt_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gpt-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gpt");
    assert!(
        gpt_count > 0,
        "应该包含 GPT 模型定价，实际数量: {}",
        gpt_count
    );

    // 验证包含 Gemini 模型
    let gemini_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gemini-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gemini");
    assert!(
        gemini_count > 0,
        "应该包含 Gemini 模型定价，实际数量: {}",
        gemini_count
    );
}

#[test]
fn model_pricing_seed_repairs_known_outdated_builtin_prices() {
    let db = Database::memory().expect("create memory db");

    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '1.68',
                 output_cost_per_million = '3.36',
                 cache_read_cost_per_million = '0.14',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'deepseek-v4-pro'",
            [],
        )
        .expect("restore old DeepSeek price");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '9',
                 output_cost_per_million = '9',
                 cache_read_cost_per_million = '9',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'glm-5.1'",
            [],
        )
        .expect("set custom GLM price");
    }

    db.ensure_model_pricing_seeded()
        .expect("ensure pricing seeded");

    let conn = db.conn.lock().expect("lock conn");
    let deepseek: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'deepseek-v4-pro'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query DeepSeek price");
    // 从远古价 1.68/3.36/0.14 出发要连跳两级才能到位：
    //   1.68/3.36/0.14 →(2026-07 条目)→ 0.435/0.87/0.003625
    //                  →(2026-08-16 峰谷调价条目)→ 1.32/3.96/0.044
    // 这同时锁住了 repair 条目的顺序：新条目必须排在旧条目之后，
    // 否则老库会停在中间价位，本断言即会失败。
    assert_eq!(
        deepseek,
        ("1.32".to_string(), "3.96".to_string(), "0.044".to_string())
    );

    let glm: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'glm-5.1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query GLM price");
    assert_eq!(glm, ("9".to_string(), "9".to_string(), "9".to_string()));
}

#[test]
fn ensure_incremental_auto_vacuum_rebuilds_existing_file_db() {
    let temp = NamedTempFile::new().expect("create temp db file");
    let path = temp.path().to_path_buf();

    let conn = Connection::open(&path).expect("open temp db");
    conn.execute("PRAGMA auto_vacuum = NONE;", [])
        .expect("set none auto_vacuum");
    Database::create_tables_on_conn(&conn).expect("create tables");

    assert_eq!(
        Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum before rebuild"),
        0,
        "existing file db should start with NONE auto_vacuum"
    );

    let rebuilt =
        Database::ensure_incremental_auto_vacuum_on_conn(&conn).expect("enable incremental mode");
    assert!(rebuilt, "existing db should require rebuild via VACUUM");
    drop(conn);

    let reopened = Connection::open(&path).expect("reopen temp db");
    assert_eq!(
        Database::get_auto_vacuum_mode(&reopened).expect("auto_vacuum after rebuild"),
        2,
        "file db should persist INCREMENTAL auto_vacuum after VACUUM rebuild"
    );
}

#[test]
fn user_agent_rewrite_config_defaults_and_rejects_invalid_regex() {
    let db = Database::memory().expect("create memory db");

    let default_config = db
        .get_user_agent_rewrite_config()
        .expect("default user agent rewrite config");
    assert!(default_config.enabled);
    assert!(default_config.matches("OpenAI/Python 2.24.0"));

    let invalid = crate::proxy::types::UserAgentRewriteConfig {
        enabled: true,
        rules: vec![crate::proxy::types::UserAgentRewriteRule {
            enabled: true,
            pattern: "(".to_string(),
        }],
        ..crate::proxy::types::UserAgentRewriteConfig::default()
    };

    let err = db
        .set_user_agent_rewrite_config(&invalid)
        .expect_err("invalid regex should be rejected");
    assert!(matches!(err, AppError::InvalidInput(_)));
}

#[test]
fn user_agent_rewrite_config_drops_legacy_claude_target_on_save() {
    let db = Database::memory().expect("create memory db");
    db.set_setting(
        "user_agent_rewrite_config",
        r#"{"enabled":true,"claudeTarget":"legacy-claude/1.0","codexTarget":"codex-custom/2.0","rules":[{"enabled":true,"pattern":"^Legacy/.*$"}]}"#,
    )
    .expect("seed legacy user-agent rewrite config");

    let config = db
        .get_user_agent_rewrite_config()
        .expect("legacy user agent rewrite config");

    assert_eq!(config.codex_target, "codex-custom/2.0");
    assert!(config.matches("Legacy/1.0"));

    db.set_user_agent_rewrite_config(&config)
        .expect("save normalized rewrite config");
    let saved = db
        .get_setting("user_agent_rewrite_config")
        .expect("read normalized setting")
        .expect("rewrite setting exists");
    assert!(!saved.contains("claudeTarget"));
    assert!(!saved.contains("legacy-claude/1.0"));
}
