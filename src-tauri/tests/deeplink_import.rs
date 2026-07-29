use std::sync::Arc;

use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, import_provider_from_deeplink, parse_deeplink_url,
    AppState, AppType, Database, Provider, ProviderService,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_codex_live_files() -> (Vec<u8>, Vec<u8>) {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();
    std::fs::create_dir_all(auth_path.parent().expect("auth.json parent"))
        .expect("create Codex live config directory");

    let auth_bytes = br#"{"OPENAI_API_KEY":"sentinel-live-key"}
"#
    .to_vec();
    let config_bytes = b"model = \"gpt-5-codex\"\nmodel_provider = \"openai\"\n".to_vec();
    std::fs::write(&auth_path, &auth_bytes).expect("seed auth.json");
    std::fs::write(&config_path, &config_bytes).expect("seed config.toml");

    (auth_bytes, config_bytes)
}

fn seed_current_codex_provider(db: &Database, id: &str) {
    let provider = Provider::with_id(
        id.to_string(),
        "Previous Codex".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "previous-route-key"},
            "config": "model = \"gpt-5-codex\"\n"
        }),
        Some("https://previous.example".to_string()),
    );
    db.save_provider(AppType::Codex.as_str(), &provider)
        .expect("seed previous Codex provider");
    db.set_current_provider(AppType::Codex.as_str(), id)
        .expect("select previous Codex provider");
}

fn assert_codex_live_files_unchanged(before: &(Vec<u8>, Vec<u8>)) {
    let auth_after = std::fs::read(get_codex_auth_path()).expect("read auth.json after import");
    let config_after =
        std::fs::read(get_codex_config_path()).expect("read config.toml after import");

    assert_eq!(
        auth_after, before.0,
        "auth.json bytes must remain unchanged"
    );
    assert_eq!(
        config_after, before.1,
        "config.toml bytes must remain unchanged"
    );
    assert_eq!(
        Sha256::digest(&auth_after),
        Sha256::digest(&before.0),
        "auth.json SHA256 must remain unchanged"
    );
    assert_eq!(
        Sha256::digest(&config_after),
        Sha256::digest(&before.1),
        "config.toml SHA256 must remain unchanged"
    );
}

#[test]
fn deeplink_import_claude_provider_persists_to_db() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=DeepLink%20Claude&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-claude-key&model=claude-sonnet-4&icon=claude";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    // Verify DB state
    let providers = db.get_all_providers("claude").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("claude"));
    let auth_token = provider
        .settings_config
        .pointer("/env/ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str());
    let base_url = provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str());
    assert_eq!(auth_token, request.api_key.as_deref());
    assert_eq!(base_url, request.endpoint.as_deref());
}

#[test]
fn deeplink_import_codex_provider_persists_to_db_without_live_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&icon=openai";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    seed_current_codex_provider(db.as_ref(), "previous-codex");
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    let auth_value = provider
        .settings_config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(|v| v.as_str());
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(auth_value, request.api_key.as_deref());
    assert!(
        config_text.contains(request.endpoint.as_deref().unwrap()),
        "config.toml content should contain endpoint"
    );
    assert!(
        config_text.contains("model = \"gpt-4o\""),
        "config.toml content should contain model setting"
    );
    assert!(
        !auth_path.exists(),
        "Codex deeplink import should not create auth.json"
    );
    assert!(
        !config_path.exists(),
        "Codex deeplink import should not create config.toml"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).expect("read current Codex provider"),
        "previous-codex",
        "missing enabled flag must not change the current Codex route"
    );
}

#[test]
fn deeplink_import_codex_provider_enabled_switches_route_only_without_live_write() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let live_before = seed_codex_live_files();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&enabled=true";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    seed_current_codex_provider(db.as_ref(), "previous-codex");
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("enabled Codex import should select the new local route");

    assert!(
        db.get_all_providers("codex")
            .expect("get providers")
            .contains_key(&provider_id),
        "enabled Codex import should persist the provider"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).expect("read current Codex provider"),
        provider_id,
        "enabled Codex import should select the new RouteOnly target"
    );
    assert_codex_live_files_unchanged(&live_before);
    assert!(
        futures::executor::block_on(db.get_live_backup(AppType::Codex.as_str()))
            .expect("read Codex live backup")
            .is_none(),
        "RouteOnly import must not create a Codex live backup"
    );
}

#[test]
fn deeplink_import_codex_provider_disabled_keeps_current_route_and_live_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let live_before = seed_codex_live_files();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=Disabled%20DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-disabled-codex-key&model=gpt-4o&enabled=false";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    seed_current_codex_provider(db.as_ref(), "previous-codex");
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("disabled Codex import should succeed");

    assert!(
        db.get_all_providers("codex")
            .expect("get providers")
            .contains_key(&provider_id),
        "disabled Codex import should persist the provider"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).expect("read current Codex provider"),
        "previous-codex",
        "enabled=false must not change the current Codex route"
    );
    assert_codex_live_files_unchanged(&live_before);
}
