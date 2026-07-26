use anyhow::{anyhow, Context, Result};
use cc_switch_lib::{
    ensure_rustls_crypto_provider, web_auth, web_config::WebdConfig, webd, AppState, Database,
};
use std::{env, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    ensure_rustls_crypto_provider();

    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--version") {
        println!("cc-switch-webd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("serve") => serve_command(&args).await,
        Some("admin") => admin_command(&args),
        Some("status") => status_command(&args).await,
        _ => {
            print_help();
            Ok(())
        }
    }
}

async fn serve_command(args: &[String]) -> Result<()> {
    let config = WebdConfig::load(config_arg(args))?;
    let db = open_database(&config)?;
    web_auth::ensure_tables(&db)?;

    if !web_auth::admin_exists(&db)? {
        log::warn!(
            "cc-switch-webd has no admin user; run `cc-switch-webd admin init` before exposing WebUI"
        );
    }

    seed_webd_database(&db)?;
    let state = AppState::new(db);
    validate_proxy_loopback(&state, config.production).await?;

    state
        .proxy_service
        .start()
        .await
        .map_err(|err| anyhow!("启动代理服务失败: {err}"))?;

    webd::serve(config, state).await
}

fn admin_command(args: &[String]) -> Result<()> {
    let config = WebdConfig::load(config_arg(args))?;
    let db = open_database(&config)?;
    match args.get(1).map(String::as_str) {
        Some("init") => {
            let username = value_after(args, "--username").unwrap_or_else(|| "admin".to_string());
            let password = admin_password(args)?;
            web_auth::init_admin(&db, &username, &password)?;
            println!("admin {username} initialized");
            Ok(())
        }
        Some("reset-password") => {
            let username = value_after(args, "--username").unwrap_or_else(|| "admin".to_string());
            let password = admin_password(args)?;
            web_auth::reset_password(&db, &username, &password)?;
            println!("admin {username} password reset");
            Ok(())
        }
        _ => {
            print_help();
            Ok(())
        }
    }
}

async fn status_command(args: &[String]) -> Result<()> {
    let config = WebdConfig::load(config_arg(args))?;
    let db = open_database(&config)?;
    let state = AppState::new(db.clone());
    let proxy = state
        .proxy_service
        .get_config()
        .await
        .map_err(|err| anyhow!("读取代理配置失败: {err}"))?;

    println!("database: {}", config.database_path.display());
    println!("webui: {}", config.admin.listen);
    println!("webui_static: {}", config.static_dir.display());
    println!("proxy: {}:{}", proxy.listen_address, proxy.listen_port);
    println!("production: {}", config.production);
    println!("admin_initialized: {}", web_auth::admin_exists(&db)?);
    Ok(())
}

fn open_database(config: &WebdConfig) -> Result<Arc<Database>> {
    Ok(Arc::new(
        Database::init_at_for_webd(&config.database_path)
            .with_context(|| format!("初始化数据库 {}", config.database_path.display()))?,
    ))
}

fn seed_webd_database(db: &Database) -> Result<()> {
    db.init_default_skill_repos()
        .context("初始化默认 Skills 仓库")?;
    db.init_default_official_providers()
        .context("初始化默认供应商")?;
    Ok(())
}

async fn validate_proxy_loopback(state: &AppState, production: bool) -> Result<()> {
    if !production {
        return Ok(());
    }
    let proxy = state
        .proxy_service
        .get_config()
        .await
        .map_err(|err| anyhow!("读取代理配置失败: {err}"))?;
    let ip = proxy
        .listen_address
        .parse::<std::net::IpAddr>()
        .with_context(|| format!("代理监听地址无效: {}", proxy.listen_address))?;
    if !ip.is_loopback() {
        anyhow::bail!("production=true 时代理监听地址必须绑定到 loopback");
    }
    Ok(())
}

fn admin_password(args: &[String]) -> Result<String> {
    value_after(args, "--password")
        .or_else(|| env::var("CC_SWITCH_WEBD_ADMIN_PASSWORD").ok())
        .context("请通过 --password 或 CC_SWITCH_WEBD_ADMIN_PASSWORD 提供管理员密码")
}

fn config_arg(args: &[String]) -> Option<PathBuf> {
    value_after(args, "--config").map(PathBuf::from)
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn print_help() {
    eprintln!(
        r#"cc-switch-webd {}

用法:
  cc-switch-webd serve [--config /etc/cc-switch-webd/config.toml]
  cc-switch-webd admin init [--username admin] [--password ...] [--config ...]
  cc-switch-webd admin reset-password [--username admin] [--password ...] [--config ...]
  cc-switch-webd status [--config ...]
  cc-switch-webd --version

未传 --password 时会读取 CC_SWITCH_WEBD_ADMIN_PASSWORD。
"#,
        env!("CARGO_PKG_VERSION")
    );
}
