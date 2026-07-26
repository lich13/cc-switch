//! Provider-only JSON export/import.
//!
//! This intentionally uses a structured JSON envelope instead of SQL so the
//! WebUI import path never executes user-uploaded SQL.

use super::{lock_conn, to_json_string, Database};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, UniversalProvider};
use chrono::{Local, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const PROVIDERS_EXPORT_FORMAT: &str = "cc-switch-providers-export";
const PROVIDERS_EXPORT_VERSION: u32 = 1;
const UNIVERSAL_PROVIDERS_KEY: &str = "universal_providers";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvidersExportEnvelope {
    format: String,
    version: u32,
    exported_at: String,
    providers: Vec<ProviderExportRow>,
    provider_endpoints: Vec<ProviderEndpointExportRow>,
    universal_providers: HashMap<String, UniversalProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderExportRow {
    id: String,
    app_type: String,
    name: String,
    settings_config: serde_json::Value,
    website_url: Option<String>,
    category: Option<String>,
    created_at: Option<i64>,
    sort_index: Option<i64>,
    notes: Option<String>,
    icon: Option<String>,
    icon_color: Option<String>,
    meta: serde_json::Value,
    is_current: bool,
    in_failover_queue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderEndpointExportRow {
    provider_id: String,
    app_type: String,
    url: String,
    added_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct Sub2apiExportEnvelope {
    exported_at: String,
    proxies: Vec<serde_json::Value>,
    accounts: Vec<Sub2apiAccount>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sub2apiProviderSelection {
    pub app_type: String,
    pub provider_id: String,
}

impl Sub2apiProviderSelection {
    pub fn new(app_type: impl Into<String>, provider_id: impl Into<String>) -> Self {
        Self {
            app_type: app_type.into(),
            provider_id: provider_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sub2apiExportCandidate {
    pub app_type: String,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
}

struct Sub2apiExportableProvider {
    selection: Sub2apiProviderSelection,
    candidate: Sub2apiExportCandidate,
    account: Sub2apiAccount,
}

#[derive(Debug, Clone, Serialize)]
struct Sub2apiAccount {
    name: String,
    platform: &'static str,
    #[serde(rename = "type")]
    account_type: &'static str,
    credentials: Sub2apiCredentials,
    extra: Sub2apiExtra,
    concurrency: u32,
    priority: u32,
    rate_multiplier: u32,
    auto_pause_on_expired: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Sub2apiCredentials {
    api_key: String,
    base_url: String,
    pool_mode: bool,
    pool_mode_retry_count: u32,
}

#[derive(Debug, Clone, Serialize)]
struct Sub2apiExtra {
    openai_apikey_responses_websockets_v2_enabled: bool,
    openai_apikey_responses_websockets_v2_mode: &'static str,
    openai_passthrough: bool,
    openai_responses_supported: bool,
}

impl Sub2apiAccount {
    fn new(name: String, api_key: String, base_url: String) -> Self {
        Self {
            name,
            platform: "openai",
            account_type: "apikey",
            credentials: Sub2apiCredentials {
                api_key,
                base_url,
                pool_mode: true,
                pool_mode_retry_count: 3,
            },
            extra: Sub2apiExtra {
                openai_apikey_responses_websockets_v2_enabled: false,
                openai_apikey_responses_websockets_v2_mode: "off",
                openai_passthrough: true,
                openai_responses_supported: true,
            },
            concurrency: 10,
            priority: 2,
            rate_multiplier: 1,
            auto_pause_on_expired: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersImportSummary {
    pub backup_id: String,
    pub provider_count: usize,
    pub provider_endpoint_count: usize,
    pub universal_provider_count: usize,
}

impl Database {
    pub fn export_providers_json(&self, target_path: &Path) -> Result<(), AppError> {
        let json = self.export_providers_json_string()?;
        crate::config::atomic_write(target_path, json.as_bytes())
    }

    pub fn export_providers_sub2api_json(&self, target_path: &Path) -> Result<(), AppError> {
        let json = self.export_providers_sub2api_json_string()?;
        crate::config::atomic_write(target_path, json.as_bytes())
    }

    pub fn export_providers_sub2api_json_for_selection(
        &self,
        target_path: &Path,
        selected_providers: &[Sub2apiProviderSelection],
    ) -> Result<(), AppError> {
        let json = self.export_providers_sub2api_json_string_for_selection(selected_providers)?;
        crate::config::atomic_write(target_path, json.as_bytes())
    }

    pub fn import_providers_json(
        &self,
        source_path: &Path,
    ) -> Result<ProvidersImportSummary, AppError> {
        if !source_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "供应商导入文件不存在: {}",
                source_path.display()
            )));
        }
        let raw = fs::read_to_string(source_path).map_err(|e| AppError::io(source_path, e))?;
        self.import_providers_json_string(&raw)
    }

    pub fn export_providers_json_string(&self) -> Result<String, AppError> {
        let conn = lock_conn!(self.conn);
        let envelope = ProvidersExportEnvelope {
            format: PROVIDERS_EXPORT_FORMAT.to_string(),
            version: PROVIDERS_EXPORT_VERSION,
            exported_at: Utc::now().to_rfc3339(),
            providers: Self::load_provider_export_rows(&conn)?,
            provider_endpoints: Self::load_provider_endpoint_export_rows(&conn)?,
            universal_providers: Self::load_universal_provider_export_map(&conn)?,
        };
        serde_json::to_string_pretty(&envelope).map_err(|source| AppError::JsonSerialize { source })
    }

    pub fn export_providers_sub2api_json_string(&self) -> Result<String, AppError> {
        let accounts = self
            .load_sub2api_exportable_providers()?
            .into_iter()
            .map(|entry| entry.account)
            .collect();

        Self::serialize_sub2api_export_envelope(accounts)
    }

    pub fn export_providers_sub2api_json_string_for_selection(
        &self,
        selected_providers: &[Sub2apiProviderSelection],
    ) -> Result<String, AppError> {
        if selected_providers.is_empty() {
            return Err(AppError::InvalidInput(
                "sub2api export empty selection".to_string(),
            ));
        }

        let selected_keys: HashSet<Sub2apiProviderSelection> =
            selected_providers.iter().cloned().collect();
        let mut matched_keys = HashSet::new();
        let accounts = self
            .load_sub2api_exportable_providers()?
            .into_iter()
            .filter_map(|entry| {
                if selected_keys.contains(&entry.selection) {
                    matched_keys.insert(entry.selection);
                    Some(entry.account)
                } else {
                    None
                }
            })
            .collect();

        if matched_keys.len() != selected_keys.len() {
            if let Some(selection) = selected_keys.difference(&matched_keys).next() {
                return Err(self.sub2api_selection_error(selection));
            }
        }

        Self::serialize_sub2api_export_envelope(accounts)
    }

    pub fn list_sub2api_export_candidates(&self) -> Result<Vec<Sub2apiExportCandidate>, AppError> {
        Ok(self
            .load_sub2api_exportable_providers()?
            .into_iter()
            .map(|entry| entry.candidate)
            .collect())
    }

    fn serialize_sub2api_export_envelope(
        accounts: Vec<Sub2apiAccount>,
    ) -> Result<String, AppError> {
        let envelope = Sub2apiExportEnvelope {
            exported_at: Utc::now().to_rfc3339(),
            proxies: Vec::new(),
            accounts,
        };
        serde_json::to_string_pretty(&envelope).map_err(|source| AppError::JsonSerialize { source })
    }

    fn load_sub2api_exportable_providers(
        &self,
    ) -> Result<Vec<Sub2apiExportableProvider>, AppError> {
        let mut exportable = Vec::new();

        for app_type in AppType::all() {
            let providers = self.get_all_providers(app_type.as_str())?;
            for provider in providers.values() {
                if let Some(entry) = Self::sub2api_exportable_provider(&app_type, provider) {
                    exportable.push(entry);
                }
            }
        }

        Ok(exportable)
    }

    fn sub2api_exportable_provider(
        app_type: &AppType,
        provider: &Provider,
    ) -> Option<Sub2apiExportableProvider> {
        let (base_url, api_key) = provider.resolve_usage_credentials(app_type);
        let api_key = api_key.trim().to_string();
        let base_url = strip_sub2api_base_url(&base_url);
        if api_key.is_empty() || base_url.is_empty() {
            return None;
        }

        let selection = Sub2apiProviderSelection::new(app_type.as_str(), provider.id.clone());
        let candidate = Sub2apiExportCandidate {
            app_type: selection.app_type.clone(),
            provider_id: selection.provider_id.clone(),
            name: provider.name.clone(),
            base_url: base_url.clone(),
        };
        let account = Sub2apiAccount::new(provider.name.clone(), api_key, base_url);

        Some(Sub2apiExportableProvider {
            selection,
            candidate,
            account,
        })
    }

    fn sub2api_selection_error(&self, selection: &Sub2apiProviderSelection) -> AppError {
        let app_type = match AppType::from_str(&selection.app_type) {
            Ok(app_type) => app_type,
            Err(_) => {
                return AppError::InvalidInput(format!(
                    "sub2api selected provider not found: {}/{}",
                    selection.app_type, selection.provider_id
                ));
            }
        };

        match self.get_all_providers(app_type.as_str()) {
            Ok(providers) if providers.contains_key(&selection.provider_id) => {
                AppError::InvalidInput(format!(
                    "sub2api selected provider is not exportable: {}/{}",
                    selection.app_type, selection.provider_id
                ))
            }
            Ok(_) => AppError::InvalidInput(format!(
                "sub2api selected provider not found: {}/{}",
                selection.app_type, selection.provider_id
            )),
            Err(err) => err,
        }
    }

    pub fn import_providers_json_string(
        &self,
        raw: &str,
    ) -> Result<ProvidersImportSummary, AppError> {
        let envelope = Self::parse_providers_export_envelope(raw)?;
        let backup_id = self.backup_current_database_file()?;

        let provider_count = envelope.providers.len();
        let provider_endpoint_count = envelope.provider_endpoints.len();
        let universal_provider_count = envelope.universal_providers.len();

        {
            let mut conn = lock_conn!(self.conn);
            let tx = conn
                .transaction()
                .map_err(|e| AppError::Database(format!("开始供应商导入事务失败: {e}")))?;

            tx.execute("DELETE FROM provider_health", [])
                .map_err(|e| AppError::Database(format!("清理 provider_health 失败: {e}")))?;
            tx.execute("DELETE FROM provider_endpoints", [])
                .map_err(|e| AppError::Database(format!("清理 provider_endpoints 失败: {e}")))?;
            tx.execute("DELETE FROM providers", [])
                .map_err(|e| AppError::Database(format!("清理 providers 失败: {e}")))?;
            tx.execute(
                "DELETE FROM settings WHERE key = ?1",
                params![UNIVERSAL_PROVIDERS_KEY],
            )
            .map_err(|e| AppError::Database(format!("清理 universal_providers 失败: {e}")))?;

            for provider in &envelope.providers {
                Self::insert_provider_export_row(&tx, provider)?;
            }
            let mut endpoint_keys = HashSet::new();
            for endpoint in &envelope.provider_endpoints {
                let key = (
                    endpoint.provider_id.as_str(),
                    endpoint.app_type.as_str(),
                    endpoint.url.as_str(),
                );
                if !endpoint_keys.insert(key) {
                    return Err(AppError::InvalidInput(format!(
                        "供应商导入文件格式无效: duplicate provider endpoint {}/{}/{}",
                        endpoint.app_type, endpoint.provider_id, endpoint.url
                    )));
                }
                Self::insert_provider_endpoint_export_row(&tx, endpoint)?;
            }
            if !envelope.universal_providers.is_empty() {
                let json = to_json_string(&envelope.universal_providers)?;
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                    params![UNIVERSAL_PROVIDERS_KEY, json],
                )
                .map_err(|e| AppError::Database(format!("写入 universal_providers 失败: {e}")))?;
            }

            tx.commit()
                .map_err(|e| AppError::Database(format!("提交供应商导入事务失败: {e}")))?;
        }

        Ok(ProvidersImportSummary {
            backup_id,
            provider_count,
            provider_endpoint_count,
            universal_provider_count,
        })
    }

    fn parse_providers_export_envelope(raw: &str) -> Result<ProvidersExportEnvelope, AppError> {
        let content = raw.trim_start_matches('\u{feff}');
        let envelope: ProvidersExportEnvelope = serde_json::from_str(content)
            .map_err(|e| AppError::InvalidInput(format!("供应商导入文件格式无效: {e}")))?;
        if envelope.format != PROVIDERS_EXPORT_FORMAT
            || envelope.version != PROVIDERS_EXPORT_VERSION
        {
            return Err(AppError::InvalidInput(
                "供应商导入文件格式无效: unsupported provider export envelope".to_string(),
            ));
        }
        Self::validate_providers_export_envelope(&envelope)?;
        Ok(envelope)
    }

    fn validate_providers_export_envelope(
        envelope: &ProvidersExportEnvelope,
    ) -> Result<(), AppError> {
        for provider in &envelope.providers {
            if provider.id.trim().is_empty() || provider.app_type.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "供应商导入文件格式无效: provider id/appType 不能为空".to_string(),
                ));
            }
            if provider.name.trim().is_empty() {
                return Err(AppError::InvalidInput(format!(
                    "供应商导入文件格式无效: provider {} name 不能为空",
                    provider.id
                )));
            }
            if !provider.settings_config.is_object() {
                return Err(AppError::InvalidInput(format!(
                    "供应商导入文件格式无效: provider {} settingsConfig 必须是对象",
                    provider.id
                )));
            }
            if !provider.meta.is_object() {
                return Err(AppError::InvalidInput(format!(
                    "供应商导入文件格式无效: provider {} meta 必须是对象",
                    provider.id
                )));
            }
        }
        for endpoint in &envelope.provider_endpoints {
            if endpoint.provider_id.trim().is_empty()
                || endpoint.app_type.trim().is_empty()
                || endpoint.url.trim().is_empty()
            {
                return Err(AppError::InvalidInput(
                    "供应商导入文件格式无效: endpoint providerId/appType/url 不能为空".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn load_provider_export_rows(conn: &Connection) -> Result<Vec<ProviderExportRow>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT id, app_type, name, settings_config, website_url, category,
                        created_at, sort_index, notes, icon, icon_color, meta,
                        is_current, in_failover_queue
                 FROM providers
                 ORDER BY app_type ASC, COALESCE(sort_index, 999999), created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let settings_config_raw: String = row.get(3)?;
                let meta_raw: String = row.get(11)?;
                let settings_config =
                    serde_json::from_str(&settings_config_raw).unwrap_or(serde_json::Value::Null);
                let meta =
                    serde_json::from_str(&meta_raw).unwrap_or_else(|_| serde_json::json!({}));
                Ok(ProviderExportRow {
                    id: row.get(0)?,
                    app_type: row.get(1)?,
                    name: row.get(2)?,
                    settings_config,
                    website_url: row.get(4)?,
                    category: row.get(5)?,
                    created_at: row.get(6)?,
                    sort_index: row.get(7)?,
                    notes: row.get(8)?,
                    icon: row.get(9)?,
                    icon_color: row.get(10)?,
                    meta,
                    is_current: row.get(12)?,
                    in_failover_queue: row.get(13)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = Vec::new();
        for row in rows {
            providers.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(providers)
    }

    fn load_provider_endpoint_export_rows(
        conn: &Connection,
    ) -> Result<Vec<ProviderEndpointExportRow>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, app_type, url, added_at
                 FROM provider_endpoints
                 ORDER BY app_type ASC, provider_id ASC, added_at ASC, url ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProviderEndpointExportRow {
                    provider_id: row.get(0)?,
                    app_type: row.get(1)?,
                    url: row.get(2)?,
                    added_at: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut endpoints = Vec::new();
        for row in rows {
            endpoints.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(endpoints)
    }

    fn load_universal_provider_export_map(
        conn: &Connection,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![UNIVERSAL_PROVIDERS_KEY],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        match raw {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Database(format!("解析 universal_providers 失败: {e}"))),
            None => Ok(HashMap::new()),
        }
    }

    fn insert_provider_export_row(
        conn: &Connection,
        provider: &ProviderExportRow,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO providers (
                id, app_type, name, settings_config, website_url, category,
                created_at, sort_index, notes, icon, icon_color, meta,
                is_current, in_failover_queue
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                provider.id,
                provider.app_type,
                provider.name,
                to_json_string(&provider.settings_config)?,
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                to_json_string(&provider.meta)?,
                provider.is_current,
                provider.in_failover_queue,
            ],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "导入供应商失败: provider={}/{}: {e}",
                provider.app_type, provider.id
            ))
        })?;
        Ok(())
    }

    fn insert_provider_endpoint_export_row(
        conn: &Connection,
        endpoint: &ProviderEndpointExportRow,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                endpoint.provider_id,
                endpoint.app_type,
                endpoint.url,
                endpoint.added_at,
            ],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "导入供应商 endpoint 失败: provider={}/{} url={}: {e}",
                endpoint.app_type, endpoint.provider_id, endpoint.url
            ))
        })?;
        Ok(())
    }

    fn backup_current_database_file(&self) -> Result<String, AppError> {
        let Some(db_path) = self.db_path.as_ref() else {
            return Ok(String::new());
        };
        if !db_path.exists() {
            return Ok(String::new());
        }

        let backup_path = self.backup_database_file_at(db_path)?;
        Ok(backup_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default())
    }

    fn backup_database_file_at(&self, db_path: &Path) -> Result<PathBuf, AppError> {
        let backup_dir = db_path
            .parent()
            .ok_or_else(|| AppError::Config("无效的数据库路径".to_string()))?
            .join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

        let base_id = format!("db_backup_{}", Local::now().format("%Y%m%d_%H%M%S"));
        let mut backup_id = base_id.clone();
        let mut backup_path = backup_dir.join(format!("{backup_id}.db"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_id = format!("{base_id}_{counter}");
            backup_path = backup_dir.join(format!("{backup_id}.db"));
            counter += 1;
        }

        {
            let conn = lock_conn!(self.conn);
            let mut dest_conn =
                Connection::open(&backup_path).map_err(|e| AppError::Database(e.to_string()))?;
            let backup = rusqlite::backup::Backup::new(&conn, &mut dest_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(backup_path)
    }
}

fn strip_sub2api_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return trimmed;
    }

    if let Ok(mut parsed) = url::Url::parse(&trimmed) {
        let path = parsed.path().trim_end_matches('/').to_string();
        if path == "/v1" {
            parsed.set_path("");
            return parsed.as_str().trim_end_matches('/').to_string();
        }
        if let Some(prefix) = path.strip_suffix("/v1") {
            parsed.set_path(prefix);
            return parsed.as_str().trim_end_matches('/').to_string();
        }
    }

    trimmed.strip_suffix("/v1").unwrap_or(&trimmed).to_string()
}
