use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::http_client;
use crate::proxy::types::{AppProxyConfig, GlobalProxyConfig, ProxyConfig};
use crate::services::model_fetch;
use crate::services::provider::ProviderService;
use crate::services::stream_check::{HealthStatus, StreamCheckResult, StreamCheckService};
use crate::services::{McpService, PromptService, ProviderSortUpdate, SkillService};
use crate::store::AppState;
use regex::Regex;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{str::FromStr, sync::LazyLock};
use toml_edit::visit_mut::{visit_table_like_kv_mut, VisitMut};

const SECRET_CONFIGURED_PLACEHOLDER: &str = "secret_configured";
const XAI_OAUTH_WEB_ERROR: &str = "WebUI 不允许创建、修改、删除或查看 xAI OAuth 供应商编辑详情";
static SECRET_ASSIGNMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)(?P<prefix>\b(?:[a-z0-9_.-]*(?:api[_-]?key|auth[_-]?token|access[_-]?token|refresh[_-]?token|id[_-]?token|bearer[_-]?token|password|client[_-]?secret|secret[_-]?access[_-]?key))\b["']?\s*[:=]\s*["']?)(?P<value>[^"'\n\r,#}\]]+)(?P<suffix>["']?)"#,
    )
    .expect("secret assignment regex")
});

pub async fn dispatch(
    state: &AppState,
    command: &str,
    args: Value,
    production: bool,
) -> Result<Value, String> {
    let reveal_form_api_keys = matches!(
        command,
        "get_provider_for_edit" | "get_universal_provider_for_edit"
    );
    let data = match command {
        // Provider management
        "get_providers" => {
            let app = app_type(arg(&args, "app")?)?;
            to_value(ProviderService::list(state, app)?)?
        }
        "get_provider_for_edit" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            let provider = state
                .db
                .get_provider_by_id(&id, app.as_str())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("供应商不存在: {id}"))?;
            reject_xai_oauth_web_provider(&provider)?;
            to_value(provider)?
        }
        "get_current_provider" => {
            let app = app_type(arg(&args, "app")?)?;
            to_value(ProviderService::current(state, app)?)?
        }
        "add_provider" => {
            let app = app_type(arg(&args, "app")?)?;
            let provider: Provider = arg(&args, "provider")?;
            reject_xai_oauth_web_provider(&provider)?;
            if let Some(existing) = state
                .db
                .get_provider_by_id(&provider.id, app.as_str())
                .map_err(|e| e.to_string())?
            {
                reject_xai_oauth_web_provider(&existing)?;
            }
            reject_unresolved_secret_placeholders(&provider.settings_config)?;
            validate_provider_surface(&provider, production)?;
            let add_to_live = opt_arg(&args, "addToLive")?.unwrap_or(false);
            to_value(ProviderService::add(state, app, provider, add_to_live)?)?
        }
        "update_provider" => {
            let app = app_type(arg(&args, "app")?)?;
            let mut provider: Provider = arg(&args, "provider")?;
            let original_id: Option<String> = opt_arg(&args, "originalId")?;
            let original_lookup = original_id.clone().unwrap_or_else(|| provider.id.clone());
            reject_xai_oauth_web_provider(&provider)?;
            if let Some(existing) = state
                .db
                .get_provider_by_id(&original_lookup, app.as_str())
                .map_err(|e| e.to_string())?
            {
                reject_xai_oauth_web_provider(&existing)?;
            }
            restore_provider_secret_placeholders(state, &app, &original_lookup, &mut provider)?;
            validate_provider_surface(&provider, production)?;
            to_value(ProviderService::update(
                state,
                app,
                original_id.as_deref(),
                provider,
            )?)?
        }
        "delete_provider" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            if let Some(existing) = state
                .db
                .get_provider_by_id(&id, app.as_str())
                .map_err(|e| e.to_string())?
            {
                reject_xai_oauth_web_provider(&existing)?;
            }
            ProviderService::delete(state, app, &id)?;
            json!(true)
        }
        "remove_provider_from_live_config" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            reject_xai_oauth_web_provider_by_id(state, &app, &id)?;
            ProviderService::remove_from_live_config(state, app, &id)?;
            json!(true)
        }
        "switch_provider" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            reject_xai_oauth_web_provider_by_id(state, &app, &id)?;
            to_value(ProviderService::switch(state, app, &id)?)?
        }
        "update_providers_sort_order" => {
            let app = app_type(arg(&args, "app")?)?;
            let updates: Vec<ProviderSortUpdate> = arg(&args, "updates")?;
            for update in &updates {
                reject_xai_oauth_web_provider_by_id(state, &app, &update.id)?;
            }
            ProviderService::update_sort_order(state, app, updates)?;
            json!(true)
        }
        "update_tray_menu" => json!(true),
        "import_default_config" => {
            let app = app_type(arg(&args, "app")?)?;
            to_value(ProviderService::import_default_config(state, app)?)?
        }
        "ensure_codex_official_provider" => to_value(
            state
                .db
                .ensure_official_seed_by_id(
                    crate::database::CODEX_OFFICIAL_PROVIDER_ID,
                    AppType::Codex,
                )
                .map_err(|e| e.to_string())?,
        )?,
        "get_universal_providers" => to_value(ProviderService::list_universal(state)?)?,
        "get_universal_provider" => {
            let id: String = arg(&args, "id")?;
            to_value(ProviderService::get_universal(state, &id)?)?
        }
        "get_universal_provider_for_edit" => {
            let id: String = arg(&args, "id")?;
            to_value(ProviderService::get_universal(state, &id)?)?
        }
        "upsert_universal_provider" => {
            let mut provider: crate::provider::UniversalProvider = arg(&args, "provider")?;
            restore_universal_provider_secret_placeholder(state, &mut provider)?;
            if production {
                validate_value_urls(&serde_json::to_value(&provider).map_err(|e| e.to_string())?)?;
            }
            ProviderService::upsert_universal(state, provider)?;
            json!(true)
        }
        "delete_universal_provider" => {
            let id: String = arg(&args, "id")?;
            to_value(ProviderService::delete_universal(state, &id)?)?
        }
        "sync_universal_provider" => {
            let id: String = arg(&args, "id")?;
            to_value(ProviderService::sync_universal_to_apps(state, &id)?)?
        }

        // Managed OAuth account operations available to the WebUI.
        "auth_start_login" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            let github_domain: Option<String> = opt_arg(&args, "githubDomain")?;
            to_value(
                state
                    .managed_auth
                    .web_start_login(&auth_provider, github_domain.as_deref())
                    .await?,
            )?
        }
        "auth_poll_for_account" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            let device_code: String = arg(&args, "deviceCode")?;
            let github_domain: Option<String> = opt_arg(&args, "githubDomain")?;
            to_value(
                state
                    .managed_auth
                    .web_poll_for_account(&auth_provider, &device_code, github_domain.as_deref())
                    .await?,
            )?
        }
        "auth_list_accounts" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            to_value(state.managed_auth.web_list_accounts(&auth_provider).await?)?
        }
        "auth_get_status" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            to_value(state.managed_auth.web_get_status(&auth_provider).await?)?
        }
        "auth_remove_account" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            let account_id: String = arg(&args, "accountId")?;
            state
                .managed_auth
                .web_remove_account(&auth_provider, &account_id)
                .await?;
            Value::Null
        }
        "auth_set_default_account" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            let account_id: String = arg(&args, "accountId")?;
            state
                .managed_auth
                .web_set_default_account(&auth_provider, &account_id)
                .await?;
            Value::Null
        }
        "auth_logout" => {
            let auth_provider: String = arg(&args, "authProvider")?;
            state.managed_auth.web_logout(&auth_provider).await?;
            Value::Null
        }

        // Settings and desktop-only safe fallbacks
        "get_settings" => to_value(crate::settings::get_settings_for_frontend())?,
        "save_settings" => {
            let settings = arg(&args, "settings")?;
            crate::settings::update_settings(settings).map_err(|e| e.to_string())?;
            json!(true)
        }
        "get_rectifier_config" => to_value(state.db.get_rectifier_config()?)?,
        "set_rectifier_config" => {
            let config: crate::proxy::types::RectifierConfig = arg(&args, "config")?;
            state.db.set_rectifier_config(&config)?;
            json!(true)
        }
        "get_user_agent_rewrite_config" => to_value(state.db.get_user_agent_rewrite_config()?)?,
        "set_user_agent_rewrite_config" => {
            let config: crate::proxy::types::UserAgentRewriteConfig = arg(&args, "config")?;
            state.db.set_user_agent_rewrite_config(&config)?;
            json!(true)
        }
        "get_optimizer_config" => to_value(state.db.get_optimizer_config()?)?,
        "set_optimizer_config" => {
            let config: crate::proxy::types::OptimizerConfig = arg(&args, "config")?;
            state.db.set_optimizer_config(&config)?;
            json!(true)
        }
        "get_config_dir" => {
            let app = app_type(arg(&args, "app")?)?;
            to_value(config_dir_for_app(app)?.to_string_lossy())?
        }
        "get_app_config_path" => to_value(crate::config::get_app_config_path().to_string_lossy())?,
        "get_claude_code_config_path" => {
            to_value(crate::config::get_claude_settings_path().to_string_lossy())?
        }
        "get_common_config_snippet" => {
            let app_type: String = arg(&args, "appType")?;
            let app = web_common_config_app_type(&app_type)?;
            to_value(
                state
                    .db
                    .get_config_snippet(app.as_str())
                    .map_err(|e| e.to_string())?,
            )?
        }
        "set_common_config_snippet" => {
            let app_type: String = arg(&args, "appType")?;
            let app = web_common_config_app_type(&app_type)?;
            let mut snippet: String = arg(&args, "snippet")?;
            let old_snippet = state
                .db
                .get_config_snippet(app.as_str())
                .map_err(|e| e.to_string())?;

            if let Some(existing) = old_snippet.as_deref() {
                snippet = restore_embedded_secret_placeholders(&snippet, existing);
            }
            reject_unresolved_secret_placeholders(&Value::String(snippet.clone()))?;
            crate::commands::validate_common_config_snippet(app.as_str(), &snippet)?;

            if let Some(legacy_snippet) = old_snippet
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                ProviderService::migrate_legacy_common_config_usage(
                    state,
                    app.clone(),
                    legacy_snippet,
                )
                .map_err(|e| e.to_string())?;
            }

            let is_cleared = snippet.trim().is_empty();
            let value = (!is_cleared).then_some(snippet);
            state
                .db
                .set_config_snippet(app.as_str(), value)
                .map_err(|e| e.to_string())?;
            state
                .db
                .set_config_snippet_cleared(app.as_str(), is_cleared)
                .map_err(|e| e.to_string())?;
            json!(true)
        }
        "extract_common_config_snippet" => {
            let app_type: String = arg(&args, "appType")?;
            let app = web_common_config_app_type(&app_type)?;
            let settings_config: String = arg(&args, "settingsConfig")?;
            if settings_config.trim().is_empty() {
                return Err("WebUI 提取通用配置必须提供非空 settingsConfig".to_string());
            }
            let settings: Value = serde_json::from_str(&settings_config)
                .map_err(|e| format!("settingsConfig 不是有效 JSON: {e}"))?;
            to_value(
                ProviderService::extract_common_config_snippet_from_settings(app, &settings)
                    .map_err(|e| e.to_string())?,
            )?
        }
        "update_toml_common_config_snippet" => {
            let config_toml: String = arg(&args, "configToml")?;
            let snippet_toml: String = arg(&args, "snippetToml")?;
            let enabled: bool = arg(&args, "enabled")?;
            to_value(
                crate::services::provider::update_toml_common_config_snippet(
                    &config_toml,
                    &snippet_toml,
                    enabled,
                )
                .map_err(|e| e.to_string())?,
            )?
        }
        "get_app_config_dir_override" => Value::Null,
        "set_app_config_dir_override" => json!(false),
        "is_portable_mode" => json!(false),
        "get_auto_launch_status" => json!(false),
        "set_auto_launch" => reject_desktop_command("set_auto_launch")?,
        "restart_app"
        | "check_for_updates"
        | "open_config_folder"
        | "open_app_config_folder"
        | "pick_directory"
        | "save_file_dialog"
        | "open_file_dialog"
        | "open_zip_file_dialog"
        | "open_provider_terminal"
        | "launch_session_terminal"
        | "list_profiles"
        | "create_profile"
        | "update_profile"
        | "delete_profile"
        | "apply_profile"
        | "clear_current_profile"
        | "open_external" => reject_desktop_command(command)?,
        "copy_text_to_clipboard" => json!(true),
        "get_migration_result" | "get_skills_migration_result" => Value::Null,

        // Proxy
        "start_proxy_server" => to_value(state.proxy_service.start().await?)?,
        "stop_proxy_server" | "stop_proxy_with_restore" => {
            state.proxy_service.stop().await?;
            Value::Null
        }
        "get_proxy_status" => to_value(state.proxy_service.get_status().await?)?,
        "is_proxy_running" => to_value(state.proxy_service.is_running().await)?,
        "is_live_takeover_active" => to_value(state.proxy_service.is_takeover_active().await?)?,
        "get_proxy_takeover_status" => to_value(state.proxy_service.get_takeover_status().await?)?,
        "set_proxy_takeover_for_app" => {
            let app_type: String = arg(&args, "appType")?;
            let enabled: bool = arg(&args, "enabled")?;
            state
                .proxy_service
                .set_takeover_for_app(&app_type, enabled)
                .await?;
            Value::Null
        }
        "get_proxy_config" => to_value(state.proxy_service.get_config().await?)?,
        "update_proxy_config" => {
            let config: ProxyConfig = arg(&args, "config")?;
            validate_proxy_listen(&config, production)?;
            state.proxy_service.update_config(&config).await?;
            Value::Null
        }
        "get_global_proxy_config" => to_value(state.db.get_global_proxy_config().await?)?,
        "update_global_proxy_config" => {
            let config: GlobalProxyConfig = arg(&args, "config")?;
            if production
                && !config
                    .listen_address
                    .parse::<std::net::IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false)
            {
                return Err("生产模式下代理监听地址必须是 loopback".to_string());
            }
            state.db.update_global_proxy_config(config).await?;
            Value::Null
        }
        "get_proxy_config_for_app" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.get_proxy_config_for_app(&app_type).await?)?
        }
        "update_proxy_config_for_app" => {
            let config: AppProxyConfig = arg(&args, "config")?;
            state.db.update_proxy_config_for_app(config.clone()).await?;
            state
                .proxy_service
                .update_circuit_breaker_config_for_app(
                    &config.app_type,
                    crate::proxy::CircuitBreakerConfig::from(&config),
                )
                .await?;
            Value::Null
        }
        "switch_proxy_provider" => {
            let app_type: String = arg(&args, "appType")?;
            let provider_id: String = arg(&args, "providerId")?;
            state
                .proxy_service
                .switch_proxy_target(&app_type, &provider_id)
                .await?;
            Value::Null
        }

        // Failover
        "get_failover_queue" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.get_failover_queue(&app_type)?)?
        }
        "get_available_providers_for_failover" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.get_available_providers_for_failover(&app_type)?)?
        }
        "add_to_failover_queue" => {
            let app_type: String = arg(&args, "appType")?;
            let provider_id: String = arg(&args, "providerId")?;
            state.db.add_to_failover_queue(&app_type, &provider_id)?;
            Value::Null
        }
        "remove_from_failover_queue" => {
            let app_type: String = arg(&args, "appType")?;
            let provider_id: String = arg(&args, "providerId")?;
            state
                .db
                .remove_from_failover_queue(&app_type, &provider_id)?;
            Value::Null
        }
        "get_auto_failover_enabled" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(
                state
                    .db
                    .get_proxy_config_for_app(&app_type)
                    .await?
                    .auto_failover_enabled,
            )?
        }
        "set_auto_failover_enabled" => {
            let app_type: String = arg(&args, "appType")?;
            let enabled: bool = arg(&args, "enabled")?;
            set_auto_failover_enabled(state, &app_type, enabled).await?;
            Value::Null
        }
        "get_provider_health" => {
            let provider_id: String = arg(&args, "providerId")?;
            let app_type: String = arg(&args, "appType")?;
            to_value(
                state
                    .db
                    .get_provider_health(&provider_id, &app_type)
                    .await?,
            )?
        }
        "reset_circuit_breaker" => {
            let provider_id: String = arg(&args, "providerId")?;
            let app_type: String = arg(&args, "appType")?;
            state
                .db
                .update_provider_health(&provider_id, &app_type, true, None)
                .await?;
            state
                .proxy_service
                .reset_provider_circuit_breaker(&provider_id, &app_type)
                .await?;
            Value::Null
        }
        "get_circuit_breaker_config" => to_value(state.db.get_circuit_breaker_config().await?)?,
        "update_circuit_breaker_config" => {
            let config = arg(&args, "config")?;
            state.db.update_circuit_breaker_config(&config).await?;
            state
                .proxy_service
                .update_circuit_breaker_configs(config)
                .await?;
            Value::Null
        }
        "get_circuit_breaker_stats" => Value::Null,

        // Usage
        "get_usage_summary" => to_value(state.db.get_usage_summary(
            opt_arg(&args, "startDate")?,
            opt_arg(&args, "endDate")?,
            opt_arg::<String>(&args, "appType")?.as_deref(),
            opt_arg::<String>(&args, "providerName")?.as_deref(),
            opt_arg::<String>(&args, "model")?.as_deref(),
        )?)?,
        "get_usage_summary_by_app" => to_value(state.db.get_usage_summary_by_app(
            opt_arg(&args, "startDate")?,
            opt_arg(&args, "endDate")?,
            opt_arg::<String>(&args, "providerName")?.as_deref(),
            opt_arg::<String>(&args, "model")?.as_deref(),
        )?)?,
        "get_usage_trends" => to_value(state.db.get_daily_trends(
            opt_arg(&args, "startDate")?,
            opt_arg(&args, "endDate")?,
            opt_arg::<String>(&args, "appType")?.as_deref(),
            opt_arg::<String>(&args, "providerName")?.as_deref(),
            opt_arg::<String>(&args, "model")?.as_deref(),
        )?)?,
        "get_provider_stats" => to_value(state.db.get_provider_stats(
            opt_arg(&args, "startDate")?,
            opt_arg(&args, "endDate")?,
            opt_arg::<String>(&args, "appType")?.as_deref(),
            opt_arg::<String>(&args, "providerName")?.as_deref(),
            opt_arg::<String>(&args, "model")?.as_deref(),
        )?)?,
        "get_model_stats" => to_value(state.db.get_model_stats(
            opt_arg(&args, "startDate")?,
            opt_arg(&args, "endDate")?,
            opt_arg::<String>(&args, "appType")?.as_deref(),
            opt_arg::<String>(&args, "providerName")?.as_deref(),
            opt_arg::<String>(&args, "model")?.as_deref(),
        )?)?,
        "get_request_logs" => {
            let filters = arg(&args, "filters")?;
            let page = opt_arg(&args, "page")?.unwrap_or(0);
            let page_size = opt_arg(&args, "pageSize")?.unwrap_or(20);
            to_value(state.db.get_request_logs(&filters, page, page_size)?)?
        }
        "get_request_detail" => {
            let request_id: String = arg(&args, "requestId")?;
            to_value(state.db.get_request_detail(&request_id)?)?
        }
        "check_provider_limits" => {
            let provider_id: String = arg(&args, "providerId")?;
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.check_provider_limits(&provider_id, &app_type)?)?
        }
        "sync_session_usage" | "queryProviderUsage" | "testUsageScript" => {
            reject_desktop_command(command)?
        }
        "get_usage_data_sources" => to_value(
            crate::services::session_usage::get_data_source_breakdown(&state.db)?,
        )?,
        "get_model_pricing" => to_value(get_model_pricing(state)?)?,
        "update_model_pricing" => {
            let model_id: String = arg(&args, "modelId")?;
            let display_name: String = arg(&args, "displayName")?;
            let input_cost: String = arg(&args, "inputCost")?;
            let output_cost: String = arg(&args, "outputCost")?;
            let cache_read_cost: String = arg(&args, "cacheReadCost")?;
            let cache_creation_cost: String = arg(&args, "cacheCreationCost")?;
            update_model_pricing(
                state,
                model_id,
                display_name,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
            )?;
            Value::Null
        }
        "delete_model_pricing" => {
            let model_id: String = arg(&args, "modelId")?;
            delete_model_pricing(state, model_id)?;
            Value::Null
        }
        "get_default_cost_multiplier" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.get_default_cost_multiplier(&app_type).await?)?
        }
        "set_default_cost_multiplier" => {
            let app_type: String = arg(&args, "appType")?;
            let value: String = arg(&args, "value")?;
            state
                .db
                .set_default_cost_multiplier(&app_type, &value)
                .await?;
            Value::Null
        }
        "get_pricing_model_source" => {
            let app_type: String = arg(&args, "appType")?;
            to_value(state.db.get_pricing_model_source(&app_type).await?)?
        }
        "set_pricing_model_source" => {
            let app_type: String = arg(&args, "appType")?;
            let value: String = arg(&args, "value")?;
            state.db.set_pricing_model_source(&app_type, &value).await?;
            Value::Null
        }

        // MCP / Prompts / Skills
        "get_mcp_servers" => to_value(McpService::get_all_servers(state)?)?,
        "upsert_mcp_server" => {
            let server = arg(&args, "server")?;
            McpService::upsert_server(state, server)?;
            Value::Null
        }
        "delete_mcp_server" => {
            let id: String = arg(&args, "id")?;
            to_value(McpService::delete_server(state, &id)?)?
        }
        "toggle_mcp_app" => {
            let server_id: String = arg(&args, "serverId")?;
            let app = app_type(arg(&args, "app")?)?;
            let enabled: bool = arg(&args, "enabled")?;
            McpService::toggle_app(state, &server_id, app, enabled)?;
            Value::Null
        }
        "import_mcp_from_apps" => json!(0),
        "get_prompts" => {
            let app = app_type(arg(&args, "app")?)?;
            to_value(PromptService::get_prompts(state, app)?)?
        }
        "upsert_prompt" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            let prompt = arg(&args, "prompt")?;
            PromptService::upsert_prompt(state, app, &id, prompt)?;
            Value::Null
        }
        "delete_prompt" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            PromptService::delete_prompt(state, app, &id)?;
            Value::Null
        }
        "enable_prompt" => {
            let app = app_type(arg(&args, "app")?)?;
            let id: String = arg(&args, "id")?;
            PromptService::enable_prompt(state, app, &id)?;
            Value::Null
        }
        "import_prompt_from_file" | "get_current_prompt_file_content" => {
            reject_desktop_command(command)?
        }
        "get_installed_skills" => {
            to_value(SkillService::get_all_installed(&state.db).map_err(|e| e.to_string())?)?
        }
        "toggle_skill_app" => {
            let id: String = arg(&args, "id")?;
            let app = app_type(arg(&args, "app")?)?;
            let enabled: bool = arg(&args, "enabled")?;
            SkillService::toggle_app(&state.db, &id, &app, enabled).map_err(|e| e.to_string())?;
            json!(true)
        }
        "scan_unmanaged_skills" => {
            to_value(SkillService::scan_unmanaged(&state.db).map_err(|e| e.to_string())?)?
        }
        "get_skill_repos" => to_value(state.db.get_skill_repos()?)?,
        "add_skill_repo" => {
            let repo = arg(&args, "repo")?;
            state.db.save_skill_repo(&repo)?;
            json!(true)
        }
        "remove_skill_repo" => {
            let owner: String = arg(&args, "owner")?;
            let name: String = arg(&args, "name")?;
            state.db.delete_skill_repo(&owner, &name)?;
            json!(true)
        }
        "discover_available_skills"
        | "check_skill_updates"
        | "update_skill"
        | "install_skill_unified"
        | "uninstall_skill_unified"
        | "restore_skill_backup"
        | "migrate_skill_storage"
        | "search_skills_sh"
        | "install_skills_from_zip" => reject_desktop_command(command)?,

        // Model fetch / checks
        "fetch_models_for_config" => {
            let base_url: String = arg(&args, "baseUrl")?;
            let api_key: String = arg(&args, "apiKey")?;
            let is_full_url: Option<bool> = opt_arg(&args, "isFullUrl")?;
            let models_url: Option<String> = opt_arg(&args, "modelsUrl")?;
            validate_fetch_models_urls(&base_url, models_url.as_deref(), production)?;
            to_value(
                model_fetch::fetch_models(
                    &base_url,
                    &api_key,
                    is_full_url.unwrap_or(false),
                    models_url.as_deref(),
                    None,
                )
                .await?,
            )?
        }
        "get_stream_check_config" => to_value(state.db.get_stream_check_config()?)?,
        "save_stream_check_config" => {
            let config = arg(&args, "config")?;
            state.db.save_stream_check_config(&config)?;
            Value::Null
        }
        "stream_check_provider" => {
            let app = app_type(arg(&args, "appType")?)?;
            let provider_id: String = arg(&args, "providerId")?;
            to_value(stream_check_provider(state, app, provider_id).await?)?
        }
        "stream_check_all_providers" => {
            let app = app_type(arg(&args, "appType")?)?;
            let proxy_targets_only = opt_arg(&args, "proxyTargetsOnly")?.unwrap_or(false);
            to_value(stream_check_all_providers(state, app, proxy_targets_only).await?)?
        }

        // Global upstream proxy
        "get_global_proxy_url" => to_value(state.db.get_global_proxy_url()?)?,
        "set_global_proxy_url" => {
            let url: String = arg(&args, "url")?;
            let url_opt = (!url.trim().is_empty()).then_some(url.as_str());
            http_client::validate_proxy(url_opt)?;
            state.db.set_global_proxy_url(url_opt)?;
            http_client::apply_proxy(url_opt)?;
            Value::Null
        }
        "get_upstream_proxy_status" => {
            let url = http_client::get_current_proxy_url();
            json!({ "enabled": url.is_some(), "proxyUrl": url })
        }
        "test_proxy_url" | "scan_local_proxies" => reject_desktop_command(command)?,

        _ => return Err(format!("WebUI 未开放命令: {command}")),
    };

    let mut data = data;
    if reveal_form_api_keys {
        redact_secrets_for_edit(&mut data);
    } else if matches!(command, "get_providers" | "get_universal_providers") {
        redact_provider_collection(&mut data);
    } else {
        redact_secrets(&mut data);
    }
    Ok(data)
}

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let Some(value) = args.get(key) else {
        return Err(format!("缺少参数: {key}"));
    };
    serde_json::from_value(value.clone()).map_err(|e| format!("参数 {key} 无效: {e}"))
}

fn opt_arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|e| format!("参数 {key} 无效: {e}"))
}

fn app_type(value: String) -> Result<AppType, String> {
    AppType::from_str(&value).map_err(|e| e.to_string())
}

fn web_common_config_app_type(value: &str) -> Result<AppType, String> {
    match value {
        "claude" | "codex" | "gemini" => AppType::from_str(value).map_err(|e| e.to_string()),
        _ => Err(format!("WebUI 不支持 {value} 的通用配置片段")),
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn reject_desktop_command(command: &str) -> Result<Value, String> {
    Err(format!("WebUI 安全边界未开放桌面专属命令: {command}"))
}

fn reject_xai_oauth_web_provider(provider: &Provider) -> Result<(), String> {
    if provider.is_xai_oauth() {
        return Err(XAI_OAUTH_WEB_ERROR.to_string());
    }
    Ok(())
}

fn reject_xai_oauth_web_provider_by_id(
    state: &AppState,
    app: &AppType,
    id: &str,
) -> Result<(), String> {
    if let Some(provider) = state
        .db
        .get_provider_by_id(id, app.as_str())
        .map_err(|e| e.to_string())?
    {
        reject_xai_oauth_web_provider(&provider)?;
    }
    Ok(())
}

fn restore_provider_secret_placeholders(
    state: &AppState,
    app: &AppType,
    original_id: &str,
    provider: &mut Provider,
) -> Result<(), String> {
    let Some(existing) = state
        .db
        .get_provider_by_id(original_id, app.as_str())
        .map_err(|e| e.to_string())?
    else {
        reject_unresolved_secret_placeholders(&provider.settings_config)?;
        return Ok(());
    };
    restore_secret_placeholders(
        &mut provider.settings_config,
        Some(&existing.settings_config),
    );
    reject_unresolved_secret_placeholders(&provider.settings_config)?;
    Ok(())
}

fn restore_universal_provider_secret_placeholder(
    state: &AppState,
    provider: &mut crate::provider::UniversalProvider,
) -> Result<(), String> {
    if provider.api_key != SECRET_CONFIGURED_PLACEHOLDER {
        return Ok(());
    }
    let Some(existing) = state
        .db
        .get_universal_provider(&provider.id)
        .map_err(|e| e.to_string())?
    else {
        return Err("新增统一供应商不能使用密钥占位符".to_string());
    };
    provider.api_key = existing.api_key;
    Ok(())
}

fn reject_unresolved_secret_placeholders(value: &Value) -> Result<(), String> {
    if contains_secret_placeholder(value) {
        return Err("新增配置不能使用密钥占位符，请重新填写密钥".to_string());
    }
    Ok(())
}

fn contains_secret_placeholder(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(SECRET_CONFIGURED_PLACEHOLDER),
        Value::Array(items) => items.iter().any(contains_secret_placeholder),
        Value::Object(map) => map.values().any(contains_secret_placeholder),
        _ => false,
    }
}

fn restore_secret_placeholders(incoming: &mut Value, existing: Option<&Value>) {
    match incoming {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let existing_child = existing.and_then(|value| value.get(&key));
                if let Some(value) = map.get_mut(&key) {
                    if is_secret_key(&key) && is_secret_placeholder_value(value) {
                        if let Some(existing_child) = existing_child {
                            *value = existing_child.clone();
                        }
                    } else {
                        restore_secret_placeholders(value, existing_child);
                    }
                }
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter_mut().enumerate() {
                let existing_item = existing.and_then(|value| value.get(idx));
                restore_secret_placeholders(item, existing_item);
            }
        }
        Value::String(text) => {
            if let Some(Value::String(existing_text)) = existing {
                *text = restore_embedded_secret_placeholders(text, existing_text);
            }
        }
        _ => {}
    }
}

fn is_secret_placeholder_value(value: &Value) -> bool {
    value.as_str() == Some(SECRET_CONFIGURED_PLACEHOLDER)
}

fn redact_secrets(value: &mut Value) {
    redact_secrets_with_policy(value, false);
}

fn redact_secrets_for_edit(value: &mut Value) {
    redact_secrets_with_policy(value, true);
}

fn redact_provider_collection(value: &mut Value) {
    let Value::Object(providers) = value else {
        redact_secrets(value);
        return;
    };

    // Provider IDs are arbitrary map keys and may legitimately contain words
    // such as "api-key". Only provider values participate in secret detection.
    for provider in providers.values_mut() {
        redact_secrets(provider);
    }
}

fn redact_secrets_with_policy(value: &mut Value, reveal_form_api_keys: bool) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if let Some(item) = map.get_mut(&key) {
                    if reveal_form_api_keys && is_form_api_key(&key) && item.as_str().is_some() {
                        continue;
                    }
                    if is_secret_key(&key) {
                        if secret_value_configured(item) {
                            *item = Value::String(SECRET_CONFIGURED_PLACEHOLDER.to_string());
                        }
                    } else {
                        redact_secrets_with_policy(item, reveal_form_api_keys);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_secrets_with_policy(item, reveal_form_api_keys);
            }
        }
        Value::String(text) => {
            *text = redact_embedded_config_text(text, reveal_form_api_keys);
        }
        _ => {}
    }
}

fn secret_value_configured(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => {
            let text = text.trim();
            !text.is_empty() && text != SECRET_CONFIGURED_PLACEHOLDER
        }
        Value::Array(items) => items.iter().any(secret_value_configured),
        Value::Object(map) => map.values().any(secret_value_configured),
        _ => true,
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && *ch != '.')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    normalized.contains("apikey")
        || normalized.ends_with("authtoken")
        || normalized.ends_with("accesstoken")
        || normalized.ends_with("refreshtoken")
        || normalized.ends_with("idtoken")
        || normalized.ends_with("bearertoken")
        || normalized.ends_with("password")
        || normalized.ends_with("clientsecret")
        || normalized.ends_with("secretaccesskey")
}

fn is_form_api_key(key: &str) -> bool {
    matches!(
        key,
        "apiKey"
            | "api_key"
            | "OPENAI_API_KEY"
            | "ANTHROPIC_AUTH_TOKEN"
            | "ANTHROPIC_API_KEY"
            | "GEMINI_API_KEY"
            | "GOOGLE_API_KEY"
            | "OPENROUTER_API_KEY"
            | "experimental_bearer_token"
    )
}

fn redact_embedded_config_text(text: &str, reveal_form_api_keys: bool) -> String {
    if let Ok(mut json) = serde_json::from_str::<Value>(text) {
        if json.is_object() || json.is_array() {
            redact_secrets_with_policy(&mut json, reveal_form_api_keys);
            if let Ok(serialized) = serde_json::to_string(&json) {
                return serialized;
            }
        }
    }

    if let Ok(mut document) = text.parse::<toml_edit::DocumentMut>() {
        TomlSecretRedactor {
            reveal_form_api_keys,
        }
        .visit_document_mut(&mut document);
        return document.to_string();
    }

    redact_secret_assignments(text)
}

struct TomlSecretRedactor {
    reveal_form_api_keys: bool,
}

impl VisitMut for TomlSecretRedactor {
    fn visit_table_like_kv_mut(&mut self, key: toml_edit::KeyMut<'_>, node: &mut toml_edit::Item) {
        let key_name = key.get().to_string();
        if self.reveal_form_api_keys && is_form_api_key(&key_name) && node.as_str().is_some() {
            return;
        }
        if is_secret_key(&key_name) {
            let empty = node.as_str().is_some_and(|value| value.trim().is_empty());
            if !node.is_none() && !empty {
                *node = toml_edit::value(SECRET_CONFIGURED_PLACEHOLDER);
            }
            return;
        }
        visit_table_like_kv_mut(self, key, node);
    }
}

fn redact_secret_assignments(text: &str) -> String {
    SECRET_ASSIGNMENT_RE
        .replace_all(text, |caps: &regex::Captures<'_>| {
            format!(
                "{}{}{}",
                &caps["prefix"], SECRET_CONFIGURED_PLACEHOLDER, &caps["suffix"]
            )
        })
        .into_owned()
}

fn restore_secret_assignments(incoming: &str, existing: &str) -> String {
    let mut existing_values = std::collections::HashMap::new();
    for caps in SECRET_ASSIGNMENT_RE.captures_iter(existing) {
        let Some(full) = caps.get(0) else {
            continue;
        };
        let key = full
            .as_str()
            .split([':', '='])
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        existing_values.insert(key, caps["value"].trim().to_string());
    }

    SECRET_ASSIGNMENT_RE
        .replace_all(incoming, |caps: &regex::Captures<'_>| {
            if caps["value"].trim() != SECRET_CONFIGURED_PLACEHOLDER {
                return caps[0].to_string();
            }
            let key = caps[0]
                .split([':', '='])
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let value = existing_values
                .get(&key)
                .map(String::as_str)
                .unwrap_or(SECRET_CONFIGURED_PLACEHOLDER);
            format!("{}{}{}", &caps["prefix"], value, &caps["suffix"])
        })
        .into_owned()
}

fn restore_embedded_secret_placeholders(incoming: &str, existing: &str) -> String {
    if let (Ok(mut incoming_json), Ok(existing_json)) = (
        serde_json::from_str::<Value>(incoming),
        serde_json::from_str::<Value>(existing),
    ) {
        if (incoming_json.is_object() || incoming_json.is_array())
            && (existing_json.is_object() || existing_json.is_array())
        {
            restore_secret_placeholders(&mut incoming_json, Some(&existing_json));
            if let Ok(serialized) = serde_json::to_string(&incoming_json) {
                return serialized;
            }
        }
    }

    if incoming.contains(SECRET_CONFIGURED_PLACEHOLDER) {
        if let (Ok(mut incoming_toml), Ok(existing_toml)) = (
            toml::from_str::<toml::Value>(incoming),
            toml::from_str::<toml::Value>(existing),
        ) {
            restore_toml_secret_placeholders(&mut incoming_toml, Some(&existing_toml));
            if let Ok(serialized) = toml::to_string(&incoming_toml) {
                return serialized;
            }
        }
    }

    restore_secret_assignments(incoming, existing)
}

fn restore_toml_secret_placeholders(incoming: &mut toml::Value, existing: Option<&toml::Value>) {
    match incoming {
        toml::Value::Table(table) => {
            for (key, value) in table {
                let existing_value = existing.and_then(|value| value.get(key));
                if is_secret_key(key) && value.as_str() == Some(SECRET_CONFIGURED_PLACEHOLDER) {
                    if let Some(existing_value) = existing_value {
                        *value = existing_value.clone();
                    }
                } else {
                    restore_toml_secret_placeholders(value, existing_value);
                }
            }
        }
        toml::Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                restore_toml_secret_placeholders(item, existing.and_then(|value| value.get(index)));
            }
        }
        toml::Value::String(text) => {
            if let Some(existing) = existing.and_then(toml::Value::as_str) {
                *text = restore_secret_assignments(text, existing);
            }
        }
        _ => {}
    }
}

fn get_model_pricing(state: &AppState) -> Result<Vec<crate::commands::ModelPricingInfo>, AppError> {
    state.db.ensure_model_pricing_seeded()?;

    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(crate::commands::ModelPricingInfo {
            model_id: row.get(0)?,
            display_name: row.get(1)?,
            input_cost_per_million: row.get(2)?,
            output_cost_per_million: row.get(3)?,
            cache_read_cost_per_million: row.get(4)?,
            cache_creation_cost_per_million: row.get(5)?,
        })
    })?;

    let mut pricing = Vec::new();
    for row in rows {
        pricing.push(row?);
    }
    Ok(pricing)
}

fn update_model_pricing(
    state: &AppState,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    let db = state.db.clone();
    let model_id = model_id.trim().to_string();
    let display_name = display_name.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }
    if display_name.is_empty() {
        return Err(AppError::localized(
            "usage.displayNameRequired",
            "显示名称不能为空",
            "Display name is required",
        ));
    }

    for (label, value) in [
        ("input_cost", &input_cost),
        ("output_cost", &output_cost),
        ("cache_read_cost", &cache_read_cost),
        ("cache_creation_cost", &cache_creation_cost),
    ] {
        let parsed = Decimal::from_str(value.trim()).map_err(|err| {
            AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格无效: {value} - {err}"),
                format!("{label} price is invalid: {value} - {err}"),
            )
        })?;
        if parsed < Decimal::ZERO {
            return Err(AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格必须为非负数: {value}"),
                format!("{label} price must be non-negative: {value}"),
            ));
        }
    }

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                model_id,
                display_name,
                input_cost.trim(),
                output_cost.trim(),
                cache_read_cost.trim(),
                cache_creation_cost.trim()
            ],
        )
        .map_err(|err| AppError::Database(format!("更新模型定价失败: {err}")))?;
    }

    if let Err(err) = db.backfill_missing_usage_costs_for_model(&model_id) {
        log::warn!("模型定价更新后回填历史用量成本失败 (model_id={model_id}): {err}");
    }

    Ok(())
}

fn delete_model_pricing(state: &AppState, model_id: String) -> Result<(), AppError> {
    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "DELETE FROM model_pricing WHERE model_id = ?1",
        rusqlite::params![model_id],
    )
    .map_err(|err| AppError::Database(format!("删除模型定价失败: {err}")))?;
    Ok(())
}

async fn set_auto_failover_enabled(
    state: &AppState,
    app_type: &str,
    enabled: bool,
) -> Result<(), String> {
    let mut config = state
        .db
        .get_proxy_config_for_app(app_type)
        .await
        .map_err(|e| e.to_string())?;
    if enabled && !config.enabled {
        return Err("需要先启用该应用的代理，再开启故障转移".to_string());
    }
    if enabled
        && state
            .db
            .get_failover_queue(app_type)
            .map_err(|e| e.to_string())?
            .is_empty()
    {
        let current = state
            .db
            .get_current_provider(app_type)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "故障转移队列为空，且未设置当前供应商".to_string())?;
        state
            .db
            .add_to_failover_queue(app_type, &current)
            .map_err(|e| e.to_string())?;
    }
    config.auto_failover_enabled = enabled;
    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())
}

async fn stream_check_provider(
    state: &AppState,
    app: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app.as_str())?;
    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
    let result = StreamCheckService::check_with_retry(&app, provider, &config, None).await?;
    let _ = state
        .db
        .save_stream_check_log(&provider_id, &provider.name, app.as_str(), &result);
    Ok(result)
}

async fn stream_check_all_providers(
    state: &AppState,
    app: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app.as_str())?;
    let allowed_ids = if proxy_targets_only {
        let mut ids = std::collections::HashSet::new();
        if let Some(current_id) = state.db.get_current_provider(app.as_str())? {
            ids.insert(current_id);
        }
        for item in state.db.get_failover_queue(app.as_str())? {
            ids.insert(item.provider_id);
        }
        Some(ids)
    } else {
        None
    };

    let mut results = Vec::new();
    for (id, provider) in providers {
        if allowed_ids.as_ref().is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let result = StreamCheckService::check_with_retry(&app, &provider, &config, None)
            .await
            .unwrap_or_else(|e| StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: e.to_string(),
                response_time_ms: None,
                http_status: None,
                model_used: String::new(),
                tested_at: chrono::Utc::now().timestamp(),
                retry_count: 0,
                error_category: None,
            });
        let _ = state
            .db
            .save_stream_check_log(&id, &provider.name, app.as_str(), &result);
        results.push((id, result));
    }
    Ok(results)
}

fn validate_proxy_listen(config: &ProxyConfig, production: bool) -> Result<(), String> {
    if !production {
        return Ok(());
    }
    let ip = config
        .listen_address
        .parse::<std::net::IpAddr>()
        .map_err(|e| format!("代理监听地址无效: {e}"))?;
    if !ip.is_loopback() {
        return Err("生产模式下代理监听地址必须是 loopback".to_string());
    }
    Ok(())
}

fn config_dir_for_app(app: AppType) -> Result<std::path::PathBuf, String> {
    let dir = match app {
        AppType::Claude => crate::config::get_claude_config_dir(),
        AppType::ClaudeDesktop => {
            crate::claude_desktop_config::get_config_library_path().map_err(|e| e.to_string())?
        }
        AppType::Codex => crate::codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };
    Ok(dir)
}

fn validate_provider_surface(provider: &Provider, production: bool) -> Result<(), String> {
    if !production {
        return Ok(());
    }
    validate_value_urls(&provider.settings_config)?;
    if let Some(website_url) = provider.website_url.as_deref() {
        validate_url_if_present(website_url)?;
    }
    Ok(())
}

fn validate_fetch_models_urls(
    base_url: &str,
    models_url: Option<&str>,
    production: bool,
) -> Result<(), String> {
    if !production {
        return Ok(());
    }
    validate_url_if_present(base_url)?;
    if let Some(models_url) = models_url {
        validate_url_if_present(models_url)?;
    }
    Ok(())
}

fn validate_value_urls(value: &Value) -> Result<(), String> {
    match value {
        Value::String(text) => validate_url_if_present(text),
        Value::Array(items) => {
            for item in items {
                validate_value_urls(item)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for value in map.values() {
                validate_value_urls(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_url_if_present(text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Ok(());
    }
    let url = url::Url::parse(trimmed).map_err(|e| format!("供应商 URL 无效: {e}"))?;
    if url.scheme() != "https" {
        return Err("生产模式下供应商 URL 必须使用 HTTPS".to_string());
    }
    if let Some(host) = url.host_str() {
        if is_forbidden_host(host) {
            return Err(format!("生产模式下拒绝内网或本机供应商地址: {host}"));
        }
    }
    Ok(())
}

fn is_forbidden_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_loopback()
                    || ip.is_private()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.octets() == [169, 254, 169, 254]
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.segments()[0] & 0xfe00 == 0xfc00
                    || ip.segments()[0] & 0xffc0 == 0xfe80
            }
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;

    fn temp_state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Arc::new(Database::init_at(dir.path().join("cc-switch.db")).expect("init db"));
        (dir, AppState::new(db))
    }

    #[test]
    fn grokbuild_config_dir_uses_the_existing_grok_path() {
        assert_eq!(
            config_dir_for_app(AppType::GrokBuild).expect("Grok Build config dir"),
            crate::grok_config::get_grok_config_dir()
        );
    }

    #[tokio::test]
    async fn pricing_commands_are_explicitly_available_to_web_rpc() {
        let (_dir, state) = temp_state();

        dispatch(
            &state,
            "update_model_pricing",
            json!({
                "modelId": "test-model",
                "displayName": "Test Model",
                "inputCost": "1.25",
                "outputCost": "2.50",
                "cacheReadCost": "0.10",
                "cacheCreationCost": "0.20"
            }),
            false,
        )
        .await
        .expect("update model pricing");

        let pricing = dispatch(&state, "get_model_pricing", json!({}), false)
            .await
            .expect("get model pricing");
        let rows = pricing.as_array().expect("pricing array");
        assert!(rows.iter().any(|row| {
            row.get("modelId").and_then(Value::as_str) == Some("test-model")
                && row.get("inputCostPerMillion").and_then(Value::as_str) == Some("1.25")
        }));

        dispatch(
            &state,
            "set_default_cost_multiplier",
            json!({ "appType": "claude", "value": "1.5" }),
            false,
        )
        .await
        .expect("set multiplier");
        let multiplier = dispatch(
            &state,
            "get_default_cost_multiplier",
            json!({ "appType": "claude" }),
            false,
        )
        .await
        .expect("get multiplier");
        assert_eq!(multiplier, json!("1.5"));

        dispatch(
            &state,
            "set_pricing_model_source",
            json!({ "appType": "claude", "value": "request" }),
            false,
        )
        .await
        .expect("set pricing model source");
        let source = dispatch(
            &state,
            "get_pricing_model_source",
            json!({ "appType": "claude" }),
            false,
        )
        .await
        .expect("get pricing model source");
        assert_eq!(source, json!("request"));

        dispatch(
            &state,
            "delete_model_pricing",
            json!({ "modelId": "test-model" }),
            false,
        )
        .await
        .expect("delete model pricing");
    }

    #[tokio::test]
    async fn optimizer_commands_accept_the_v317_config_shape() {
        let (_dir, state) = temp_state();
        let config = json!({
            "enabled": true,
            "thinkingOptimizer": false,
            "cacheInjection": true
        });

        let saved = dispatch(
            &state,
            "set_optimizer_config",
            json!({ "config": config }),
            false,
        )
        .await
        .expect("save v3.17 optimizer config");
        assert_eq!(saved, json!(true));

        let loaded = dispatch(&state, "get_optimizer_config", json!({}), false)
            .await
            .expect("load v3.17 optimizer config");
        assert_eq!(
            loaded,
            json!({
                "enabled": true,
                "thinkingOptimizer": false,
                "cacheInjection": true
            })
        );
    }

    #[tokio::test]
    async fn web_rpc_rejects_unknown_and_desktop_only_commands() {
        let (_dir, state) = temp_state();

        let unknown = dispatch(&state, "not_a_real_command", json!({}), false)
            .await
            .expect_err("unknown command");
        assert!(unknown.contains("未开放命令"));

        let desktop = dispatch(
            &state,
            "open_external",
            json!({ "url": "https://example.com" }),
            false,
        )
        .await
        .expect_err("desktop command");
        assert!(desktop.contains("桌面专属命令"));
    }

    #[tokio::test]
    async fn ensure_codex_official_provider_is_available_to_web_rpc() {
        let (_dir, state) = temp_state();

        let inserted = dispatch(&state, "ensure_codex_official_provider", json!({}), false)
            .await
            .expect("ensure Codex official provider");
        assert_eq!(inserted, json!(true));

        let provider = state
            .db
            .get_provider_by_id(
                crate::database::CODEX_OFFICIAL_PROVIDER_ID,
                AppType::Codex.as_str(),
            )
            .expect("query Codex official provider")
            .expect("Codex official provider exists");
        assert_eq!(provider.category.as_deref(), Some("official"));

        let repeated = dispatch(&state, "ensure_codex_official_provider", json!({}), false)
            .await
            .expect("ensure existing Codex official provider");
        assert_eq!(repeated, json!(false));
    }

    #[tokio::test]
    async fn update_toml_common_config_snippet_is_available_and_redacts_secrets() {
        let (_dir, state) = temp_state();

        let updated = dispatch(
            &state,
            "update_toml_common_config_snippet",
            json!({
                "configToml": "# keep this comment\nmodel = \"gpt-5.6\"\nexperimental_bearer_token = \"sk-web-secret\"\n",
                "snippetToml": "[tui]\nnotifications = false\n",
                "enabled": true
            }),
            false,
        )
        .await
        .expect("merge Codex common config snippet");

        let updated = updated.as_str().expect("updated TOML string");
        assert!(updated.contains("# keep this comment"));
        assert!(updated.contains("model = \"gpt-5.6\""));
        assert!(updated.contains("[tui]"));
        assert!(updated.contains("notifications = false"));
        assert!(!updated.contains("sk-web-secret"));
        assert!(updated.contains(SECRET_CONFIGURED_PLACEHOLDER));
    }

    #[tokio::test]
    async fn common_config_snippet_commands_are_available_to_web_without_live_reads() {
        let (_dir, state) = temp_state();

        dispatch(
            &state,
            "set_common_config_snippet",
            json!({
                "appType": "codex",
                "snippet": "# shared\n[tui]\nnotifications = false\nexperimental_bearer_token = \"stored-secret\"\n"
            }),
            false,
        )
        .await
        .expect("save Codex common config snippet");

        let loaded = dispatch(
            &state,
            "get_common_config_snippet",
            json!({ "appType": "codex" }),
            false,
        )
        .await
        .expect("load Codex common config snippet");
        let loaded = loaded.as_str().expect("common config string");
        assert!(loaded.contains("notifications = false"));
        assert!(loaded.contains(SECRET_CONFIGURED_PLACEHOLDER));
        assert!(!loaded.contains("stored-secret"));

        dispatch(
            &state,
            "set_common_config_snippet",
            json!({ "appType": "codex", "snippet": loaded }),
            false,
        )
        .await
        .expect("round-trip redacted common config snippet");
        let stored = state
            .db
            .get_config_snippet("codex")
            .expect("query common config snippet")
            .expect("stored common config snippet");
        assert!(stored.contains("stored-secret"));
        assert!(!stored.contains(SECRET_CONFIGURED_PLACEHOLDER));

        let extracted = dispatch(
            &state,
            "extract_common_config_snippet",
            json!({
                "appType": "codex",
                "settingsConfig": serde_json::to_string(&json!({
                    "auth": { "OPENAI_API_KEY": "provider-secret" },
                    "config": "model = \"gpt-5.6\"\n[tui]\nnotifications = true\n"
                }))
                .expect("serialize settings")
            }),
            false,
        )
        .await
        .expect("extract common config from editor settings");
        let extracted = extracted.as_str().expect("extracted common config");
        assert!(extracted.contains("[tui]"));
        assert!(extracted.contains("notifications = true"));
        assert!(!extracted.contains("provider-secret"));
        assert!(!extracted.contains("model ="));

        let err = dispatch(
            &state,
            "extract_common_config_snippet",
            json!({ "appType": "codex" }),
            false,
        )
        .await
        .expect_err("WebUI must not extract from live files");
        assert!(err.contains("settingsConfig"), "{err}");

        let err = dispatch(
            &state,
            "set_common_config_snippet",
            json!({ "appType": "codex", "snippet": "[broken" }),
            false,
        )
        .await
        .expect_err("invalid TOML must be rejected");
        assert!(err.contains("TOML") || err.contains("toml") || err.contains("格式"));

        let err = dispatch(
            &state,
            "get_common_config_snippet",
            json!({ "appType": "grokbuild" }),
            false,
        )
        .await
        .expect_err("unsupported common config app must be rejected");
        assert!(err.contains("不支持"), "{err}");
    }

    #[tokio::test]
    async fn web_managed_auth_exposes_copilot_and_codex_but_rejects_xai() {
        let (_dir, state) = temp_state();

        for provider in ["github_copilot", "codex_oauth"] {
            let status = dispatch(
                &state,
                "auth_get_status",
                json!({ "authProvider": provider }),
                false,
            )
            .await
            .expect("WebUI managed auth status");
            assert_eq!(
                status.get("provider").and_then(Value::as_str),
                Some(provider)
            );
            assert_eq!(
                status.get("authenticated").and_then(Value::as_bool),
                Some(false)
            );
            assert_eq!(
                status
                    .get("accounts")
                    .and_then(Value::as_array)
                    .map(Vec::len),
                Some(0)
            );

            let accounts = dispatch(
                &state,
                "auth_list_accounts",
                json!({ "authProvider": provider }),
                false,
            )
            .await
            .expect("WebUI managed auth accounts");
            assert_eq!(accounts.as_array().map(Vec::len), Some(0));

            dispatch(
                &state,
                "auth_logout",
                json!({ "authProvider": provider }),
                false,
            )
            .await
            .expect("WebUI managed auth logout");
        }

        for (command, args) in [
            ("auth_get_status", json!({ "authProvider": "xai_oauth" })),
            ("auth_list_accounts", json!({ "authProvider": "xai_oauth" })),
            ("auth_start_login", json!({ "authProvider": "xai_oauth" })),
            (
                "auth_poll_for_account",
                json!({
                    "authProvider": "xai_oauth",
                    "deviceCode": "not-used",
                }),
            ),
            (
                "auth_remove_account",
                json!({
                    "authProvider": "xai_oauth",
                    "accountId": "not-used",
                }),
            ),
            (
                "auth_set_default_account",
                json!({
                    "authProvider": "xai_oauth",
                    "accountId": "not-used",
                }),
            ),
            ("auth_logout", json!({ "authProvider": "xai_oauth" })),
        ] {
            let err = dispatch(&state, command, args, false)
                .await
                .expect_err("WebUI must reject xAI OAuth account management");
            assert!(
                err.contains("xAI OAuth") && err.contains("WebUI"),
                "{command}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn project_profile_commands_remain_desktop_only() {
        let (_dir, state) = temp_state();

        for command in [
            "list_profiles",
            "create_profile",
            "update_profile",
            "delete_profile",
            "apply_profile",
            "clear_current_profile",
        ] {
            let err = dispatch(&state, command, json!({}), false)
                .await
                .expect_err("Project Profiles must not be available to WebUI");
            assert!(
                err.contains("桌面专属命令"),
                "{command} should be explicitly desktop-only, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn production_rejects_private_provider_urls_before_persistence() {
        let (_dir, state) = temp_state();

        let err = dispatch(
            &state,
            "add_provider",
            json!({
                "app": "claude",
                "provider": {
                    "id": "private",
                    "name": "Private",
                    "settingsConfig": {
                        "env": {
                            "ANTHROPIC_BASE_URL": "https://127.0.0.1:8080"
                        }
                    }
                }
            }),
            true,
        )
        .await
        .expect_err("private provider url");

        assert!(err.contains("拒绝内网或本机供应商地址"));
        let providers = dispatch(&state, "get_providers", json!({ "app": "claude" }), false)
            .await
            .expect("get providers");
        assert_eq!(providers.as_object().expect("provider map").len(), 0);
    }

    #[tokio::test]
    async fn production_rejects_private_provider_website_url() {
        let (_dir, state) = temp_state();

        let err = dispatch(
            &state,
            "add_provider",
            json!({
                "app": "claude",
                "provider": {
                    "id": "private-site",
                    "name": "Private Site",
                    "settingsConfig": {
                        "env": {
                            "ANTHROPIC_BASE_URL": "https://api.example.com"
                        }
                    },
                    "websiteUrl": "https://127.0.0.1:8443"
                }
            }),
            true,
        )
        .await
        .expect_err("private website url");

        assert!(err.contains("拒绝内网或本机供应商地址"));
    }

    #[tokio::test]
    async fn production_rejects_private_fetch_models_urls() {
        let (_dir, state) = temp_state();

        let base_err = dispatch(
            &state,
            "fetch_models_for_config",
            json!({
                "baseUrl": "https://127.0.0.1:8080",
                "apiKey": "redacted-test-key"
            }),
            true,
        )
        .await
        .expect_err("private base url");
        assert!(base_err.contains("拒绝内网或本机供应商地址"));

        let override_err = dispatch(
            &state,
            "fetch_models_for_config",
            json!({
                "baseUrl": "https://api.example.com",
                "apiKey": "redacted-test-key",
                "modelsUrl": "https://169.254.169.254/latest/meta-data"
            }),
            true,
        )
        .await
        .expect_err("private models url");
        assert!(override_err.contains("拒绝内网或本机供应商地址"));
    }

    #[tokio::test]
    async fn provider_reads_redact_secrets_and_updates_preserve_placeholders() {
        let (_dir, state) = temp_state();
        let provider = Provider::with_id(
            "secure".to_string(),
            "Secure Provider".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-real-secret",
                    "ANTHROPIC_MODEL": "claude-sonnet-4-20250514"
                }
            }),
            None,
        );
        state
            .db
            .save_provider("claude", &provider)
            .expect("save provider");

        let providers = dispatch(&state, "get_providers", json!({ "app": "claude" }), false)
            .await
            .expect("get providers");
        let serialized = serde_json::to_string(&providers).expect("serialize providers");
        assert!(!serialized.contains("sk-real-secret"));
        assert!(serialized.contains(SECRET_CONFIGURED_PLACEHOLDER));

        let mut redacted_provider = providers.get("secure").cloned().expect("redacted provider");
        redacted_provider["name"] = json!("Renamed Provider");
        dispatch(
            &state,
            "update_provider",
            json!({
                "app": "claude",
                "provider": redacted_provider,
                "originalId": "secure"
            }),
            false,
        )
        .await
        .expect("update provider");

        let stored = state
            .db
            .get_provider_by_id("secure", "claude")
            .expect("load provider")
            .expect("provider exists");
        assert_eq!(
            stored
                .settings_config
                .pointer("/env/ANTHROPIC_AUTH_TOKEN")
                .and_then(Value::as_str),
            Some("sk-real-secret")
        );
    }

    #[tokio::test]
    async fn provider_ids_that_look_like_secret_fields_remain_provider_objects() {
        let (_dir, state) = temp_state();
        let provider = Provider::with_id(
            "qa-api-key".to_string(),
            "API Key ID Provider".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "sk-real-secret"
                },
                "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://api.example.com/v1\"\n"
            }),
            None,
        );
        state
            .db
            .save_provider("codex", &provider)
            .expect("save provider");

        let providers = dispatch(&state, "get_providers", json!({ "app": "codex" }), false)
            .await
            .expect("get providers");
        let listed = providers
            .get("qa-api-key")
            .expect("provider id should remain a map key");

        assert!(listed.is_object(), "provider entry must remain an object");
        assert_eq!(
            listed.get("name").and_then(Value::as_str),
            Some("API Key ID Provider")
        );
        assert_eq!(
            listed
                .pointer("/settingsConfig/auth/OPENAI_API_KEY")
                .and_then(Value::as_str),
            Some(SECRET_CONFIGURED_PLACEHOLDER)
        );
    }

    #[tokio::test]
    async fn provider_edit_detail_reveals_only_form_api_keys() {
        let (_dir, state) = temp_state();
        let provider = Provider::with_id(
            "edit-secure".to_string(),
            "Edit Secure".to_string(),
            json!({
                "apiKey": "form-camel-key",
                "api_key": "form-snake-key",
                "env": {
                    "OPENAI_API_KEY": "form-openai-key",
                    "ANTHROPIC_AUTH_TOKEN": "form-anthropic-auth",
                    "ANTHROPIC_API_KEY": "form-anthropic-key",
                    "GEMINI_API_KEY": "form-gemini-key",
                    "GOOGLE_API_KEY": "form-google-key",
                    "OPENROUTER_API_KEY": "form-openrouter-key",
                    "access_token": "oauth-access-secret",
                    "refresh_token": "oauth-refresh-secret",
                    "id_token": "oauth-id-secret",
                    "password": "password-secret",
                    "clientSecret": "client-secret",
                    "secretAccessKey": "aws-secret-access-key"
                },
                "jsonConfig": r#"{"apiKey":"json-form-key","oauth":{"access_token":"json-oauth-secret"},"nested":{"OPENROUTER_API_KEY":"json-openrouter-key","clientSecret":"json-client-secret"}}"#,
                "tomlConfig": "api_key = \"toml-form-key\"\naccess_token = \"toml-oauth-secret\"\n[nested]\nexperimental_bearer_token = \"toml-bearer-key\"\npassword = \"toml-password-secret\"\n",
                "opaqueConfig": "OPENAI_API_KEY = opaque-form-key\n"
            }),
            None,
        );
        state
            .db
            .save_provider("claude", &provider)
            .expect("save provider");

        let listed = dispatch(&state, "get_providers", json!({ "app": "claude" }), false)
            .await
            .expect("list providers");
        let listed_text = serde_json::to_string(&listed).expect("serialize provider list");
        for secret in [
            "form-camel-key",
            "form-openai-key",
            "form-anthropic-auth",
            "form-openrouter-key",
            "json-form-key",
            "toml-form-key",
            "toml-bearer-key",
            "oauth-access-secret",
            "client-secret",
        ] {
            assert!(!listed_text.contains(secret), "list leaked {secret}");
        }
        assert!(listed_text.contains(SECRET_CONFIGURED_PLACEHOLDER));

        let edit = dispatch(
            &state,
            "get_provider_for_edit",
            json!({ "app": "claude", "id": "edit-secure" }),
            false,
        )
        .await
        .expect("get provider for edit");
        let settings = edit.get("settingsConfig").expect("settingsConfig");

        for (pointer, expected) in [
            ("/apiKey", "form-camel-key"),
            ("/api_key", "form-snake-key"),
            ("/env/OPENAI_API_KEY", "form-openai-key"),
            ("/env/ANTHROPIC_AUTH_TOKEN", "form-anthropic-auth"),
            ("/env/ANTHROPIC_API_KEY", "form-anthropic-key"),
            ("/env/GEMINI_API_KEY", "form-gemini-key"),
            ("/env/GOOGLE_API_KEY", "form-google-key"),
            ("/env/OPENROUTER_API_KEY", "form-openrouter-key"),
        ] {
            assert_eq!(
                settings.pointer(pointer).and_then(Value::as_str),
                Some(expected)
            );
        }

        for pointer in [
            "/env/access_token",
            "/env/refresh_token",
            "/env/id_token",
            "/env/password",
            "/env/clientSecret",
            "/env/secretAccessKey",
        ] {
            assert_eq!(
                settings.pointer(pointer).and_then(Value::as_str),
                Some(SECRET_CONFIGURED_PLACEHOLDER),
                "sensitive field {pointer} was not redacted"
            );
        }

        let json_config: Value = serde_json::from_str(
            settings
                .get("jsonConfig")
                .and_then(Value::as_str)
                .expect("JSON config"),
        )
        .expect("structured JSON remains valid");
        assert_eq!(json_config["apiKey"], json!("json-form-key"));
        assert_eq!(
            json_config["nested"]["OPENROUTER_API_KEY"],
            json!("json-openrouter-key")
        );
        assert_eq!(
            json_config["oauth"]["access_token"],
            json!(SECRET_CONFIGURED_PLACEHOLDER)
        );
        assert_eq!(
            json_config["nested"]["clientSecret"],
            json!(SECRET_CONFIGURED_PLACEHOLDER)
        );

        let toml_config: toml::Value = toml::from_str(
            settings
                .get("tomlConfig")
                .and_then(Value::as_str)
                .expect("TOML config"),
        )
        .expect("structured TOML remains valid");
        assert_eq!(toml_config["api_key"].as_str(), Some("toml-form-key"));
        assert_eq!(
            toml_config["nested"]["experimental_bearer_token"].as_str(),
            Some("toml-bearer-key")
        );
        assert_eq!(
            toml_config["access_token"].as_str(),
            Some(SECRET_CONFIGURED_PLACEHOLDER)
        );
        assert_eq!(
            toml_config["nested"]["password"].as_str(),
            Some(SECRET_CONFIGURED_PLACEHOLDER)
        );

        let opaque = settings
            .get("opaqueConfig")
            .and_then(Value::as_str)
            .expect("opaque config");
        assert!(!opaque.contains("opaque-form-key"));
        assert!(opaque.contains(SECRET_CONFIGURED_PLACEHOLDER));

        let edit_text = serde_json::to_string(&edit).expect("serialize edit provider");
        for secret in [
            "oauth-access-secret",
            "oauth-refresh-secret",
            "oauth-id-secret",
            "password-secret",
            "client-secret",
            "aws-secret-access-key",
            "json-oauth-secret",
            "json-client-secret",
            "toml-oauth-secret",
            "toml-password-secret",
            "opaque-form-key",
        ] {
            assert!(!edit_text.contains(secret), "edit detail leaked {secret}");
        }
    }

    #[tokio::test]
    async fn universal_provider_edit_detail_is_separate_from_redacted_reads() {
        let (_dir, state) = temp_state();
        let provider = crate::provider::UniversalProvider::new(
            "universal-edit".to_string(),
            "Universal Edit".to_string(),
            "custom".to_string(),
            "https://api.example.com".to_string(),
            "universal-form-key".to_string(),
        );
        state
            .db
            .save_universal_provider(&provider)
            .expect("save universal provider");

        for command in ["get_universal_providers", "get_universal_provider"] {
            let args = if command == "get_universal_provider" {
                json!({ "id": "universal-edit" })
            } else {
                json!({})
            };
            let value = dispatch(&state, command, args, false)
                .await
                .expect("redacted universal read");
            let text = serde_json::to_string(&value).expect("serialize universal read");
            assert!(
                !text.contains("universal-form-key"),
                "{command} leaked API key"
            );
            assert!(text.contains(SECRET_CONFIGURED_PLACEHOLDER));
        }

        let edit = dispatch(
            &state,
            "get_universal_provider_for_edit",
            json!({ "id": "universal-edit" }),
            false,
        )
        .await
        .expect("get universal provider for edit");
        assert_eq!(
            edit.get("apiKey").and_then(Value::as_str),
            Some("universal-form-key")
        );

        let missing = dispatch(
            &state,
            "get_universal_provider_for_edit",
            json!({ "id": "missing" }),
            false,
        )
        .await
        .expect("missing universal provider");
        assert!(missing.is_null());
    }

    #[tokio::test]
    async fn universal_provider_ids_that_look_like_secret_fields_remain_provider_objects() {
        let (_dir, state) = temp_state();
        let provider = crate::provider::UniversalProvider::new(
            "universal-api-key".to_string(),
            "Universal API Key ID".to_string(),
            "custom".to_string(),
            "https://api.example.com".to_string(),
            "universal-real-secret".to_string(),
        );
        state
            .db
            .save_universal_provider(&provider)
            .expect("save universal provider");

        let providers = dispatch(&state, "get_universal_providers", json!({}), false)
            .await
            .expect("get universal providers");
        let listed = providers
            .get("universal-api-key")
            .expect("provider id should remain a map key");

        assert!(listed.is_object(), "provider entry must remain an object");
        assert_eq!(
            listed.get("name").and_then(Value::as_str),
            Some("Universal API Key ID")
        );
        assert_eq!(
            listed.get("apiKey").and_then(Value::as_str),
            Some(SECRET_CONFIGURED_PLACEHOLDER)
        );
    }

    #[tokio::test]
    async fn web_rpc_rejects_xai_oauth_provider_crud_and_edit_detail() {
        let (_dir, state) = temp_state();
        let mut existing = Provider::with_id(
            "xai-managed".to_string(),
            "xAI Managed".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "managed-placeholder" },
                "config": "model_provider = \"xai\"\n[model_providers.xai]\nname = \"xAI\"\nbase_url = \"https://api.x.ai/v1\"\n"
            }),
            None,
        );
        existing.category = Some("custom".to_string());
        existing.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });
        state
            .db
            .save_provider("codex", &existing)
            .expect("save managed xAI provider");
        let original = serde_json::to_value(
            state
                .db
                .get_provider_by_id("xai-managed", "codex")
                .expect("query provider")
                .expect("provider exists"),
        )
        .expect("serialize original provider");

        let mut new_managed = existing.clone();
        new_managed.id = "xai-managed-new".to_string();
        let err = dispatch(
            &state,
            "add_provider",
            json!({ "app": "codex", "provider": new_managed, "addToLive": false }),
            false,
        )
        .await
        .expect_err("WebUI must reject xAI OAuth creation");
        assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");
        assert!(state
            .db
            .get_provider_by_id("xai-managed-new", "codex")
            .expect("query new provider")
            .is_none());

        let mut overwrite_without_meta = existing.clone();
        overwrite_without_meta.name = "Attacker Replacement".to_string();
        overwrite_without_meta.meta = None;
        overwrite_without_meta.settings_config["auth"]["OPENAI_API_KEY"] = json!("attacker-key");
        let err = dispatch(
            &state,
            "add_provider",
            json!({
                "app": "codex",
                "provider": overwrite_without_meta,
                "addToLive": false
            }),
            false,
        )
        .await
        .expect_err("add UPSERT must not overwrite an existing xAI OAuth provider");
        assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");

        let mut stripped_update = existing.clone();
        stripped_update.name = "Attacker Rename".to_string();
        stripped_update.meta = None;
        stripped_update.settings_config["auth"]["OPENAI_API_KEY"] = json!("attacker-key");
        let err = dispatch(
            &state,
            "update_provider",
            json!({
                "app": "codex",
                "provider": stripped_update,
                "originalId": "xai-managed"
            }),
            false,
        )
        .await
        .expect_err("stripping metadata must not bypass xAI OAuth update protection");
        assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");

        let err = dispatch(
            &state,
            "delete_provider",
            json!({ "app": "codex", "id": "xai-managed" }),
            false,
        )
        .await
        .expect_err("WebUI must reject xAI OAuth deletion");
        assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");

        let err = dispatch(
            &state,
            "get_provider_for_edit",
            json!({ "app": "codex", "id": "xai-managed" }),
            false,
        )
        .await
        .expect_err("WebUI must reject xAI OAuth edit details");
        assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");

        for (command, args) in [
            (
                "switch_provider",
                json!({ "app": "codex", "id": "xai-managed" }),
            ),
            (
                "remove_provider_from_live_config",
                json!({ "app": "codex", "id": "xai-managed" }),
            ),
            (
                "update_providers_sort_order",
                json!({
                    "app": "codex",
                    "updates": [{ "id": "xai-managed", "sortIndex": 99 }]
                }),
            ),
        ] {
            let err = dispatch(&state, command, args, false)
                .await
                .expect_err("WebUI must reject xAI OAuth mutations");
            assert!(err.contains("xAI OAuth") && err.contains("WebUI"), "{err}");
        }

        let stored = serde_json::to_value(
            state
                .db
                .get_provider_by_id("xai-managed", "codex")
                .expect("query unchanged provider")
                .expect("managed provider remains"),
        )
        .expect("serialize stored provider");
        assert_eq!(stored, original);
    }

    #[tokio::test]
    async fn web_rpc_keeps_regular_xai_api_key_provider_editable() {
        let (_dir, state) = temp_state();
        let mut provider = Provider::with_id(
            "xai-api-key".to_string(),
            "xAI API Key".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "xai-form-key" },
                "config": "model_provider = \"xai\"\n[model_providers.xai]\nname = \"xAI\"\nbase_url = \"https://api.x.ai/v1\"\n"
            }),
            None,
        );
        provider.category = Some("custom".to_string());
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        state
            .db
            .save_provider("codex", &provider)
            .expect("save API key provider");

        let edit = dispatch(
            &state,
            "get_provider_for_edit",
            json!({ "app": "codex", "id": "xai-api-key" }),
            false,
        )
        .await
        .expect("regular xAI API key provider remains editable");
        assert_eq!(
            edit.pointer("/settingsConfig/auth/OPENAI_API_KEY")
                .and_then(Value::as_str),
            Some("xai-form-key")
        );

        let mut updated: Provider =
            serde_json::from_value(edit).expect("deserialize edit provider");
        updated.name = "xAI API Key Renamed".to_string();
        dispatch(
            &state,
            "update_provider",
            json!({ "app": "codex", "provider": updated, "originalId": "xai-api-key" }),
            false,
        )
        .await
        .expect("update regular xAI API key provider");
        assert_eq!(
            state
                .db
                .get_provider_by_id("xai-api-key", "codex")
                .expect("query API key provider")
                .expect("API key provider exists")
                .name,
            "xAI API Key Renamed"
        );

        dispatch(
            &state,
            "delete_provider",
            json!({ "app": "codex", "id": "xai-api-key" }),
            false,
        )
        .await
        .expect("delete regular xAI API key provider");
        assert!(state
            .db
            .get_provider_by_id("xai-api-key", "codex")
            .expect("query deleted provider")
            .is_none());
    }

    #[tokio::test]
    async fn codex_config_text_secrets_are_redacted_and_preserved() {
        let (_dir, state) = temp_state();
        let provider = Provider::with_id(
            "codex-secure".to_string(),
            "Codex Secure".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "sk-openai-secret"
                },
                "config": "model = \"gpt-5.5\"\nexperimental_bearer_token = \"sk-config-secret\"\n"
            }),
            None,
        );
        state
            .db
            .save_provider("codex", &provider)
            .expect("save provider");

        let providers = dispatch(&state, "get_providers", json!({ "app": "codex" }), false)
            .await
            .expect("get providers");
        let serialized = serde_json::to_string(&providers).expect("serialize providers");
        assert!(!serialized.contains("sk-openai-secret"));
        assert!(!serialized.contains("sk-config-secret"));
        assert!(serialized.contains(SECRET_CONFIGURED_PLACEHOLDER));

        let mut redacted_provider = providers
            .get("codex-secure")
            .cloned()
            .expect("redacted provider");
        redacted_provider["notes"] = json!("updated");
        dispatch(
            &state,
            "update_provider",
            json!({
                "app": "codex",
                "provider": redacted_provider,
                "originalId": "codex-secure"
            }),
            false,
        )
        .await
        .expect("update provider");

        let stored = state
            .db
            .get_provider_by_id("codex-secure", "codex")
            .expect("load provider")
            .expect("provider exists");
        let stored_text = stored
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        assert!(stored_text.contains("sk-config-secret"));
        assert_eq!(
            stored
                .settings_config
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(Value::as_str),
            Some("sk-openai-secret")
        );
    }

    #[tokio::test]
    async fn toml_secret_placeholders_restore_by_structural_path() {
        let (_dir, state) = temp_state();
        let provider = Provider::with_id(
            "toml-structural".to_string(),
            "TOML Structural".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "provider-key" },
                "config": "[first]\naccess_token = \"first-secret\"\n[second]\naccess_token = \"second-secret\"\n"
            }),
            None,
        );
        state
            .db
            .save_provider("codex", &provider)
            .expect("save provider");

        let providers = dispatch(&state, "get_providers", json!({ "app": "codex" }), false)
            .await
            .expect("get providers");
        let mut redacted = providers
            .get("toml-structural")
            .cloned()
            .expect("redacted provider");
        redacted["name"] = json!("TOML Structural Renamed");
        dispatch(
            &state,
            "update_provider",
            json!({
                "app": "codex",
                "provider": redacted,
                "originalId": "toml-structural"
            }),
            false,
        )
        .await
        .expect("update provider");

        let stored = state
            .db
            .get_provider_by_id("toml-structural", "codex")
            .expect("load provider")
            .expect("provider exists");
        let config: toml::Value = toml::from_str(
            stored
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .expect("config text"),
        )
        .expect("stored TOML");
        assert_eq!(
            config["first"]["access_token"].as_str(),
            Some("first-secret")
        );
        assert_eq!(
            config["second"]["access_token"].as_str(),
            Some("second-secret")
        );
    }
}
