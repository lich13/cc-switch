use crate::database::Database;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub(crate) struct ManagedAuthManagers {
    pub copilot: Arc<RwLock<CopilotAuthManager>>,
    pub codex_oauth: Arc<RwLock<CodexOAuthManager>>,
}

impl ManagedAuthManagers {
    pub(crate) fn from_database(db: &Database) -> Self {
        Self::new(managed_auth_data_dir(db))
    }

    pub(crate) fn new(data_dir: PathBuf) -> Self {
        Self {
            copilot: Arc::new(RwLock::new(CopilotAuthManager::new(data_dir.clone()))),
            codex_oauth: Arc::new(RwLock::new(CodexOAuthManager::new(data_dir))),
        }
    }
}

fn managed_auth_data_dir(db: &Database) -> PathBuf {
    match db.db_path.as_deref() {
        Some(db_path) => managed_auth_data_dir_for_path(db_path),
        None => {
            static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
            std::env::temp_dir().join(format!(
                "cc-switch-managed-auth-{}-{}",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ))
        }
    }
}

fn managed_auth_data_dir_for_path(db_path: &Path) -> PathBuf {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    if parent.file_name().and_then(|name| name.to_str()) == Some(".cc-switch") {
        parent.to_path_buf()
    } else {
        parent.join(".cc-switch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_database_reuses_dot_cc_switch_parent() {
        assert_eq!(
            managed_auth_data_dir_for_path(Path::new("/home/user/.cc-switch/cc-switch.db")),
            PathBuf::from("/home/user/.cc-switch")
        );
    }

    #[test]
    fn webd_database_uses_private_dot_cc_switch_child() {
        assert_eq!(
            managed_auth_data_dir_for_path(Path::new("/var/lib/cc-switch-webd/cc-switch.db")),
            PathBuf::from("/var/lib/cc-switch-webd/.cc-switch")
        );
    }
}
