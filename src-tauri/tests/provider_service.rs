use serde_json::json;

use cc_switch_lib::{
    get_claude_settings_path, read_json_file, AppError, AppState, AppType, McpApps, McpServer,
    MultiAppConfig, Provider, ProviderMeta, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{
    create_test_state, create_test_state_with_config, enable_codex_official_auth_preservation,
    ensure_test_home, reset_test_fs, test_mutex,
};

fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

fn seed_codex_live_files(auth: &serde_json::Value, config: &str) {
    let auth_path = cc_switch_lib::get_codex_auth_path();
    let config_path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).expect("create codex config dir");
    }
    std::fs::write(
        &auth_path,
        serde_json::to_string(auth).expect("serialize auth"),
    )
    .expect("seed auth.json");
    std::fs::write(&config_path, config).expect("seed config.toml");
}

fn codex_live_file_bytes() -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(cc_switch_lib::get_codex_auth_path()).expect("read auth.json bytes"),
        std::fs::read(cc_switch_lib::get_codex_config_path()).expect("read config.toml bytes"),
    )
}

fn assert_codex_live_files_unchanged(before: &(Vec<u8>, Vec<u8>)) {
    assert_eq!(
        std::fs::read(cc_switch_lib::get_codex_auth_path()).expect("read auth.json bytes"),
        before.0,
        "auth.json should remain byte-equivalent"
    );
    assert_eq!(
        std::fs::read(cc_switch_lib::get_codex_config_path()).expect("read config.toml bytes"),
        before.1,
        "config.toml should remain byte-equivalent"
    );
}

fn assert_codex_switch_route_only_updates_current(
    state: &AppState,
    target_provider: &str,
    _previous_current: Option<&str>,
    before: &(Vec<u8>, Vec<u8>),
) {
    ProviderService::switch(state, AppType::Codex, target_provider)
        .expect("Codex provider switch should update local route only");
    assert_codex_live_files_unchanged(before);

    let current_id = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("read current provider after route-only switch");
    assert_eq!(
        current_id.as_deref(),
        Some(target_provider),
        "Codex route-only switch should update the current provider"
    );
    assert!(
        futures::executor::block_on(state.db.get_live_backup(AppType::Codex.as_str()))
            .expect("read Codex live backup after route-only switch")
            .is_none(),
        "Codex route-only switch should not create or keep a live backup"
    );
}

#[test]
fn migrate_legacy_common_config_usage_marks_historical_provider_enabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "legacy-provider".to_string();
        manager.providers.insert(
            "legacy-provider".to_string(),
            Provider::with_id(
                "legacy-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "includeCoAuthoredBy": false,
                    "env": {
                        "ANTHROPIC_API_KEY": "legacy-key"
                    }
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{ "includeCoAuthoredBy": false }"#.to_string()),
        )
        .expect("set common config snippet");

    ProviderService::migrate_legacy_common_config_usage_if_needed(&state, AppType::Claude)
        .expect("migrate legacy common config");

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get providers after migration");
    let provider = providers
        .get("legacy-provider")
        .expect("legacy provider exists");

    assert_eq!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.common_config_enabled),
        Some(true),
        "historical provider should be explicitly marked as using common config"
    );
    assert!(
        provider
            .settings_config
            .get("includeCoAuthoredBy")
            .is_none(),
        "common config fields should be stripped from provider storage after migration"
    );
    assert_eq!(
        provider
            .settings_config
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str()),
        Some("legacy-key"),
        "provider-specific auth should remain untouched"
    );
}

#[test]
fn provider_service_switch_codex_updates_route_only_and_keeps_live_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let legacy_auth = json!({ "OPENAI_API_KEY": "legacy-key" });
    let legacy_config = r#"[mcp_servers.legacy]
type = "stdio"
command = "echo"
"#;
    seed_codex_live_files(&legacy_auth, legacy_config);

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": "stale-config"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Latest".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"[mcp_servers.latest]
type = "stdio"
command = "say"
"#
                }),
                None,
            ),
        );
    }

    // 使用新的统一 MCP 结构（v3.7.0+）
    let servers = initial_config
        .mcp
        .servers
        .get_or_insert_with(Default::default);
    servers.insert(
        "echo-server".into(),
        McpServer {
            id: "echo-server".into(),
            name: "Echo Server".into(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: true,
                gemini: false,
                grokbuild: false,
                opencode: false,
                hermes: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "new-provider",
        Some("old-provider"),
        &before,
    );

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers after route-only switch");

    let new_provider = providers.get("new-provider").expect("new provider exists");
    let new_config_text = new_provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        new_config_text.contains("mcp_servers.latest"),
        "route-only switch should leave provider config untouched"
    );

    let legacy = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    let legacy_auth_value = legacy
        .settings_config
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        legacy_auth_value, "stale",
        "route-only switch should not backfill the previous provider"
    );
}

#[test]
fn provider_service_switch_codex_preserves_user_model_provider_id_after_migration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let legacy_auth = json!({ "OPENAI_API_KEY": "rightcode-key" });
    let legacy_config = r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    seed_codex_live_files(&legacy_auth, legacy_config);

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "RightCode".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": legacy_config
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "AiHubMix".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "new-provider",
        Some("old-provider"),
        &before,
    );

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers after route-only switch");
    let new_config_text = providers
        .get("new-provider")
        .expect("new provider exists")
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        new_config_text.contains("[model_providers.aihubmix]"),
        "route-only switch should leave provider template unchanged"
    );
}

#[test]
fn provider_service_switch_codex_preserves_oauth_and_provider_auth_route_only() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "oauth-token",
            "account_id": "acct-1"
        }
    });
    let legacy_config = r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    seed_codex_live_files(&live_auth, legacy_config);

    let bridge_provider = Provider::with_id(
        "bridge-provider".to_string(),
        "Bridge Provider".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "bridge-key"},
            "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "legacy-provider".to_string();
        manager.providers.insert(
            "legacy-provider".to_string(),
            Provider::with_id(
                "legacy-provider".to_string(),
                "RightCode".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "rightcode-key"},
                    "config": legacy_config
                }),
                None,
            ),
        );
        manager
            .providers
            .insert("bridge-provider".to_string(), bridge_provider);
        manager.providers.insert(
            "plain-provider".to_string(),
            Provider::with_id(
                "plain-provider".to_string(),
                "Plain Provider".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "plain-key"},
                    "config": r#"model_provider = "plain"
model = "gpt-5.4"

[model_providers.plain]
name = "Plain"
base_url = "https://plain.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "bridge-provider",
        Some("legacy-provider"),
        &before,
    );

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers");
    let stored_bridge = providers
        .get("bridge-provider")
        .expect("bridge provider exists after route-only switch");
    assert_eq!(
        stored_bridge
            .settings_config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(|v| v.as_str()),
        Some("bridge-key"),
        "route-only switch should leave stored provider auth untouched"
    );
    assert!(
        stored_bridge
            .settings_config
            .pointer("/auth/tokens")
            .is_none(),
        "route-only switch should not persist ChatGPT OAuth tokens into provider storage"
    );
    assert!(
        !stored_bridge
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("experimental_bearer_token"),
        "stored provider config should stay clean; bridge token is generated only for live config"
    );
}

#[tokio::test(flavor = "current_thread")]
#[allow(
    clippy::await_holding_lock,
    reason = "this integration-style test must serialize global test HOME and settings mutations across async takeover calls"
)]
async fn codex_official_to_deepseek_then_takeover_enables_route_only_switch() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": "oauth-access",
            "id_token": "oauth-id"
        }
    });
    let official_config = r#"model_provider = "openai"
model = "gpt-5"

[model_providers.openai]
name = "OpenAI"
wire_api = "responses"
"#;
    seed_codex_live_files(&oauth_auth, official_config);

    let deepseek_provider_config = r#"model_provider = "deepseek"
model = "deepseek-chat"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#;
    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "official-provider".to_string();

        let mut official_provider = Provider::with_id(
            "official-provider".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": oauth_auth,
                "config": official_config
            }),
            None,
        );
        official_provider.category = Some("official".to_string());
        manager
            .providers
            .insert("official-provider".to_string(), official_provider);

        let mut deepseek_provider = Provider::with_id(
            "deepseek-provider".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "deepseek-key"},
                "config": deepseek_provider_config
            }),
            None,
        );
        deepseek_provider.category = Some("custom".to_string());
        manager
            .providers
            .insert("deepseek-provider".to_string(), deepseek_provider);
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let mut proxy_config = state.db.get_proxy_config().await.expect("get proxy config");
    proxy_config.listen_port = 0;
    state
        .db
        .update_proxy_config(proxy_config)
        .await
        .expect("set proxy to ephemeral port");

    let before = codex_live_file_bytes();

    ProviderService::switch(&state, AppType::Codex, "deepseek-provider")
        .expect("switch from official subscription to DeepSeek route");
    assert_codex_live_files_unchanged(&before);

    let auth_after_switch: serde_json::Value =
        read_json_file(&cc_switch_lib::get_codex_auth_path()).expect("read auth after switch");
    assert_eq!(
        auth_after_switch, oauth_auth,
        "normal provider switch with Codex preservation enabled must keep OAuth auth.json"
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current provider after route-only switch")
            .as_deref(),
        Some("deepseek-provider"),
        "normal Codex switch should update the local route without writing live files"
    );

    state
        .proxy_service
        .set_takeover_for_app("codex", true)
        .await
        .expect("Codex route-only takeover should enable without live writes");
    assert_codex_live_files_unchanged(&before);
    assert!(
        state
            .db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read Codex proxy config")
            .enabled,
        "route-only takeover should mark Codex enabled"
    );

    let auth_after_takeover: serde_json::Value =
        read_json_file(&cc_switch_lib::get_codex_auth_path()).expect("read auth after takeover");
    assert_eq!(
        auth_after_takeover, oauth_auth,
        "enabling route-only takeover must not rewrite Codex OAuth auth.json"
    );

    assert!(
        state
            .db
            .get_live_backup("codex")
            .await
            .expect("read Codex backup after takeover")
            .is_none(),
        "route-only takeover should not create a Codex live backup"
    );

    ProviderService::switch(&state, AppType::Codex, "deepseek-provider")
        .expect("Codex route-only switch should update local route");
    assert_codex_live_files_unchanged(&before);
    let current = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("read current provider after route-only switch");
    assert_eq!(current.as_deref(), Some("deepseek-provider"));
}

#[tokio::test(flavor = "current_thread")]
#[allow(
    clippy::await_holding_lock,
    reason = "this integration-style test must serialize global test HOME and settings mutations across async takeover calls"
)]
async fn codex_route_only_allows_official_switch_while_proxy_enabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "official-live-token",
            "account_id": "acct-live"
        }
    });
    seed_codex_live_files(
        &live_auth,
        "model_provider = \"openai\"\nmodel = \"gpt-5\"\n",
    );

    let mut official_provider = Provider::with_id(
        "official-provider".to_string(),
        "OpenAI Official".to_string(),
        json!({
            "auth": {},
            "config": ""
        }),
        None,
    );
    official_provider.category = Some("official".to_string());

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "deepseek-provider".to_string();
        manager.providers.insert(
            "deepseek-provider".to_string(),
            Provider::with_id(
                "deepseek-provider".to_string(),
                "DeepSeek".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "deepseek-key"},
                    "config": r#"model_provider = "deepseek"
model = "deepseek-chat"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
                }),
                None,
            ),
        );
        manager
            .providers
            .insert("official-provider".to_string(), official_provider);
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let mut proxy_config = state.db.get_proxy_config().await.expect("get proxy config");
    proxy_config.listen_port = 0;
    state
        .db
        .update_proxy_config(proxy_config)
        .await
        .expect("set proxy to ephemeral port");
    let before = codex_live_file_bytes();

    state
        .proxy_service
        .set_takeover_for_app("codex", true)
        .await
        .expect("enable Codex route-only takeover");

    ProviderService::switch(&state, AppType::Codex, "official-provider")
        .expect("Codex provider switch should allow official route-only targets");
    assert_codex_live_files_unchanged(&before);
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current provider")
            .as_deref(),
        Some("official-provider")
    );

    state
        .proxy_service
        .hot_switch_provider("codex", "deepseek-provider")
        .await
        .expect("Codex proxy panel hot switch should allow custom route-only target");
    state
        .proxy_service
        .hot_switch_provider("codex", "official-provider")
        .await
        .expect("Codex proxy panel hot switch should allow official route-only target");
    assert_codex_live_files_unchanged(&before);
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current provider after hot switch")
            .as_deref(),
        Some("official-provider")
    );
}

#[test]
fn provider_service_switch_codex_preserves_live_oauth_when_preservation_off() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "official-oauth-token",
            "account_id": "acct-1"
        }
    });
    let legacy_config = r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    seed_codex_live_files(&live_auth, legacy_config);

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "legacy-provider".to_string();
        manager.providers.insert(
            "legacy-provider".to_string(),
            Provider::with_id(
                "legacy-provider".to_string(),
                "RightCode".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "rightcode-key"},
                    "config": legacy_config
                }),
                None,
            ),
        );
        manager.providers.insert(
            "third-party".to_string(),
            Provider::with_id(
                "third-party".to_string(),
                "AiHubMix".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "third-party-key"},
                    "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "third-party",
        Some("legacy-provider"),
        &before,
    );
}

#[test]
fn provider_service_switch_codex_supports_official_login_provider_without_auth_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "official-oauth-token",
            "account_id": "acct-official"
        }
    });
    seed_codex_live_files(&live_auth, "");

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "legacy-provider".to_string();
        manager.providers.insert(
            "legacy-provider".to_string(),
            Provider::with_id(
                "legacy-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "legacy-key"},
                    "config": r#"model_provider = "legacy"

[model_providers.legacy]
name = "Legacy"
base_url = "https://legacy.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                }),
                None,
            ),
        );
        let mut official_provider = Provider::with_id(
            "official-provider".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": {},
                "config": ""
            }),
            None,
        );
        official_provider.category = Some("official".to_string());
        manager
            .providers
            .insert("official-provider".to_string(), official_provider);
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "official-provider",
        Some("legacy-provider"),
        &before,
    );
}

#[test]
fn provider_service_switch_codex_official_accounts_switch_route_only_without_auth_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let live_auth_a = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "access_token": "official-a-live-token",
            "account_id": "acct-a"
        }
    });
    seed_codex_live_files(&live_auth_a, "");

    let mut official_a = Provider::with_id(
        "official-a".to_string(),
        "Official A".to_string(),
        json!({
            "auth": {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {
                    "access_token": "stale-a-token",
                    "account_id": "acct-a"
                }
            },
            "config": ""
        }),
        None,
    );
    official_a.category = Some("official".to_string());

    let mut official_b = Provider::with_id(
        "official-b".to_string(),
        "Official B".to_string(),
        json!({
            "auth": {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": null,
                "tokens": {
                    "access_token": "official-b-token",
                    "account_id": "acct-b"
                }
            },
            "config": ""
        }),
        None,
    );
    official_b.category = Some("official".to_string());

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "official-a".to_string();
        manager
            .providers
            .insert("official-a".to_string(), official_a);
        manager
            .providers
            .insert("official-b".to_string(), official_b);
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "official-b",
        Some("official-a"),
        &before,
    );
}

#[test]
fn reapply_codex_official_live_returns_disabled_and_preserves_live_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let live_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": { "access_token": "official-token", "account_id": "acct" }
    });
    seed_codex_live_files(&live_auth, "model = \"gpt-5\"\n");

    let mut config = MultiAppConfig::default();
    let manager = config
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager");
    manager.current = "official-provider".to_string();
    let mut provider = Provider::with_id(
        "official-provider".to_string(),
        "Official".to_string(),
        json!({ "auth": live_auth, "config": "" }),
        None,
    );
    provider.category = Some("official".to_string());
    manager.providers.insert(provider.id.clone(), provider);

    let state = create_test_state_with_config(&config).expect("create test state");
    let before = codex_live_file_bytes();
    let err = cc_switch_lib::reapply_current_codex_official_live(&state)
        .expect_err("RouteOnly must reject official live reapply");
    match err {
        AppError::Localized { key, .. } => assert_eq!(key, "codex.live.write_disabled"),
        other => panic!("unexpected error: {other:?}"),
    }
    assert_codex_live_files_unchanged(&before);
}

#[test]
fn provider_service_switch_codex_route_only_keeps_provider_specific_model_provider_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let legacy_auth = json!({ "OPENAI_API_KEY": "rightcode-key" });
    let provider_a_config = r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    seed_codex_live_files(&legacy_auth, provider_a_config);

    let mut initial_config = MultiAppConfig::default();
    {
        let manager = initial_config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "provider-a".to_string();
        manager.providers.insert(
            "provider-a".to_string(),
            Provider::with_id(
                "provider-a".to_string(),
                "RightCode".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "rightcode-key"},
                    "config": provider_a_config
                }),
                None,
            ),
        );
        manager.providers.insert(
            "provider-b".to_string(),
            Provider::with_id(
                "provider-b".to_string(),
                "AiHubMix".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "aihubmix-key"},
                    "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"
profile = "work"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true

[profiles.work]
model_provider = "aihubmix"
model = "gpt-5.4"
"#
                }),
                None,
            ),
        );
        manager.providers.insert(
            "provider-c".to_string(),
            Provider::with_id(
                "provider-c".to_string(),
                "Vendor C".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "vendor-c-key"},
                    "config": r#"model_provider = "vendor_c"
model = "gpt-5.4"

[model_providers.vendor_c]
name = "Vendor C"
base_url = "https://vendor-c.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&initial_config).expect("create test state");
    let before = codex_live_file_bytes();

    assert_codex_switch_route_only_updates_current(
        &state,
        "provider-b",
        Some("provider-a"),
        &before,
    );

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers after route-only switch");
    let provider_b_config = providers
        .get("provider-b")
        .expect("provider b exists")
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .expect("provider b config");
    let parsed: toml::Value = toml::from_str(provider_b_config).expect("parse provider b config");

    assert_eq!(
        parsed.get("model_provider").and_then(|v| v.as_str()),
        Some("aihubmix"),
        "route-only switch should leave provider b's storage-specific model_provider id"
    );
    assert!(
        parsed
            .get("model_providers")
            .and_then(|v| v.get("aihubmix"))
            .is_some(),
        "provider b should keep its own model_providers table after route-only switch"
    );
    assert_eq!(
        parsed
            .get("profiles")
            .and_then(|v| v.get("work"))
            .and_then(|v| v.get("model_provider"))
            .and_then(|v| v.as_str()),
        Some("aihubmix"),
        "profile overrides should be restored to provider b's storage-specific id"
    );
}

#[test]
fn sync_current_provider_for_app_keeps_live_takeover_and_updates_restore_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "current-provider".to_string();

        let mut provider = Provider::with_id(
            "current-provider".to_string(),
            "Current".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "real-token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        manager
            .providers
            .insert("current-provider".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{ "includeCoAuthoredBy": false }"#.to_string()),
        )
        .expect("set common config snippet");

    let taken_over_live = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:5000",
            "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"
        }
    });
    let settings_path = get_claude_settings_path();
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("create dir");
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&taken_over_live).expect("serialize taken over live"),
    )
    .expect("write taken over live");

    futures::executor::block_on(state.db.save_live_backup("claude", "{\"env\":{}}"))
        .expect("seed live backup");

    let mut proxy_config = futures::executor::block_on(state.db.get_proxy_config_for_app("claude"))
        .expect("get proxy config");
    proxy_config.enabled = true;
    futures::executor::block_on(state.db.update_proxy_config_for_app(proxy_config))
        .expect("enable takeover");

    ProviderService::sync_current_provider_for_app(&state, AppType::Claude)
        .expect("sync current provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read live settings after sync");
    assert_eq!(
        live_after, taken_over_live,
        "sync should not overwrite live config while takeover is active"
    );

    let backup = futures::executor::block_on(state.db.get_live_backup("claude"))
        .expect("get live backup")
        .expect("backup exists");
    let backup_value: serde_json::Value =
        serde_json::from_str(&backup.original_config).expect("parse backup value");

    assert_eq!(
        backup_value
            .get("includeCoAuthoredBy")
            .and_then(|v| v.as_bool()),
        Some(false),
        "restore backup should receive the updated effective config"
    );
    assert_eq!(
        backup_value
            .get("env")
            .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str()),
        Some("real-token"),
        "restore backup should preserve the provider token rather than proxy placeholder"
    );
}

#[test]
fn switch_codex_provider_with_takeover_live_but_stopped_proxy_updates_route_only() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": "oauth-access",
            "id_token": "oauth-id"
        }
    });
    let old_provider_config = r#"model_provider = "deepseek"
model = "deepseek-chat"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "old-key"
"#;
    let proxy_live_config = r#"model_provider = "deepseek"
model = "deepseek-chat"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#;
    seed_codex_live_files(&oauth_auth, proxy_live_config);

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();

        let mut old_provider = Provider::with_id(
            "old-provider".to_string(),
            "DeepSeek Old".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "old-key"},
                "config": old_provider_config
            }),
            None,
        );
        old_provider.category = Some("custom".to_string());
        manager
            .providers
            .insert("old-provider".to_string(), old_provider);

        let mut new_provider = Provider::with_id(
            "new-provider".to_string(),
            "DeepSeek New".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "new-key"},
                "config": r#"model_provider = "deepseek-new"
model = "deepseek-reasoner"

[model_providers.deepseek-new]
name = "DeepSeek New"
base_url = "https://new.deepseek.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        new_provider.category = Some("custom".to_string());
        manager
            .providers
            .insert("new-provider".to_string(), new_provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    futures::executor::block_on(
        state.db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": oauth_auth,
                "config": old_provider_config
            }))
            .expect("serialize backup"),
        ),
    )
    .expect("seed Codex live backup");

    assert!(
        !futures::executor::block_on(state.proxy_service.is_running()),
        "fixture keeps the proxy server stopped"
    );

    let before = codex_live_file_bytes();
    ProviderService::switch(&state, AppType::Codex, "new-provider")
        .expect("Codex takeover switch should update local route only");
    assert_codex_live_files_unchanged(&before);

    assert!(
        futures::executor::block_on(state.db.get_live_backup("codex"))
            .expect("get Codex backup")
            .is_none(),
        "route-only switch should clear stale Codex live backup"
    );

    let current = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get current provider");
    assert_eq!(current.as_deref(), Some("new-provider"));
}

#[test]
fn explicitly_cleared_common_snippet_is_not_auto_extracted() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");
    state
        .db
        .set_config_snippet_cleared(AppType::Claude.as_str(), true)
        .expect("mark snippet explicitly cleared");

    assert!(
        !state
            .db
            .should_auto_extract_config_snippet(AppType::Claude.as_str())
            .expect("check auto-extract eligibility"),
        "explicitly cleared snippets should block auto-extraction"
    );

    state
        .db
        .set_config_snippet(AppType::Claude.as_str(), Some("{}".to_string()))
        .expect("set snippet");
    state
        .db
        .set_config_snippet_cleared(AppType::Claude.as_str(), false)
        .expect("clear explicit-empty marker");

    assert!(
        !state
            .db
            .should_auto_extract_config_snippet(AppType::Claude.as_str())
            .expect("check auto-extract after snippet saved"),
        "existing snippets should also block auto-extraction"
    );
}

#[test]
fn legacy_common_config_migration_flag_roundtrip() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    assert!(
        !state
            .db
            .is_legacy_common_config_migrated()
            .expect("initial migration flag"),
        "migration flag should default to false"
    );

    state
        .db
        .set_legacy_common_config_migrated(true)
        .expect("set migration flag");
    assert!(
        state
            .db
            .is_legacy_common_config_migrated()
            .expect("read migration flag"),
        "migration flag should persist once set"
    );

    state
        .db
        .set_legacy_common_config_migrated(false)
        .expect("clear migration flag");
    assert!(
        !state
            .db
            .is_legacy_common_config_migrated()
            .expect("read migration flag after clear"),
        "migration flag should be removable for tests/debugging"
    );
}

#[test]
fn switch_packycode_gemini_updates_security_selected_type() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "packy-gemini".to_string();
        manager.providers.insert(
            "packy-gemini".to_string(),
            Provider::with_id(
                "packy-gemini".to_string(),
                "PackyCode".to_string(),
                json!({
                    "env": {
                        "GEMINI_API_KEY": "pk-key",
                        "GOOGLE_GEMINI_BASE_URL": "https://www.packyapi.com"
                    }
                }),
                Some("https://www.packyapi.com".to_string()),
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "packy-gemini")
        .expect("switching to PackyCode Gemini should succeed");

    // Gemini security settings are written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let settings_path = home.join(".gemini").join("settings.json");
    assert!(
        settings_path.exists(),
        "Gemini settings.json should exist at {}",
        settings_path.display()
    );
    let raw = std::fs::read_to_string(&settings_path).expect("read gemini settings.json");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("parse gemini settings.json after switch");

    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "PackyCode Gemini should set security.auth.selectedType"
    );
}

#[test]
fn packycode_partner_meta_triggers_security_flag_even_without_keywords() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "packy-meta".to_string();
        let mut provider = Provider::with_id(
            "packy-meta".to_string(),
            "Generic Gemini".to_string(),
            json!({
                "env": {
                    "GEMINI_API_KEY": "pk-meta",
                    "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com"
                }
            }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            partner_promotion_key: Some("packycode".to_string()),
            ..ProviderMeta::default()
        });
        manager.providers.insert("packy-meta".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "packy-meta")
        .expect("switching to partner meta provider should succeed");

    // Gemini security settings are written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let settings_path = home.join(".gemini").join("settings.json");
    assert!(
        settings_path.exists(),
        "Gemini settings.json should exist at {}",
        settings_path.display()
    );
    let raw = std::fs::read_to_string(&settings_path).expect("read gemini settings.json");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("parse gemini settings.json after switch");

    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "Partner meta should set security.auth.selectedType even without packy keywords"
    );
}

#[test]
fn switch_google_official_gemini_preserves_env_vars() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "google-official".to_string();
        let mut provider = Provider::with_id(
            "google-official".to_string(),
            "Google".to_string(),
            json!({
                "env": {
                    "GEMINI_MODEL": "gemini-2.5-pro"
                }
            }),
            Some("https://ai.google.dev".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            partner_promotion_key: Some("google-official".to_string()),
            ..ProviderMeta::default()
        });
        manager
            .providers
            .insert("google-official".to_string(), provider);
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "google-official")
        .expect("switching to Google official Gemini should succeed");

    // Verify env vars are preserved in ~/.gemini/.env
    let env_path = home.join(".gemini").join(".env");
    assert!(
        env_path.exists(),
        "Gemini .env should exist at {}",
        env_path.display()
    );
    let env_content = std::fs::read_to_string(&env_path).expect("read gemini .env");
    assert!(
        env_content.contains("GEMINI_MODEL=gemini-2.5-pro"),
        "GEMINI_MODEL should be preserved in .env, got: {env_content}"
    );

    // Verify OAuth security flag is still set correctly
    let gemini_settings = home.join(".gemini").join("settings.json");
    let gemini_raw = std::fs::read_to_string(&gemini_settings).expect("read gemini settings");
    let gemini_value: serde_json::Value =
        serde_json::from_str(&gemini_raw).expect("parse gemini settings");
    assert_eq!(
        gemini_value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("oauth-personal"),
        "OAuth security flag should still be set"
    );
}

#[test]
fn provider_service_switch_claude_updates_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let legacy_live = json!({
        "env": {
            "ANTHROPIC_API_KEY": "legacy-key"
        },
        "workspace": {
            "path": "/tmp/workspace"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&legacy_live).expect("serialize legacy live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "stale-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Fresh Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "fresh-key" },
                    "workspace": { "path": "/tmp/new-workspace" }
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Claude, "new-provider")
        .expect("switch provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read claude live settings");
    assert_eq!(
        live_after
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "live settings.json should reflect new provider auth"
    );

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    let current_id = state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let legacy_provider = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    assert_eq!(
        legacy_provider.settings_config, legacy_live,
        "previous provider should receive backfilled live config"
    );
}

/// 切走勾选了通用配置的 Claude 供应商时，应把它 live 里新增的可共享键
/// （用户直接在应用内装插件/改偏好）捕获进通用配置片段，并带到下一个供应商。
#[test]
fn switch_claude_syncs_new_shared_keys_from_live_into_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    // A 的 live = A 私有密钥（含非 Anthropic 的 OpenRouter 凭据）+ 已共享的 theme
    // + 用户刚在应用内新增的 enableAllProjectMcpServers
    let live = json!({
        "env": { "ANTHROPIC_API_KEY": "a-key", "OPENROUTER_API_KEY": "sk-or-leak" },
        "theme": "dark",
        "enableAllProjectMcpServers": true
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&live).expect("serialize live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        let mut provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        provider_a.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("a".to_string(), provider_a);
        let mut provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        provider_b.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("b".to_string(), provider_b);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{"theme":"dark"}"#.to_string()),
        )
        .expect("seed common config snippet");

    ProviderService::switch(&state, AppType::Claude, "b").expect("switch should succeed");

    // 片段应捕获到新增键，并保留已有共享键，且绝不含密钥
    let snippet = state
        .db
        .get_config_snippet(AppType::Claude.as_str())
        .expect("read snippet")
        .expect("snippet present");
    let snippet_value: serde_json::Value =
        serde_json::from_str(&snippet).expect("snippet is valid JSON");
    assert_eq!(
        snippet_value.get("enableAllProjectMcpServers"),
        Some(&json!(true)),
        "newly added shared key should be captured into common config"
    );
    assert_eq!(
        snippet_value.get("theme").and_then(|v| v.as_str()),
        Some("dark"),
        "previously shared key should be preserved"
    );
    assert!(
        snippet_value
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .is_none(),
        "secrets must never leak into the shared snippet"
    );
    assert!(
        snippet_value
            .get("env")
            .and_then(|env| env.get("OPENROUTER_API_KEY"))
            .is_none(),
        "non-Anthropic Claude credentials must never leak into the shared snippet"
    );

    // 新增键应通过通用配置带到 B 的 live
    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read live after switch");
    assert_eq!(
        live_after.get("enableAllProjectMcpServers"),
        Some(&json!(true)),
        "shared key should propagate to the next provider's live config"
    );
    assert!(
        live_after
            .get("env")
            .and_then(|env| env.get("OPENROUTER_API_KEY"))
            .is_none(),
        "leaked credential must not be injected into the next provider's live"
    );
    assert_eq!(
        live_after
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str()),
        Some("b-key"),
        "live should reflect new provider's own auth"
    );
}

/// 用户在应用内删掉一个已共享的键后，切换应把删除同步进通用配置，
/// 且不会在切到下一个供应商时被重新注入（否则会"删不掉"）。
#[test]
fn switch_claude_syncs_deletions_from_live_into_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    // live 里 theme 还在，但用户已删掉 enableAllProjectMcpServers
    let live = json!({
        "env": { "ANTHROPIC_API_KEY": "a-key" },
        "theme": "dark"
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&live).expect("serialize live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        let mut provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        provider_a.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("a".to_string(), provider_a);
        let mut provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        provider_b.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("b".to_string(), provider_b);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    // 片段里仍残留 enableAllProjectMcpServers（上次共享的）
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{"theme":"dark","enableAllProjectMcpServers":true}"#.to_string()),
        )
        .expect("seed common config snippet");

    ProviderService::switch(&state, AppType::Claude, "b").expect("switch should succeed");

    let snippet = state
        .db
        .get_config_snippet(AppType::Claude.as_str())
        .expect("read snippet")
        .expect("snippet present");
    let snippet_value: serde_json::Value =
        serde_json::from_str(&snippet).expect("snippet is valid JSON");
    assert!(
        snippet_value.get("enableAllProjectMcpServers").is_none(),
        "deleted key should be removed from common config"
    );
    assert_eq!(
        snippet_value.get("theme").and_then(|v| v.as_str()),
        Some("dark"),
        "untouched shared key should remain"
    );

    // 切到 B 后 live 不应再出现被删除的键
    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read live after switch");
    assert!(
        live_after.get("enableAllProjectMcpServers").is_none(),
        "deleted shared key must not be re-injected into the next provider"
    );
}

/// Codex 全局使用 RouteOnly：切换只更新数据库路由，不读取用户自主管理的
/// live 配置回填账号，也不把通用配置片段写入 live 文件。
#[test]
fn switch_codex_route_only_does_not_sync_live_into_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // A 激活状态下的 live：A 专属路由 + 已共享的 [tui] + 用户刚加的
    // disable_response_storage + cc-switch 注入产物 + MCP 同步投影
    // + 顶层 wire_api（无 model_provider 时的 fallback 写法，属 A 的路由语义）
    // + 历史错误格式 [mcp.servers]（sync_all_enabled 清不掉的孤儿形态）
    let live_config = r#"model = "gpt-5.5"
model_provider = "aprov"
wire_api = "chat"
experimental_bearer_token = "sk-a-live-secret"
model_catalog_json = "cc-switch-model-catalog.json"
web_search = "disabled"
disable_response_storage = true

[tui]
notifications = true

[model_providers.aprov]
name = "A Prov"
base_url = "https://a.example/v1"
wire_api = "responses"

[mcp_servers.echo]
type = "stdio"
command = "echo"

[mcp.servers.ghost-legacy]
command = "ghost-cmd"
"#;
    seed_codex_live_files(&json!({ "OPENAI_API_KEY": "sk-a" }), live_config);

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "a".to_string();
        let mut provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-a" },
                "config": "model = \"gpt-5.5\"\nmodel_provider = \"aprov\"\n\n[model_providers.aprov]\nname = \"A Prov\"\nbase_url = \"https://a.example/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        );
        provider_a.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("a".to_string(), provider_a);
        let mut provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-b" },
                "config": "model = \"gpt-5.5\"\nmodel_provider = \"bprov\"\n\n[model_providers.bprov]\nname = \"B Prov\"\nbase_url = \"https://b.example/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        );
        provider_b.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("b".to_string(), provider_b);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Codex.as_str(),
            Some("[tui]\nnotifications = true\n".to_string()),
        )
        .expect("seed codex common config snippet");
    let before = codex_live_file_bytes();

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch should succeed");

    // RouteOnly 不把用户 live 中的新增键回填到账号通用配置。
    let snippet = state
        .db
        .get_config_snippet(AppType::Codex.as_str())
        .expect("read snippet")
        .expect("snippet present");
    assert!(
        !snippet.contains("disable_response_storage"),
        "route-only switch must not capture live keys, got: {snippet}"
    );
    assert!(
        snippet.contains("notifications = true"),
        "stored common config should remain unchanged, got: {snippet}"
    );
    assert_codex_live_files_unchanged(&before);

    // A 的数据库配置也不能被不相关的 live 内容回填污染。
    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("read providers after switch");
    let stored_a = providers.get("a").expect("provider a exists");
    let stored_a_config = stored_a
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        stored_a_config.contains("model_provider = \"aprov\""),
        "provider-owned routing must remain unchanged, got: {stored_a_config}"
    );
    for forbidden in [
        "disable_response_storage",
        "notifications",
        "mcp_servers",
        "experimental_bearer_token",
        "ghost-legacy",
    ] {
        assert!(
            !stored_a_config.contains(forbidden),
            "'{forbidden}' must not be imported from live in route-only mode, got: {stored_a_config}"
        );
    }
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current provider")
            .as_deref(),
        Some("b")
    );
}

/// RouteOnly 不以用户自主管理的 live 文件为数据库通用配置的事实来源；
/// live 中的删除不会隐式改写数据库片段，也不会触发目标账号 live 写入。
#[test]
fn switch_codex_route_only_does_not_apply_common_config_deletions_from_live() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // 片段里有两个共享键，但用户已在 live 里删掉 disable_response_storage
    let live_config = r#"model_provider = "aprov"

[tui]
notifications = true

[model_providers.aprov]
name = "A Prov"
base_url = "https://a.example/v1"
wire_api = "responses"
"#;
    seed_codex_live_files(&json!({ "OPENAI_API_KEY": "sk-a" }), live_config);

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "a".to_string();
        for (id, name, prov_key) in [("a", "A", "aprov"), ("b", "B", "bprov")] {
            let mut provider = Provider::with_id(
                id.to_string(),
                name.to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": format!("sk-{id}") },
                    "config": format!("model_provider = \"{prov_key}\"\n\n[model_providers.{prov_key}]\nname = \"{name} Prov\"\nbase_url = \"https://{id}.example/v1\"\nwire_api = \"responses\"\n")
                }),
                None,
            );
            provider.meta = Some(ProviderMeta {
                common_config_enabled: Some(true),
                ..Default::default()
            });
            manager.providers.insert(id.to_string(), provider);
        }
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Codex.as_str(),
            Some("disable_response_storage = true\n\n[tui]\nnotifications = true\n".to_string()),
        )
        .expect("seed codex common config snippet");
    let before = codex_live_file_bytes();

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch should succeed");

    let snippet = state
        .db
        .get_config_snippet(AppType::Codex.as_str())
        .expect("read snippet")
        .expect("snippet present");
    assert!(
        snippet.contains("disable_response_storage = true"),
        "route-only switch must not rewrite the stored snippet, got: {snippet}"
    );
    assert!(
        snippet.contains("notifications = true"),
        "kept shared key should remain in the snippet, got: {snippet}"
    );
    assert_codex_live_files_unchanged(&before);
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Codex.as_str())
            .expect("read current provider")
            .as_deref(),
        Some("b")
    );
}

/// 未勾选"写入通用配置"的供应商，其 live 改动不应自动污染通用配置片段。
#[test]
fn switch_claude_does_not_sync_common_config_for_opted_out_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let live = json!({
        "env": { "ANTHROPIC_API_KEY": "a-key" },
        "providerSpecific": "x"
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&live).expect("serialize live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        // A 未勾选通用配置（meta = None）
        manager.providers.insert(
            "a".to_string(),
            Provider::with_id(
                "a".to_string(),
                "A".to_string(),
                json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
                None,
            ),
        );
        manager.providers.insert(
            "b".to_string(),
            Provider::with_id(
                "b".to_string(),
                "B".to_string(),
                json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet(
            AppType::Claude.as_str(),
            Some(r#"{"theme":"dark"}"#.to_string()),
        )
        .expect("seed common config snippet");

    ProviderService::switch(&state, AppType::Claude, "b").expect("switch should succeed");

    let snippet = state
        .db
        .get_config_snippet(AppType::Claude.as_str())
        .expect("read snippet")
        .expect("snippet present");
    let snippet_value: serde_json::Value =
        serde_json::from_str(&snippet).expect("snippet is valid JSON");
    assert!(
        snippet_value.get("providerSpecific").is_none(),
        "opted-out provider's live changes must not pollute the shared snippet"
    );
    assert_eq!(
        snippet_value.get("theme").and_then(|v| v.as_str()),
        Some("dark"),
        "snippet should stay unchanged for opted-out providers"
    );
}

/// 用户显式清空过通用配置（_cleared）后，切换不应把片段重新塞回来。
#[test]
fn switch_claude_respects_explicitly_cleared_common_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let live = json!({
        "env": { "ANTHROPIC_API_KEY": "a-key" },
        "theme": "dark"
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&live).expect("serialize live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        let mut provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        provider_a.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("a".to_string(), provider_a);
        let mut provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        provider_b.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });
        manager.providers.insert("b".to_string(), provider_b);
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    state
        .db
        .set_config_snippet_cleared(AppType::Claude.as_str(), true)
        .expect("mark snippet cleared");

    ProviderService::switch(&state, AppType::Claude, "b").expect("switch should succeed");

    assert!(
        state
            .db
            .get_config_snippet(AppType::Claude.as_str())
            .expect("read snippet")
            .is_none(),
        "explicitly cleared snippet must not be resurrected by switch-away sync"
    );
}

#[test]
fn provider_service_switch_missing_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let err = ProviderService::switch(&state, AppType::Claude, "missing")
        .expect_err("switching missing provider should fail");
    match err {
        AppError::Message(msg) => {
            assert!(
                msg.contains("不存在") || msg.contains("not found"),
                "expected provider not found message, got {msg}"
            );
        }
        other => panic!("expected Message error for provider not found, got {other:?}"),
    }
}

#[test]
fn provider_service_switch_codex_missing_auth_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.providers.insert(
            "invalid".to_string(),
            Provider::with_id(
                "invalid".to_string(),
                "Broken Codex".to_string(),
                json!({
                    "config": "[mcp_servers.test]\ncommand = \"noop\""
                }),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    let err = ProviderService::switch(&state, AppType::Codex, "invalid")
        .expect_err("invalid Codex provider should be rejected before route update");
    assert!(
        err.to_string().contains("auth"),
        "error should report invalid provider auth, got: {err:?}"
    );
}

#[test]
fn provider_service_delete_codex_removes_provider_and_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "keep-key"},
                    "config": ""
                }),
                None,
            ),
        );
        manager.providers.insert(
            "to-delete".to_string(),
            Provider::with_id(
                "to-delete".to_string(),
                "DeleteCodex".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "delete-key"},
                    "config": ""
                }),
                None,
            ),
        );
    }

    let sanitized = sanitize_provider_name("DeleteCodex");
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    let auth_path = codex_dir.join(format!("auth-{sanitized}.json"));
    let cfg_path = codex_dir.join(format!("config-{sanitized}.toml"));
    std::fs::write(&auth_path, "{}").expect("seed auth file");
    std::fs::write(&cfg_path, "base_url = \"https://example\"").expect("seed config file");

    let app_state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::delete(&app_state, AppType::Codex, "to-delete")
        .expect("delete provider should succeed");

    let providers = app_state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get all providers");
    assert!(
        !providers.contains_key("to-delete"),
        "provider entry should be removed"
    );
    // v3.7.0+ 不再使用供应商特定文件（如 auth-*.json, config-*.toml）
    // 删除供应商只影响数据库记录，不清理这些旧格式文件
}

#[test]
fn provider_service_delete_claude_removes_provider_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "keep-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "delete".to_string(),
            Provider::with_id(
                "delete".to_string(),
                "DeleteClaude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "delete-key" }
                }),
                None,
            ),
        );
    }

    let sanitized = sanitize_provider_name("DeleteClaude");
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("create claude dir");
    let by_name = claude_dir.join(format!("settings-{sanitized}.json"));
    let by_id = claude_dir.join("settings-delete.json");
    std::fs::write(&by_name, "{}").expect("seed settings by name");
    std::fs::write(&by_id, "{}").expect("seed settings by id");

    let app_state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::delete(&app_state, AppType::Claude, "delete").expect("delete claude provider");

    let providers = app_state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    assert!(
        !providers.contains_key("delete"),
        "claude provider should be removed"
    );
    // v3.7.0+ 不再使用供应商特定文件（如 settings-*.json）
    // 删除供应商只影响数据库记录，不清理这些旧格式文件
}

#[test]
fn provider_service_delete_current_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "keep".to_string();
        manager.providers.insert(
            "keep".to_string(),
            Provider::with_id(
                "keep".to_string(),
                "Keep".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "keep-key" }
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = ProviderService::delete(&app_state, AppType::Claude, "keep")
        .expect_err("deleting current provider should fail");
    match err {
        AppError::Localized { zh, .. } => assert!(
            zh.contains("不能删除当前正在使用的供应商")
                || zh.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {zh}"
        ),
        AppError::Config(msg) => assert!(
            msg.contains("不能删除当前正在使用的供应商")
                || msg.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {msg}"
        ),
        AppError::Message(msg) => assert!(
            msg.contains("不能删除当前正在使用的供应商")
                || msg.contains("无法删除当前正在使用的供应商"),
            "unexpected message: {msg}"
        ),
        other => panic!("expected Config/Message error, got {other:?}"),
    }
}

#[test]
fn recover_from_crash_without_backup_cleans_placeholder_instead_of_writing_it_back() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // 接管态 Claude Live，且 DB 中无备份（模拟切换 app_config_dir 后新库首启的场景）
    let taken_over_live = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
            "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"
        }
    });
    let settings_path = get_claude_settings_path();
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("create dir");
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&taken_over_live).expect("serialize taken over live"),
    )
    .expect("write taken over live");

    let state = create_test_state().expect("create test state");

    // 模拟历史异常：接管态 Live 已被导入成 current provider（SSOT 被污染）
    let provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        taken_over_live.clone(),
        None,
    );
    state
        .db
        .save_provider(AppType::Claude.as_str(), &provider)
        .expect("save placeholder provider");
    state
        .db
        .set_current_provider(AppType::Claude.as_str(), "default")
        .expect("set current provider");

    futures::executor::block_on(state.proxy_service.recover_from_crash())
        .expect("recover from crash");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read live settings after recovery");
    let env = live_after.get("env").cloned().unwrap_or_else(|| json!({}));
    assert_ne!(
        env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
        Some("PROXY_MANAGED"),
        "recovery must not write the placeholder back to live"
    );
    assert!(
        env.get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(|url| !url.starts_with("http://127.0.0.1"))
            .unwrap_or(true),
        "recovery must drop the local proxy base URL"
    );
}
