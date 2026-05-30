use crate::error::AppError;
#[cfg(not(target_os = "macos"))]
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static AUTO_LAUNCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(any(target_os = "macos", test))]
const LEGACY_AUTO_LAUNCH_NAMES: &[&str] = &["CC Switch"];

#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCH_AGENT_LABEL: &str = "com.lich13.cc-switch-pure-route";

#[cfg(any(target_os = "macos", test))]
pub(crate) const MACOS_LAUNCH_AGENT_STARTUP_ARG: &str = "--cc-switch-launch-agent-startup";

/// 获取 macOS 上的 .app bundle 路径
/// 将 `/path/to/CC Switch.app/Contents/MacOS/CC Switch` 转换为 `/path/to/CC Switch.app`
#[cfg(any(target_os = "macos", test))]
fn get_macos_app_bundle_path(exe_path: &Path) -> Option<PathBuf> {
    let path_str = exe_path.to_string_lossy();
    // 查找 .app/Contents/MacOS/ 模式
    if let Some(app_pos) = path_str.find(".app/Contents/MacOS/") {
        let app_bundle_end = app_pos + 4; // ".app" 的结束位置
        Some(PathBuf::from(&path_str[..app_bundle_end]))
    } else {
        None
    }
}

fn get_current_auto_launch_path() -> Result<PathBuf, AppError> {
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;
    // macOS 需要使用 .app bundle 路径；LaunchAgent 对 .app 通过 /usr/bin/open 启动。
    #[cfg(target_os = "macos")]
    let app_path = get_macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(not(target_os = "macos"))]
    let app_path = exe_path;

    Ok(app_path)
}

fn app_name_for_auto_launch(app_path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        if app_path.extension().and_then(|ext| ext.to_str()) == Some("app") {
            return app_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("CC Switch Pure Route")
                .to_string();
        }
    }

    app_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("CC Switch Pure Route")
        .to_string()
}

#[cfg(any(target_os = "macos", test))]
fn auto_launch_names_to_remove(current_name: &str) -> Vec<String> {
    let mut names = vec![current_name.to_string()];
    for name in LEGACY_AUTO_LAUNCH_NAMES {
        if !names.iter().any(|existing| existing == name) {
            names.push((*name).to_string());
        }
    }
    names
}

#[cfg(not(target_os = "macos"))]
fn build_auto_launch(app_path: &Path) -> Result<AutoLaunch, AppError> {
    let app_name = app_name_for_auto_launch(app_path);

    // Windows/Linux: 使用 auto-launch 的注册表/XDG autostart 实现。
    // macOS 自启由本文件的 LaunchAgent writer 单独处理，避免启动期 System Events。
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(&app_name)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, AppError> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| AppError::Message(format!("执行 osascript 失败: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Message(format!(
            "osascript 执行失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn delete_login_item_if_exists(name: &str) -> Result<(), AppError> {
    let quoted = applescript_string(name);
    let script = format!(
        r#"tell application "System Events"
    if exists login item {quoted} then delete login item {quoted}
end tell"#
    );
    run_osascript(&script).map(|_| ())
}

#[cfg(target_os = "macos")]
fn remove_current_and_legacy_login_items(current_name: &str) -> Result<(), AppError> {
    for name in auto_launch_names_to_remove(current_name) {
        delete_login_item_if_exists(&name)?;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_label() -> &'static str {
    MACOS_LAUNCH_AGENT_LABEL
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_dir() -> Result<PathBuf, AppError> {
    let home =
        dirs::home_dir().ok_or_else(|| AppError::Message("无法获取用户 home 目录".to_string()))?;
    Ok(home.join("Library").join("LaunchAgents"))
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_file() -> Result<PathBuf, AppError> {
    Ok(macos_launch_agent_dir()?.join(format!("{}.plist", macos_launch_agent_label())))
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_program_arguments(app_path: &Path) -> Vec<String> {
    if app_path.extension().and_then(|ext| ext.to_str()) == Some("app") {
        return vec![
            "/usr/bin/open".to_string(),
            "-g".to_string(),
            app_path.to_string_lossy().to_string(),
            "--args".to_string(),
            MACOS_LAUNCH_AGENT_STARTUP_ARG.to_string(),
        ];
    }

    vec![
        app_path.to_string_lossy().to_string(),
        MACOS_LAUNCH_AGENT_STARTUP_ARG.to_string(),
    ]
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn is_macos_launch_agent_startup_arg(value: &str) -> bool {
    value == MACOS_LAUNCH_AGENT_STARTUP_ARG
}

#[cfg(any(target_os = "macos", test))]
fn plist_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_plist(app_path: &Path) -> String {
    let args = macos_launch_agent_program_arguments(app_path)
        .into_iter()
        .map(|arg| format!("        <string>{}</string>", plist_escape(&arg)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
{}
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#,
        plist_escape(macos_launch_agent_label()),
        args
    )
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_matches_app(app_path: &Path) -> Result<bool, AppError> {
    let file = macos_launch_agent_file()?;
    let expected = macos_launch_agent_plist(app_path);

    match std::fs::read_to_string(&file) {
        Ok(existing) => Ok(existing == expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::Message(format!(
            "读取 macOS LaunchAgent 失败: {}: {error}",
            file.display()
        ))),
    }
}

#[cfg(target_os = "macos")]
fn ensure_macos_launch_agent_for_app(app_path: &Path) -> Result<(), AppError> {
    if macos_launch_agent_matches_app(app_path)? {
        return Ok(());
    }

    let dir = macos_launch_agent_dir()?;
    std::fs::create_dir_all(&dir).map_err(|error| {
        AppError::Message(format!(
            "创建 macOS LaunchAgent 目录失败: {}: {error}",
            dir.display()
        ))
    })?;

    let file = macos_launch_agent_file()?;
    let temp_file = file.with_extension("plist.tmp");
    std::fs::write(&temp_file, macos_launch_agent_plist(app_path)).map_err(|error| {
        AppError::Message(format!(
            "写入 macOS LaunchAgent 失败: {}: {error}",
            temp_file.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&temp_file, std::fs::Permissions::from_mode(0o644)).map_err(
            |error| {
                AppError::Message(format!(
                    "设置 macOS LaunchAgent 权限失败: {}: {error}",
                    temp_file.display()
                ))
            },
        )?;
    }

    std::fs::rename(&temp_file, &file).map_err(|error| {
        let _ = std::fs::remove_file(&temp_file);
        AppError::Message(format!(
            "替换 macOS LaunchAgent 失败: {} -> {}: {error}",
            temp_file.display(),
            file.display()
        ))
    })?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_macos_launch_agent() -> Result<(), AppError> {
    let file = macos_launch_agent_file()?;

    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Message(format!(
            "删除 macOS LaunchAgent 失败: {}: {error}",
            file.display()
        ))),
    }
}

fn with_auto_launch_lock<T>(
    operation: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = AUTO_LAUNCH_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| AppError::Message("auto-launch lock poisoned".to_string()))?;
    operation()
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    with_auto_launch_lock(enable_auto_launch_unlocked)
}

fn enable_auto_launch_unlocked() -> Result<(), AppError> {
    let app_path = get_current_auto_launch_path()?;

    #[cfg(target_os = "macos")]
    {
        ensure_macos_launch_agent_for_app(&app_path)?;

        let app_name = app_name_for_auto_launch(&app_path);
        if let Err(error) = remove_current_and_legacy_login_items(&app_name) {
            log::warn!("清理旧 macOS login item 失败，LaunchAgent 已写入: {error}");
        }

        log::info!(
            "已启用 macOS LaunchAgent 开机自启: {} -> {}",
            macos_launch_agent_label(),
            app_path.display()
        );
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let auto_launch = build_auto_launch(&app_path)?;
        auto_launch
            .enable()
            .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;

        let app_name = app_name_for_auto_launch(&app_path);
        log::info!("已启用开机自启: {app_name} -> {}", app_path.display());
        Ok(())
    }
}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    with_auto_launch_lock(disable_auto_launch_unlocked)
}

fn disable_auto_launch_unlocked() -> Result<(), AppError> {
    let app_path = get_current_auto_launch_path()?;

    #[cfg(target_os = "macos")]
    {
        remove_macos_launch_agent()?;

        let app_name = app_name_for_auto_launch(&app_path);
        if let Err(error) = remove_current_and_legacy_login_items(&app_name) {
            log::warn!("清理旧 macOS login item 失败，LaunchAgent 已删除: {error}");
        }

        log::info!("已禁用 macOS LaunchAgent 开机自启");
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let auto_launch = build_auto_launch(&app_path)?;
        auto_launch
            .disable()
            .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
        log::info!("已禁用开机自启");
        Ok(())
    }
}

/// 检查是否已启用开机自启
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    with_auto_launch_lock(is_auto_launch_enabled_unlocked)
}

fn is_auto_launch_enabled_unlocked() -> Result<bool, AppError> {
    let app_path = get_current_auto_launch_path()?;

    #[cfg(target_os = "macos")]
    {
        macos_launch_agent_matches_app(&app_path)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let auto_launch = build_auto_launch(&app_path)?;
        auto_launch
            .is_enabled()
            .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
    }
}

/// 当 settings.json 中保存了开机自启=true 时，启动期修复当前 app 的自启记录。
///
/// macOS 启动期修复必须保持无 AppleScript/System Events 副作用。
/// macOS Ventura+ 会把启动期 System Events 调用显示成后台活动提示。
pub fn repair_auto_launch_for_current_app() -> Result<(), AppError> {
    with_auto_launch_lock(repair_auto_launch_for_current_app_unlocked)
}

fn repair_auto_launch_for_current_app_unlocked() -> Result<(), AppError> {
    let app_path = get_current_auto_launch_path()?;

    #[cfg(target_os = "macos")]
    {
        ensure_macos_launch_agent_for_app(&app_path)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let auto_launch = build_auto_launch(&app_path)?;
        auto_launch
            .enable()
            .map_err(|e| AppError::Message(format!("修复开机自启失败: {e}")))
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_get_macos_app_bundle_path_valid() {
        let exe_path = std::path::Path::new("/Applications/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from("/Applications/CC Switch.app"))
        );
    }

    #[test]
    fn test_get_macos_app_bundle_path_with_spaces() {
        let exe_path =
            std::path::Path::new("/Users/test/My Apps/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from(
                "/Users/test/My Apps/CC Switch.app"
            ))
        );
    }

    #[test]
    fn test_get_macos_app_bundle_path_not_in_bundle() {
        let exe_path = std::path::Path::new("/usr/local/bin/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_macos_app_bundle_path_dev_build() {
        // 开发环境下的路径通常不在 .app bundle 内
        let exe_path = std::path::Path::new("/Users/dev/project/target/debug/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_current_auto_launch_name_comes_from_fork_bundle() {
        let app_path = std::path::Path::new("/Applications/CC Switch Pure Route.app");

        assert_eq!(app_name_for_auto_launch(app_path), "CC Switch Pure Route");
    }

    #[test]
    fn test_auto_launch_cleanup_includes_legacy_official_name() {
        let names = auto_launch_names_to_remove("CC Switch Pure Route");

        assert!(names.contains(&"CC Switch Pure Route".to_string()));
        assert!(names.contains(&"CC Switch".to_string()));
    }

    #[test]
    fn test_macos_launch_agent_label_is_stable_for_fork() {
        assert_eq!(
            macos_launch_agent_label(),
            "com.lich13.cc-switch-pure-route"
        );
    }

    #[test]
    fn test_macos_launch_agent_file_uses_user_launch_agents_dir() {
        let file = macos_launch_agent_file().expect("launch agent file");

        assert!(file.ends_with("Library/LaunchAgents/com.lich13.cc-switch-pure-route.plist"));
    }

    #[test]
    fn test_macos_open_program_arguments_use_app_bundle() {
        let app_path = std::path::Path::new("/Applications/CC Switch Pure Route.app");

        assert_eq!(
            macos_launch_agent_program_arguments(app_path),
            vec![
                "/usr/bin/open".to_string(),
                "-g".to_string(),
                "/Applications/CC Switch Pure Route.app".to_string(),
                "--args".to_string(),
                MACOS_LAUNCH_AGENT_STARTUP_ARG.to_string(),
            ]
        );
    }

    #[test]
    fn test_macos_program_arguments_use_binary_when_not_app_bundle() {
        let app_path = std::path::Path::new("/Users/dev/cc-switch/target/debug/cc-switch");

        assert_eq!(
            macos_launch_agent_program_arguments(app_path),
            vec![
                "/Users/dev/cc-switch/target/debug/cc-switch".to_string(),
                MACOS_LAUNCH_AGENT_STARTUP_ARG.to_string(),
            ]
        );
    }

    #[test]
    fn test_macos_launch_agent_plist_contains_current_bundle_path() {
        let app_path = std::path::Path::new("/Applications/CC Switch Pure Route.app");
        let plist = macos_launch_agent_plist(app_path);

        assert!(plist.contains("<string>com.lich13.cc-switch-pure-route</string>"));
        assert!(plist.contains("<string>/usr/bin/open</string>"));
        assert!(plist.contains("<string>-g</string>"));
        assert!(plist.contains("<string>/Applications/CC Switch Pure Route.app</string>"));
        assert!(plist.contains("<string>--args</string>"));
        assert!(plist.contains("<string>--cc-switch-launch-agent-startup</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
    }

    #[test]
    fn test_macos_launch_agent_startup_arg_detection_is_exact() {
        assert!(is_macos_launch_agent_startup_arg(
            "--cc-switch-launch-agent-startup"
        ));
        assert!(!is_macos_launch_agent_startup_arg(
            "--cc-switch-launch-agent-startup=1"
        ));
        assert!(!is_macos_launch_agent_startup_arg("--other"));
    }

    #[test]
    fn test_plist_escape_handles_xml_special_chars() {
        assert_eq!(
            plist_escape("/Applications/A&B <Test> \"One\".app"),
            "/Applications/A&amp;B &lt;Test&gt; &quot;One&quot;.app"
        );
    }

    #[test]
    fn test_auto_launch_lock_serializes_operations() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let active = active.clone();
            let max_active = max_active.clone();
            handles.push(std::thread::spawn(move || {
                with_auto_launch_lock(|| {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
                .expect("auto-launch lock operation should succeed");
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread should not panic");
        }

        assert_eq!(
            max_active.load(Ordering::SeqCst),
            1,
            "auto-launch operations should run one at a time inside this process"
        );
    }
}
