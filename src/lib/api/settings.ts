import { invoke, isWebRuntime, webFetch } from "@/lib/runtime";
import type {
  Settings,
  WebDavSyncSettings,
  S3SyncSettings,
  RemoteSnapshotInfo,
} from "@/types";
import type { AppId } from "./types";

export interface ConfigTransferResult {
  success: boolean;
  message: string;
  filePath?: string;
  backupId?: string;
}

type JsonObject = Record<string, unknown>;

export interface Sub2apiProviderSelection {
  appType: AppId;
  providerId: string;
}

export interface Sub2apiExportCandidate extends Sub2apiProviderSelection {
  name: string;
  baseUrl: string;
}

export interface WebDavTestResult {
  success: boolean;
  message?: string;
}

export interface CodexUnifyHistoryRestoreResult {
  restoredJsonlFiles: number;
  restoredStateRows: number;
  /** 还原被跳过的原因（如当前目录没有账本）；存在时不应报成功 */
  skippedReason?: string;
}

export interface WebDavSyncResult {
  status: string;
}

const contentDispositionFilename = (value: string | null): string | null => {
  if (!value) return null;

  const encoded = value.match(/filename\*=UTF-8''([^;]+)/i);
  if (encoded?.[1]) {
    try {
      return decodeURIComponent(encoded[1].replace(/^"|"$/g, ""));
    } catch {
      return encoded[1].replace(/^"|"$/g, "");
    }
  }

  const plain = value.match(/filename="?([^";]+)"?/i);
  return plain?.[1] ?? null;
};

const readWebTransferBody = async (
  response: Response,
): Promise<JsonObject | string | null> => {
  const contentType = response.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    return (await response.json().catch(() => null)) as JsonObject | null;
  }
  return await response.text().catch(() => "");
};

const transferErrorMessage = (
  body: JsonObject | string | null,
  fallback: string,
) => {
  if (body && typeof body === "object" && "error" in body) {
    return String(body.error);
  }
  if (body && typeof body === "object" && "message" in body) {
    return String(body.message);
  }
  if (typeof body === "string" && body.trim()) {
    return body;
  }
  return fallback;
};

const normalizeTransferResult = (
  body: JsonObject | string | null,
): ConfigTransferResult => {
  if (body && typeof body === "object") {
    return {
      success: body.success === undefined ? true : Boolean(body.success),
      message: typeof body.message === "string" ? body.message : "",
      filePath: typeof body.filePath === "string" ? body.filePath : undefined,
      backupId: typeof body.backupId === "string" ? body.backupId : undefined,
    };
  }
  return {
    success: true,
    message: typeof body === "string" ? body : "",
  };
};

const triggerBrowserDownload = (blob: Blob, fileName: string) => {
  const objectUrl = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = objectUrl;
  link.download = fileName;
  link.rel = "noopener";
  link.style.display = "none";
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(objectUrl);
};

export const settingsApi = {
  async get(): Promise<Settings> {
    return await invoke("get_settings");
  },

  async save(settings: Settings): Promise<boolean> {
    return await invoke("save_settings", { settings });
  },

  /** 是否存在统一 Codex 会话历史的迁移备份（关闭弹窗据此显示"恢复备份"勾选） */
  async hasCodexUnifyHistoryBackup(): Promise<boolean> {
    return await invoke("has_codex_unify_history_backup");
  },

  /** 按迁移备份账本把当时迁入共享桶的官方会话还原回 openai 桶（幂等） */
  async restoreCodexUnifiedHistory(): Promise<CodexUnifyHistoryRestoreResult> {
    return await invoke("restore_codex_unified_history");
  },

  async restart(): Promise<boolean> {
    return await invoke("restart_app");
  },

  async installUpdateAndRestart(): Promise<boolean> {
    return await invoke("install_update_and_restart");
  },

  async checkUpdates(): Promise<void> {
    await invoke("check_for_updates");
  },

  async isPortable(): Promise<boolean> {
    return await invoke("is_portable_mode");
  },

  async getConfigDir(appId: AppId): Promise<string> {
    return await invoke("get_config_dir", { app: appId });
  },

  async openConfigFolder(appId: AppId): Promise<void> {
    await invoke("open_config_folder", { app: appId });
  },

  async pickDirectory(defaultPath?: string): Promise<string | null> {
    return await invoke("pick_directory", { defaultPath });
  },

  async selectConfigDirectory(defaultPath?: string): Promise<string | null> {
    return await invoke("pick_directory", { defaultPath });
  },

  async getClaudeCodeConfigPath(): Promise<string> {
    return await invoke("get_claude_code_config_path");
  },

  async getAppConfigPath(): Promise<string> {
    return await invoke("get_app_config_path");
  },

  async openAppConfigFolder(): Promise<void> {
    await invoke("open_app_config_folder");
  },

  async getAppConfigDirOverride(): Promise<string | null> {
    return await invoke("get_app_config_dir_override");
  },

  async setAppConfigDirOverride(path: string | null): Promise<boolean> {
    return await invoke("set_app_config_dir_override", { path });
  },

  async applyClaudePluginConfig(options: {
    official: boolean;
  }): Promise<boolean> {
    const { official } = options;
    return await invoke("apply_claude_plugin_config", { official });
  },

  async applyClaudeOnboardingSkip(): Promise<boolean> {
    return await invoke("apply_claude_onboarding_skip");
  },

  async clearClaudeOnboardingSkip(): Promise<boolean> {
    return await invoke("clear_claude_onboarding_skip");
  },

  async saveFileDialog(defaultName: string): Promise<string | null> {
    return await invoke("save_file_dialog", { defaultName });
  },

  async openFileDialog(): Promise<string | null> {
    return await invoke("open_file_dialog");
  },

  async saveProvidersFileDialog(defaultName: string): Promise<string | null> {
    return await invoke("save_providers_file_dialog", { defaultName });
  },

  async openProvidersFileDialog(): Promise<string | null> {
    return await invoke("open_providers_file_dialog");
  },

  async exportConfigToFile(filePath: string): Promise<ConfigTransferResult> {
    return await invoke("export_config_to_file", { filePath });
  },

  async importConfigFromFile(filePath: string): Promise<ConfigTransferResult> {
    return await invoke("import_config_from_file", { filePath });
  },

  async exportProvidersToFile(filePath: string): Promise<ConfigTransferResult> {
    return await invoke("export_providers_to_file", { filePath });
  },

  async exportProvidersSub2apiToFile(
    filePath: string,
    selectedProviders: Sub2apiProviderSelection[],
  ): Promise<ConfigTransferResult> {
    return await invoke("export_providers_sub2api_to_file", {
      filePath,
      selectedProviders,
    });
  },

  async getProvidersSub2apiExportCandidates(): Promise<
    Sub2apiExportCandidate[]
  > {
    if (isWebRuntime()) {
      const response = await webFetch(
        "/api/admin/providers/export/sub2api/candidates",
        {
          method: "GET",
        },
      );
      const body = await readWebTransferBody(response);
      if (!response.ok) {
        throw new Error(transferErrorMessage(body, `HTTP ${response.status}`));
      }
      return Array.isArray((body as JsonObject | null)?.candidates)
        ? ((body as JsonObject).candidates as Sub2apiExportCandidate[])
        : [];
    }

    const result = await invoke<{ candidates?: Sub2apiExportCandidate[] }>(
      "list_providers_sub2api_export_candidates",
    );
    return Array.isArray(result?.candidates) ? result.candidates : [];
  },

  async importProvidersFromFile(
    filePath: string,
  ): Promise<ConfigTransferResult> {
    return await invoke("import_providers_from_file", { filePath });
  },

  async downloadProvidersExport(
    defaultName: string,
  ): Promise<ConfigTransferResult> {
    const response = await webFetch("/api/admin/providers/export", {
      method: "GET",
    });
    if (!response.ok) {
      const body = await readWebTransferBody(response);
      throw new Error(transferErrorMessage(body, `HTTP ${response.status}`));
    }

    const fileName =
      contentDispositionFilename(response.headers.get("content-disposition")) ||
      defaultName;
    const blob = await response.blob();
    triggerBrowserDownload(blob, fileName);
    return {
      success: true,
      message: "",
      filePath: fileName,
    };
  },

  async downloadProvidersSub2apiExport(
    defaultName: string,
    selectedProviders: Sub2apiProviderSelection[],
  ): Promise<ConfigTransferResult> {
    const response = await webFetch("/api/admin/providers/export/sub2api", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ selectedProviders }),
    });
    if (!response.ok) {
      const body = await readWebTransferBody(response);
      throw new Error(transferErrorMessage(body, `HTTP ${response.status}`));
    }

    const fileName =
      contentDispositionFilename(response.headers.get("content-disposition")) ||
      defaultName;
    const blob = await response.blob();
    triggerBrowserDownload(blob, fileName);
    return {
      success: true,
      message: "",
      filePath: fileName,
    };
  },

  async importProvidersFromContent(
    content: string,
  ): Promise<ConfigTransferResult> {
    const response = await webFetch("/api/admin/providers/import", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: content,
    });
    const body = await readWebTransferBody(response);
    if (!response.ok) {
      throw new Error(transferErrorMessage(body, `HTTP ${response.status}`));
    }
    return normalizeTransferResult(body);
  },

  // ─── WebDAV sync ──────────────────────────────────────────

  async webdavTestConnection(
    settings: WebDavSyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return await invoke("webdav_test_connection", {
      settings,
      preserveEmptyPassword,
    });
  },

  async webdavSyncUpload(): Promise<WebDavSyncResult> {
    return await invoke("webdav_sync_upload");
  },

  async webdavSyncDownload(): Promise<WebDavSyncResult> {
    return await invoke("webdav_sync_download");
  },

  async webdavSyncSaveSettings(
    settings: WebDavSyncSettings,
    passwordTouched = false,
  ): Promise<{ success: boolean }> {
    return await invoke("webdav_sync_save_settings", {
      settings,
      passwordTouched,
    });
  },

  async webdavSyncFetchRemoteInfo(): Promise<
    RemoteSnapshotInfo | { empty: true }
  > {
    return await invoke("webdav_sync_fetch_remote_info");
  },

  // ===== S3 Sync API =====

  async s3TestConnection(
    settings: S3SyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return await invoke("s3_test_connection", {
      settings,
      preserveEmptyPassword,
    });
  },

  async s3SyncUpload(): Promise<WebDavSyncResult> {
    return await invoke("s3_sync_upload");
  },

  async s3SyncDownload(): Promise<WebDavSyncResult> {
    return await invoke("s3_sync_download");
  },

  async s3SyncSaveSettings(
    settings: S3SyncSettings,
    passwordTouched: boolean,
  ): Promise<{ success: boolean }> {
    return await invoke("s3_sync_save_settings", {
      settings,
      passwordTouched,
    });
  },

  async s3SyncFetchRemoteInfo(): Promise<RemoteSnapshotInfo | { empty: true }> {
    return await invoke("s3_sync_fetch_remote_info");
  },

  async syncCurrentProvidersLive(): Promise<void> {
    const result = (await invoke("sync_current_providers_live")) as {
      success?: boolean;
      message?: string;
    };
    if (!result?.success) {
      throw new Error(result?.message || "Sync current providers failed");
    }
  },

  async openExternal(url: string): Promise<void> {
    try {
      const u = new URL(url);
      const scheme = u.protocol.replace(":", "").toLowerCase();
      if (scheme !== "http" && scheme !== "https") {
        throw new Error("Unsupported URL scheme");
      }
    } catch {
      throw new Error("Invalid URL");
    }
    if (isWebRuntime()) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    await invoke("open_external", { url });
  },

  async setAutoLaunch(enabled: boolean): Promise<boolean> {
    return await invoke("set_auto_launch", { enabled });
  },

  async getAutoLaunchStatus(): Promise<boolean> {
    return await invoke("get_auto_launch_status");
  },

  async getToolVersions(
    tools?: string[],
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<
    Array<{
      name: string;
      version: string | null;
      latest_version: string | null;
      error: string | null;
      installed_but_broken: boolean;
      env_type: "windows" | "wsl" | "macos" | "linux" | "unknown";
      wsl_distro: string | null;
    }>
  > {
    return await invoke("get_tool_versions", { tools, wslShellByTool });
  },

  async runToolLifecycleAction(
    tools: string[],
    action: "install" | "update",
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<void> {
    await invoke("run_tool_lifecycle_action", {
      tools,
      action,
      wslShellByTool,
    });
  },

  /** 探测各工具安装分布：枚举所有安装、标记冲突、生成锚定升级命令。
   *  诊断按钮、升级前确认、升级后补诊共用此命令，各取所需字段。 */
  async probeToolInstallations(
    tools: string[],
  ): Promise<ToolInstallationReport[]> {
    return await invoke("probe_tool_installations", { tools });
  },

  async getRectifierConfig(): Promise<RectifierConfig> {
    return await invoke("get_rectifier_config");
  },

  async setRectifierConfig(config: RectifierConfig): Promise<boolean> {
    return await invoke("set_rectifier_config", { config });
  },

  async getOptimizerConfig(): Promise<OptimizerConfig> {
    return await invoke("get_optimizer_config");
  },

  async setOptimizerConfig(config: OptimizerConfig): Promise<boolean> {
    return await invoke("set_optimizer_config", { config });
  },

  async getUserAgentRewriteConfig(): Promise<UserAgentRewriteConfig> {
    return await invoke("get_user_agent_rewrite_config");
  },

  async setUserAgentRewriteConfig(
    config: UserAgentRewriteConfig,
  ): Promise<boolean> {
    return await invoke("set_user_agent_rewrite_config", { config });
  },

  async getLogConfig(): Promise<LogConfig> {
    return await invoke("get_log_config");
  },

  async setLogConfig(config: LogConfig): Promise<boolean> {
    return await invoke("set_log_config", { config });
  },
};

/** 单处工具安装的诊断信息（多处安装冲突检测）。字段对应后端 ToolInstallation。 */
export interface ToolInstallation {
  path: string;
  version: string | null;
  runnable: boolean;
  error: string | null;
  source: string;
  is_path_default: boolean;
}

/** 一次"探测工具安装分布"的结果。字段对应后端 ToolInstallationReport。 */
export interface ToolInstallationReport {
  tool: string;
  installs: ToolInstallation[];
  is_conflict: boolean;
  needs_confirmation: boolean;
  command: string;
  anchored: boolean;
}

export interface RectifierConfig {
  enabled: boolean;
  requestThinkingSignature: boolean;
  requestThinkingBudget: boolean;
  requestMediaFallback: boolean;
  requestMediaHeuristic: boolean;
}

export interface OptimizerConfig {
  enabled: boolean;
  thinkingOptimizer: boolean;
  cacheInjection: boolean;
}

export interface UserAgentRewriteRule {
  enabled?: boolean;
  pattern: string;
  [key: string]: unknown;
}

export interface UserAgentRewriteConfig {
  enabled: boolean;
  rules: UserAgentRewriteRule[];
  codexTarget: string;
}

export interface LogConfig {
  enabled: boolean;
  level: "error" | "warn" | "info" | "debug" | "trace";
}

export interface BackupEntry {
  filename: string;
  sizeBytes: number;
  createdAt: string;
}

export const backupsApi = {
  async createDbBackup(): Promise<string> {
    return await invoke("create_db_backup");
  },

  async listDbBackups(): Promise<BackupEntry[]> {
    return await invoke("list_db_backups");
  },

  async restoreDbBackup(filename: string): Promise<string> {
    return await invoke("restore_db_backup", { filename });
  },

  async renameDbBackup(oldFilename: string, newName: string): Promise<string> {
    return await invoke("rename_db_backup", { oldFilename, newName });
  },

  async deleteDbBackup(filename: string): Promise<void> {
    await invoke("delete_db_backup", { filename });
  },
};
