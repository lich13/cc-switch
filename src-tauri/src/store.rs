use crate::database::Database;
use crate::managed_auth::ManagedAuthManagers;
use crate::services::{ProxyService, UsageCache};
use std::sync::Arc;

/// 全局应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    pub(crate) managed_auth: ManagedAuthManagers,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let managed_auth = ManagedAuthManagers::from_database(&db);
        let proxy_service = ProxyService::new_with_managed_auth(db.clone(), managed_auth.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            managed_auth,
        }
    }
}
