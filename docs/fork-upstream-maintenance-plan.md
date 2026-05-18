# CC Switch Pure Route Fork Execution Plan

> **For agentic workers:** this document is both the implementation plan and the handoff record for the `lich13/cc-switch` fork. Keep it current when changing routing, upstream-sync, or release behavior.

**Goal:** Build a customized CC Switch fork that adds a no-write Codex local routing mode, while keeping the fork able to follow `farion1231/cc-switch` through automated upstream-sync pull requests.

**Architecture:** Keep official `farion1231/cc-switch` as a read-only `upstream` remote and use the user's fork as `origin`. Implement the Codex customization as a small, isolated feature layer: a backend routing mode, a Codex-only route control, and write guards that prevent `~/.codex/auth.json` and `~/.codex/config.toml` mutation while pure routing is active.

**Tech Stack:** Tauri 2, Rust 1.85+, React 18, TypeScript, TanStack Query, pnpm, GitHub Actions, GitHub CLI.

---

## Chinese Execution Summary

这份方案按“先建可持续 fork，再做纯路由功能，最后接入上游自动同步”的顺序执行。

- 远端拓扑：把官方仓库改成只读 `upstream`，把用户自己的 fork 设为可写 `origin`。
- 纯路由边界：Codex 只有通过生成的 `codex -c` 启动命令进入 CC Switch 本地代理；这个模式不透明劫持已有 Codex 进程，也不写 `~/.codex/auth.json` 或 `~/.codex/config.toml`。
- 功能实现：新增 `local_only` 路由模式、Codex 专用按钮、运行时启动命令展示、服务层写入保护、前后端测试。
- 自动跟随官方：新增 `.github/workflows/upstream-sync.yml`，每天从 `farion1231/cc-switch` 合并到 fork 的同步分支，创建/更新 PR，并在同一 workflow 中跑 CI 等价检查。
- 发布风险：如果发布 fork 安装包，必须改 Tauri `identifier`、updater endpoint 和签名 key，否则自定义安装包可能被官方 updater 覆盖回官方版本。

## 0. Current Repo Facts

Verified on this workspace:

- Local repo: `/Users/gosu/Documents/cc-switch`.
- Current work branch: `codex/pure-local-routing-sync`.
- Current remote `origin`: `https://github.com/lich13/cc-switch.git`.
- Current remote `upstream`: `https://github.com/farion1231/cc-switch.git`.
- `upstream` push URL is disabled.
- `main` is fast-forwarded to upstream `ed33990b` (`fix-codex-mise-detection (#2822)`).
- Official latest release checked during implementation: `v3.15.0`, published `2026-05-16T03:42:43Z`.
- Existing CI: `.github/workflows/ci.yml`.
- Existing release workflow: `.github/workflows/release.yml`.
- `.gitignore` no longer ignores `.github`; fork workflows are intended to be tracked normally.
- Fork app version is `3.15.0-lich13.1`.
- Fork product identity is `CC Switch Pure Route` / `com.lich13.ccswitch`.
- Tauri updater endpoint points at `https://github.com/lich13/cc-switch/releases/latest/download/latest.json`.
- Fork-specific Tauri updater signing secrets are set in `lich13/cc-switch`: `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Apple signing/notarization secrets are not present. The release workflow packages unsigned macOS artifacts when those secrets are missing.

Stop immediately if:

- The working tree has unrelated user edits in files this plan needs to touch.
- GitHub CLI cannot create/discover the fork and no fork URL is available before remote rewiring.
- A sync merge has conflicts and the conflict list includes core custom files such as `src-tauri/src/services/proxy.rs`, `src-tauri/src/codex_config.rs`, `src/App.tsx`, or `src/components/proxy/*`.

## 1. Target Git Topology

Use this remote layout:

- `origin`: the user's customized fork, writable.
- `upstream`: official `farion1231/cc-switch`, read-only.

The local setup command sequence is:

```bash
cd /Users/gosu/Documents/cc-switch

git remote rename origin upstream
git remote set-url --push upstream DISABLED
git remote add origin "$FORK_REPO_URL"

git fetch --all --prune --tags
git push -u origin main
git branch --set-upstream-to=origin/main main

git config rerere.enabled true
git config rerere.autoupdate true
```

Before running it, create or discover the user's fork URL with GitHub CLI:

```bash
GITHUB_OWNER="$(gh api user --jq .login)"
gh repo view "$GITHUB_OWNER/cc-switch" >/dev/null 2>&1 \
  || gh repo fork farion1231/cc-switch --clone=false --remote=false
FORK_REPO_URL="$(gh repo view "$GITHUB_OWNER/cc-switch" --json sshUrl -q .sshUrl)"
test -n "$FORK_REPO_URL"
```

If GitHub CLI is not authenticated, read the fork URL interactively and reject invalid values:

```bash
printf 'Fork Git URL: '
read -r FORK_REPO_URL
case "$FORK_REPO_URL" in
  git@github.com:*|https://github.com/*) ;;
  *) echo "Invalid GitHub fork URL: $FORK_REPO_URL" >&2; exit 1 ;;
esac
```

Verification:

```bash
git remote -v
git status --short --branch
```

Expected:

- `origin` points to the user's fork.
- `upstream` fetch points to `https://github.com/farion1231/cc-switch.git`.
- `upstream` push is `DISABLED`.
- `main` tracks `origin/main`.

## 2. Branch Policy

Use these branches:

- `main`: stable customized fork.
- `codex/pure-local-routing-sync`: implementation branch for the no-write Codex routing feature and release configuration.
- `chore/sync-upstream`: recurring upstream sync branch created by GitHub Actions.

Rules:

- Prefer PRs into `main`. Direct push is acceptable only for explicit release operations after local verification.
- Upstream syncs enter through PRs.
- Prefer merge commits for upstream sync PRs. Do not squash upstream sync PRs, because preserving ancestry makes the next upstream merge easier.
- Feature work can be squash-merged if desired, but keep the final feature commit message searchable, for example `feat: add pure Codex local routing`.

## 3. Pure Codex Routing Design

### 3.1 User-Facing Behavior

Add a new Codex-only button in the provider page header:

- It starts the local proxy server.
- It marks Codex as using `local_only` routing.
- It does not modify `~/.codex/auth.json`.
- It does not modify `~/.codex/config.toml`.
- It exposes a temporary Codex launch command that points Codex to CC Switch via `-c` overrides.
- While enabled, clicking Codex provider cards switches CC Switch's internal Codex target provider without writing Codex live files.

Keep the existing proxy takeover button as a separate feature:

- Existing takeover still means file takeover.
- Existing takeover can continue to write `auth.json` and `config.toml`.
- UI copy must distinguish "file takeover" from "pure local route".

### 3.2 How Codex Requests Enter The Local Route

Pure routing cannot transparently redirect an already-running Codex CLI without changing Codex config or intercepting network traffic. The supported path is a generated launch command:

```bash
OPENAI_API_KEY=PROXY_MANAGED codex \
  -c 'model_provider="ccswitch-local"' \
  -c 'model_providers.ccswitch-local.name="CC Switch Local Route"' \
  -c 'model_providers.ccswitch-local.requires_openai_auth=true' \
  -c 'model_providers.ccswitch-local.base_url="http://127.0.0.1:15721/v1"' \
  -c 'model_providers.ccswitch-local.wire_api="responses"'
```

These `-c` flags are runtime overrides. They do not edit `~/.codex/config.toml`.

The UI should provide:

- copy command button;
- optional "open terminal" button using the existing terminal preference machinery;
- visible route base URL, for example `http://127.0.0.1:15721/v1`;
- status indicator showing current routed provider.

### 3.3 Backend Routing Mode

Add a per-app routing mode:

```text
off
file_takeover
local_only
```

Mapping:

- `off`: no routing for this app.
- `file_takeover`: current behavior, including backup and live config mutation.
- `local_only`: proxy server is active and provider switching is internal only; live Codex files are never written by this mode.

Important compatibility rule:

- `local_only` counts as routing active for provider hot-switching and failover persistence.
- `local_only` does not count as live takeover; restore and backup code must ignore it.

For migration:

- Existing `proxy_config.enabled = true` becomes `routing_mode = "file_takeover"`.
- Existing `proxy_config.enabled = false` becomes `routing_mode = "off"`.
- Keep `enabled` during the first implementation for compatibility with current code and old data.

### 3.4 No-Write Guard

When Codex `routing_mode = "local_only"`:

- `switch_provider("codex", id)` must not call the normal live-write switch path.
- provider card switch must call a proxy hot-switch path.
- `set_proxy_takeover_for_app("codex", true)` must remain file takeover and must not be called by the pure route button.
- MCP sync commands that target Codex live config must be blocked or explicitly ask the user to turn off pure routing.
- config import/export sync that writes current providers to live config must skip Codex or fail with a clear message.

The guard belongs at service/command boundaries that have access to `AppState` or `Database`. Do not make low-level `codex_config.rs` read global DB state.

## 4. File Structure

### 4.1 New Files

- `src/components/proxy/CodexRoutingControls.tsx`
  - Codex-specific pure route button and command display.
  - Based on `ClaudeDesktopRouteToggle`, but not tied to Claude Desktop behavior.

- `.github/workflows/upstream-sync.yml`
  - Scheduled and manual upstream merge workflow.
  - Creates or updates a PR from `chore/sync-upstream` into `main`.

### 4.2 Modified Files

- `src-tauri/src/database/schema.rs`
  - Add `routing_mode` column to `proxy_config`.
  - Migrate existing data.

- `src-tauri/src/database/dao/proxy.rs`
  - Read and write app routing mode.

- `src-tauri/src/proxy/types.rs`
  - Add backend `ProxyRoutingMode` and route-info DTOs.

- `src/types/proxy.ts`
  - Add frontend `ProxyRoutingMode` and route-info types.

- `src-tauri/src/services/proxy.rs`
  - Split file takeover from local-only route activation.
  - Reuse hot-switch provider behavior for local-only mode.
  - Keep Codex live write paths isolated to file takeover.
  - Transitioning from file takeover to local-only clears internal takeover state and stale backup without restoring or rewriting live files.

- `src-tauri/src/proxy/failover_switch.rs`
  - Treat Codex `local_only` as routing active when failover persists the newly selected provider.
  - Continue treating only `file_takeover` as live takeover.

- `src-tauri/src/commands/proxy.rs`
  - Add commands for routing mode and Codex route info.

- `src-tauri/src/lib.rs`
  - Register new proxy commands.

- `src/lib/api/proxy.ts`
  - Add frontend wrappers.

- `src/lib/query/proxy.ts` and `src/hooks/useProxyStatus.ts`
  - Add query/mutation hooks and cache invalidation.

- `src-tauri/src/services/provider/mod.rs`
  - If Codex local-only routing is active, switch/update/add current Codex providers internally without writing Codex live files.

- `src-tauri/src/commands/failover.rs`
- `src/components/proxy/FailoverToggle.tsx`
  - Treat Codex `local_only` as an active route for failover toggling.

- `src/App.tsx`
  - Minimal render change: show `CodexRoutingControls` for `activeApp === "codex"` next to or before existing takeover control.

- `.github/workflows/release.yml`
  - Publishes fork releases as stable GitHub releases.
  - Generates `latest.json` from fork release assets.
  - Uses fork Tauri updater signing key.
  - Skips Apple notarization checks unless Apple secrets are configured.

- `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.lock`
  - Fork version and updater identity are pinned to `3.15.0-lich13.1`.

- `src/i18n/locales/en.json`
- `src/i18n/locales/zh.json`
- `src/i18n/locales/ja.json`
  - Add precise labels for pure routing vs file takeover.

## 5. Implementation Tasks

### Task 1: Create The Fork Remote Layout

**Files:** none.

- [ ] Confirm or create the fork repository.

Run:

```bash
cd /Users/gosu/Documents/cc-switch
git status --short --branch
git remote -v
GITHUB_OWNER="$(gh api user --jq .login)"
gh repo view "$GITHUB_OWNER/cc-switch" >/dev/null 2>&1 \
  || gh repo fork farion1231/cc-switch --clone=false --remote=false
FORK_REPO_URL="$(gh repo view "$GITHUB_OWNER/cc-switch" --json sshUrl -q .sshUrl)"
test -n "$FORK_REPO_URL"
```

Expected: clean tree before remote rewiring.

- [ ] Rewire remotes.

Run:

```bash
cd /Users/gosu/Documents/cc-switch
git remote rename origin upstream
git remote set-url --push upstream DISABLED
git remote add origin "$FORK_REPO_URL"
git fetch --all --prune --tags
git push -u origin main
git branch --set-upstream-to=origin/main main
git config rerere.enabled true
git config rerere.autoupdate true
```

- [ ] Verify remotes.

Run:

```bash
git remote -v
git status --short --branch
```

Expected: `origin` is fork, `upstream` is official, tree remains clean.

### Task 2: Add Routing Mode Schema

**Files:**

- Modify: `src-tauri/src/database/schema.rs`
- Modify: `src-tauri/src/database/dao/proxy.rs`
- Modify: `src-tauri/src/proxy/types.rs`
- Modify: `src/types/proxy.ts`

- [ ] Add backend enum.

Target type:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRoutingMode {
    Off,
    FileTakeover,
    LocalOnly,
}

impl Default for ProxyRoutingMode {
    fn default() -> Self {
        Self::Off
    }
}

impl ProxyRoutingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::FileTakeover => "file_takeover",
            Self::LocalOnly => "local_only",
        }
    }
}
```

- [ ] Add `routing_mode` to `proxy_config`.

Use a default of `'off'` and migrate rows:

```sql
ALTER TABLE proxy_config ADD COLUMN routing_mode TEXT NOT NULL DEFAULT 'off';
UPDATE proxy_config
SET routing_mode = CASE
  WHEN enabled = 1 THEN 'file_takeover'
  ELSE 'off'
END
WHERE routing_mode = 'off';
```

Use the repository's existing `add_column_if_missing` style rather than raw one-off migration code if the schema module already has helpers for this.

- [ ] Add DAO methods:

```rust
pub async fn get_proxy_routing_mode_for_app(&self, app_type: &str) -> Result<ProxyRoutingMode, AppError>;
pub async fn set_proxy_routing_mode_for_app(&self, app_type: &str, mode: ProxyRoutingMode) -> Result<(), AppError>;
```

- [ ] Add frontend type:

```ts
export type ProxyRoutingMode = "off" | "file_takeover" | "local_only";
```

- [ ] Run backend schema tests.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml database::tests -- --nocapture
```

Expected: database tests pass.

### Task 3: Add Codex Local Route Backend Commands

**Files:**

- Modify: `src-tauri/src/commands/proxy.rs`
- Modify: `src-tauri/src/services/proxy.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/proxy/types.rs`

- [ ] Add route info DTO:

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLocalRouteInfo {
    pub enabled: bool,
    pub base_url: String,
    pub launch_command: String,
    pub active_provider_id: Option<String>,
    pub active_provider_name: Option<String>,
}
```

- [ ] Add service methods:

```rust
pub async fn set_routing_mode_for_app(
    &self,
    app_type: &str,
    mode: ProxyRoutingMode,
) -> Result<(), String>;

pub async fn get_routing_mode_for_app(
    &self,
    app_type: &str,
) -> Result<ProxyRoutingMode, String>;

pub async fn get_codex_local_route_info(&self) -> Result<CodexLocalRouteInfo, String>;
```

- [ ] Behavior for `set_routing_mode_for_app("codex", LocalOnly)`:

```text
1. Start proxy server if not running.
2. Set DB routing mode to local_only.
3. Do not call backup_live_config_strict.
4. Do not call takeover_live_config_strict.
5. Do not call write_codex_live.
6. Populate active target from current Codex provider if available.
```

- [ ] Behavior for `set_routing_mode_for_app("codex", Off)`:

```text
1. Set DB routing mode to off.
2. Do not restore Codex live files, because local_only never changed them.
3. If no app is in file_takeover and no other local_only route is active, stop proxy server.
```

- [ ] Keep `set_proxy_takeover_for_app` as file takeover.

When file takeover is enabled, set `routing_mode = "file_takeover"` for the app. When disabled, set `routing_mode = "off"` after restore succeeds.

- [ ] Add Tauri commands:

```rust
#[tauri::command]
pub async fn get_proxy_routing_mode_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<ProxyRoutingMode, String>;

#[tauri::command]
pub async fn set_proxy_routing_mode_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
    mode: ProxyRoutingMode,
) -> Result<(), String>;

#[tauri::command]
pub async fn get_codex_local_route_info(
    state: tauri::State<'_, AppState>,
) -> Result<CodexLocalRouteInfo, String>;
```

- [ ] Register commands in `src-tauri/src/lib.rs`.

Keep command additions adjacent to existing proxy commands to reduce future merge conflicts.

- [ ] Add backend tests.

Test names:

```rust
#[tokio::test]
async fn codex_local_only_routing_does_not_write_codex_live_files()

#[tokio::test]
async fn codex_local_only_switch_updates_current_provider_without_live_write()

#[tokio::test]
async fn codex_local_only_failover_persists_current_provider_without_live_write()

#[tokio::test]
async fn file_takeover_still_writes_codex_live_files()
```

Each no-write test must:

```text
1. Set a temp HOME or CC_SWITCH_TEST_HOME.
2. Create sentinel .codex/auth.json and .codex/config.toml.
3. Store original bytes.
4. Enable local_only route.
5. Switch Codex provider through proxy hot switch.
6. Disable local_only route.
7. Re-read both files as bytes.
8. Assert exact byte equality.
```

### Task 4: Route Provider Switching Correctly

**Files:**

- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/proxy/failover_switch.rs`
- Modify: `src/hooks/useProviderActions.ts`
- Modify: `src/lib/api/proxy.ts`
- Modify: `src/lib/query/proxy.ts`

- [ ] Backend rule:

When app is Codex and routing mode is `local_only`, `ProviderService::switch()` must not enter `switch_normal()`.

Use:

```rust
if matches!(app_type, AppType::Codex) {
    let mode = futures::executor::block_on(
        state.db.get_proxy_routing_mode_for_app("codex")
    )?;
    if mode == ProxyRoutingMode::LocalOnly {
        futures::executor::block_on(
            state.proxy_service.hot_switch_provider("codex", id)
        )
        .map_err(|e| AppError::Message(format!("热切换失败: {e}")))?;
        return Ok(SwitchResult::default());
    }
}
```

This is a safety net. The frontend should still call the dedicated proxy switch path while local-only routing is active.

- [ ] Failover rule:

`FailoverSwitchManager` currently persists failover switches only when app takeover is enabled. Update the Codex branch so `routing_mode = "local_only"` also allows persistence through `ProxyService::hot_switch_provider("codex", provider_id)`.

Do not set `proxy_config.enabled = true` for this case. `enabled` remains the compatibility flag for file takeover.

- [ ] Frontend rule:

In `useProviderActions`, when `activeApp === "codex"` and route mode is `local_only`, use:

```ts
await proxyApi.switchProxyProvider("codex", provider.id);
```

Then invalidate:

```ts
queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
queryClient.invalidateQueries({ queryKey: ["codexLocalRouteInfo"] });
```

- [ ] Test with sentinel Codex files.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml codex_local_only -- --nocapture
pnpm test:unit -- tests/hooks/useProviderActions.test.tsx
```

Expected: no-write tests pass and frontend switch behavior uses proxy API.

### Task 5: Add Codex UI Controls

**Files:**

- Create: `src/components/proxy/CodexRoutingControls.tsx`
- Modify: `src/App.tsx`
- Modify: `src/types/proxy.ts`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/ja.json`

- [ ] Create a component with this public shape:

```ts
interface CodexRoutingControlsProps {
  className?: string;
}
```

- [ ] Component behavior:

```text
1. Query Codex routing mode.
2. Query Codex local route info.
3. Toggle local_only mode through set_proxy_routing_mode_for_app.
4. Copy launch command.
5. Show clear tooltip: "Pure route: does not modify Codex auth.json or config.toml".
```

- [ ] Use lucide icons:

```ts
import { Copy, Loader2, Radio, Terminal } from "lucide-react";
```

- [ ] Keep `App.tsx` small.

Current header logic chooses `ClaudeDesktopRouteToggle` for Claude Desktop and `ProxyToggle` for other apps. Change it to:

```tsx
{activeApp === "claude-desktop" ? (
  <ClaudeDesktopRouteToggle />
) : activeApp === "codex" ? (
  <>
    <CodexRoutingControls />
    {settingsData?.enableLocalProxy && <ProxyToggle activeApp={activeApp} />}
  </>
) : (
  settingsData?.enableLocalProxy && <ProxyToggle activeApp={activeApp} />
)}
```

This keeps the file takeover button available but visually separate.

- [ ] Add i18n keys.

Recommended key namespace:

```json
{
  "codexRoute": {
    "title": "Codex pure route",
    "active": "Codex pure route active",
    "inactive": "Route Codex through CC Switch without modifying auth.json or config.toml",
    "copyCommand": "Copy launch command",
    "commandCopied": "Launch command copied",
    "stopBlocked": "Other routing modes are still using the proxy service.",
    "startFailed": "Failed to start Codex pure route: {{detail}}",
    "stopFailed": "Failed to stop Codex pure route: {{detail}}"
  }
}
```

Translate into Chinese and Japanese in the matching locale files.

- [ ] Add or update frontend tests.

Recommended test file:

```text
tests/components/CodexRoutingControls.test.tsx
```

Test cases:

```text
1. Toggle on calls set_proxy_routing_mode_for_app with appType=codex, mode=local_only.
2. Toggle off calls set_proxy_routing_mode_for_app with appType=codex, mode=off.
3. Copy button writes launch_command to clipboard.
4. Component does not call set_proxy_takeover_for_app.
```

Run:

```bash
pnpm test:unit -- tests/components/CodexRoutingControls.test.tsx
pnpm typecheck
```

### Task 6: Add Write Guards Around Codex Live Mutations

**Files:**

- Modify: `src-tauri/src/commands/import_export.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Modify: `src-tauri/src/services/mcp.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/services/proxy.rs`

- [ ] Add helper:

```rust
async fn ensure_codex_live_write_allowed(
    state: &AppState,
    operation: &str,
) -> Result<(), AppError> {
    let mode = state
        .db
        .get_proxy_routing_mode_for_app("codex")
        .await?;
    if mode == ProxyRoutingMode::LocalOnly {
        return Err(AppError::Message(format!(
            "Codex pure routing is active; refusing to write Codex live config for {operation}"
        )));
    }
    Ok(())
}
```

Place it where current service layering makes sense. If it must be shared across commands, put it in `src-tauri/src/services/codex_routing.rs`.

- [ ] Guard these operations:

```text
sync_current_providers_live
set_common_config_snippet for appType=codex if it triggers live sync
Codex MCP enable/upsert/delete live sync
normal Codex provider switch
Codex import/export live sync
```

- [ ] Keep file takeover exempt.

`set_proxy_takeover_for_app("codex", true)` is intentionally the file takeover path and should still write after it sets or transitions to `file_takeover`.

- [ ] Add tests:

```rust
#[tokio::test]
async fn codex_local_only_blocks_sync_current_provider_live()

#[tokio::test]
async fn codex_local_only_blocks_codex_mcp_live_sync()

#[tokio::test]
async fn codex_file_takeover_allows_live_write()
```

### Task 7: Add Upstream Sync Workflow

**Files:**

- Create: `.github/workflows/upstream-sync.yml`

`.gitignore` no longer ignores `.github`, so add the workflow normally:

```bash
git add .github/workflows/upstream-sync.yml
```

The checked-in workflow has this behavior:

- Runs daily at `02:17 UTC` and can also be started with `workflow_dispatch`.
- Checks out `origin/main`, fetches `farion1231/cc-switch@main` with tags, and merges it into `chore/sync-upstream`.
- If there is no upstream delta, exits without opening a PR.
- If there is a delta, force-pushes `chore/sync-upstream` and creates or updates a PR into `main`.
- Runs CI-equivalent checks in the same workflow after PR creation:
  - `pnpm install --frozen-lockfile`
  - `pnpm typecheck`
  - `pnpm format:check`
  - `pnpm test:unit`
  - `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
  - `cargo test --manifest-path src-tauri/Cargo.toml`

This workflow intentionally runs checks itself because PRs created with the default `GITHUB_TOKEN` may not trigger the normal `pull_request` CI workflow. If `UPSTREAM_SYNC_TOKEN` or `SYNC_PAT` is configured, the same workflow can use that token for checkout, push, and PR creation.

GitHub settings required:

- Optional secret `UPSTREAM_SYNC_TOKEN` or `SYNC_PAT` with repo write permissions if you want PRs created by this workflow to trigger other workflows normally.
- Actions workflow permissions: read and write.
- Allow GitHub Actions to create and approve pull requests if your repository settings expose that option.

Notes:

- GitHub's `GITHUB_TOKEN` is scoped to the repository and controlled by workflow permissions.
- GitHub documents that workflow events created by `GITHUB_TOKEN` usually do not trigger new workflow runs, except selected dispatch events. The sync workflow therefore creates or updates the PR and runs the important checks itself in the same job.

### Task 8: Add Conflict Triage Procedure

**Files:**

- Create or modify: `docs/fork-upstream-maintenance-plan.md`

When the upstream sync workflow fails due to conflicts:

- [ ] Pull the sync branch locally.

```bash
cd /Users/gosu/Documents/cc-switch
git fetch upstream --prune
git fetch origin --prune
git checkout main
git pull --ff-only origin main
git checkout -B sync/manual-upstream upstream/main
```

- [ ] Recreate the merge from fork `main`.

```bash
git checkout -B sync/resolve-upstream origin/main
git merge --no-ff upstream/main
```

- [ ] List conflicts.

```bash
git diff --name-only --diff-filter=U
```

- [ ] Resolve with this priority:

```text
1. Keep upstream changes for unrelated files.
2. Reapply local customization only in isolated files.
3. Preserve all no-write Codex tests.
4. Never drop schema migrations from either side.
5. Re-run full verification.
```

- [ ] After resolving:

```bash
pnpm install --frozen-lockfile
pnpm typecheck
pnpm format:check
pnpm test:unit
pnpm build:renderer
mkdir -p dist
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
git commit
git push -u origin sync/resolve-upstream
gh pr create --base main --head sync/resolve-upstream --title "chore(sync): resolve upstream merge"
```

### Task 9: Decide Release And App Updater Policy

**Files if shipping fork builds:**

- Modify: `src-tauri/tauri.conf.json`
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `.github/workflows/release.yml`

If the fork is only for source-level development:

- Do not change updater settings.
- Do not publish app installers from the fork.
- Use local builds only.

If the fork will be installed as a custom app:

- [ ] Change Tauri identifier from `com.ccswitch.desktop` to a fork-specific identifier, for example:

```bash
GITHUB_OWNER="$(gh api user --jq .login)"
UPDATER_ENDPOINT="https://github.com/${GITHUB_OWNER}/cc-switch/releases/latest/download/latest.json"
export UPDATER_ENDPOINT

node <<'NODE'
const fs = require("fs");
const path = "src-tauri/tauri.conf.json";
const config = JSON.parse(fs.readFileSync(path, "utf8"));

config.productName = "CC Switch Pure Route";
config.identifier = "com.local.ccswitch.pureroute";
config.plugins = config.plugins || {};
config.plugins.updater = config.plugins.updater || {};
config.plugins.updater.endpoints = [process.env.UPDATER_ENDPOINT];

fs.writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
NODE
```

- [ ] Verify the updater endpoint no longer points to official releases:

```bash
! rg -n "farion1231/cc-switch" src-tauri/tauri.conf.json
rg -n "github.com/.*/cc-switch/releases" src-tauri/tauri.conf.json
```

- [ ] Generate and use a fork-specific Tauri updater key.

Do not reuse official updater keys.

- [ ] Decide product name.

Keep `CC Switch` only if replacing official app is intended. Use a fork name if both official and custom app may be installed.

- [ ] Configure release secrets required by `.github/workflows/release.yml`.

Expected secrets include Tauri signing, Apple signing, notarization, and platform-specific release requirements.

## 6. Verification Matrix

Run this full local verification before merging feature and upstream sync PRs:

```bash
cd /Users/gosu/Documents/cc-switch
pnpm install --frozen-lockfile
pnpm typecheck
pnpm format:check
pnpm test:unit
pnpm build:renderer
mkdir -p dist
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

Manual no-write verification:

```bash
CODEX_DIR="${HOME}/.codex"
AUTH_FILE="$CODEX_DIR/auth.json"
CONFIG_FILE="$CODEX_DIR/config.toml"

shasum -a 256 "$AUTH_FILE" "$CONFIG_FILE"
# Enable Codex pure route in UI.
# Copy and run the generated Codex launch command.
# Switch Codex providers inside CC Switch.
# Stop Codex pure route.
shasum -a 256 "$AUTH_FILE" "$CONFIG_FILE"
```

Expected: the two checksum lines are identical.

Manual route verification:

```bash
curl -sS http://127.0.0.1:15721/health
```

Expected JSON contains:

```json
{"status":"healthy"}
```

Then start Codex with the generated command and confirm CC Switch proxy status shows an active Codex target.

## 7. Maintenance Rules

- Keep local custom files small and named around `codex_routing` or `CodexRoutingControls`.
- Avoid broad edits in `src/App.tsx`, `src-tauri/src/lib.rs`, and `src-tauri/src/services/proxy.rs`; these are likely upstream conflict points.
- Add tests beside the changed layer.
- Every upstream sync PR must pass CI-equivalent checks before merge.
- If an upstream release changes Codex config semantics, first inspect:

```bash
rg -n "Codex|auth\\.json|config\\.toml|model_provider|base_url|responses" src-tauri/src src tests
```

- After every upstream sync, re-run the no-write checksum verification at least once before publishing a fork build.

## 8. References

- GitHub Actions CI in this repo: `.github/workflows/ci.yml`.
- Tauri updater config in this repo: `src-tauri/tauri.conf.json`.
- GitHub documentation on `GITHUB_TOKEN`: https://docs.github.com/actions/concepts/security/github_token
- GitHub documentation on workflow-trigger behavior: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow
