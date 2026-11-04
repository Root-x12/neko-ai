use anyhow::Context;
use neko_server::{crypto, db, devices, AppState, Config};
use tracing_subscriber::EnvFilter;

fn main() {
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|rt| rt.block_on(run()));
    if let Err(e) = result {
        eprintln!("\n  Neko AI error: {e:#}\n");
        // A double-clicked program's window would close before the message can be read.
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            eprintln!("  Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Config::from_env()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return command(&config, &args).await;
    }
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("creating data directory {}", config.data_dir.display()))?;
    let db = db::Db::open(&config.data_dir.join("neko.db")).context("opening database")?;
    let vault = crypto::Vault::load_or_create(&config.data_dir).context("loading secret key")?;

    let setup_code = if db.call(|c| db::user_count(c)).await? == 0 { Some(crypto::random_code()) } else { None };

    if !config.web_dir.join("index.html").exists() {
        tracing::warn!(
            "web UI not found at {} — build it with `trunk build --release` in crates/web, or set NEKO_WEB_DIR",
            config.web_dir.display()
        );
    }

    let listener =
        tokio::net::TcpListener::bind(config.bind).await.with_context(|| format!("binding {}", config.bind))?;
    print_banner(&config, setup_code.as_deref());
    if config.bundled {
        open_browser(&format!("http://localhost:{}", config.bind.port()));
    }

    let state = AppState::new(config, db, vault, setup_code);
    // Hourly cleanup of uploads that were never sent.
    let sweeper = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            neko_server::admin::maintenance(&sweeper).await;
        }
    });
    let app = neko_server::router(state);
    // Connect info lets the audit log record client addresses.
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Maintenance commands, e.g. `neko-ai reset-password alice` when a password is forgotten.
async fn command(config: &Config, args: &[String]) -> anyhow::Result<()> {
    let open_db = || db::Db::open(&config.data_dir.join("neko.db")).context("opening database");
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["list-users"] => {
            let db = open_db()?;
            let users = db.call(|c| db::list_users(c)).await?;
            if users.is_empty() {
                println!("No accounts yet. Start Neko AI and create one in the browser.");
            }
            for u in users {
                println!("{:<24} {:<6} {}", u.username, u.role.as_str(), if u.disabled { "disabled" } else { "" });
            }
            Ok(())
        }
        ["reset-password", username] => {
            let db = open_db()?;
            let name = username.to_string();
            let found = db.call(move |c| db::find_user_by_name(c, &name)).await?;
            let Some(rec) = found else { anyhow::bail!("No account called “{username}”. Try: neko-ai list-users") };
            let password = crypto::random_code();
            let hash = crypto::hash_password(&password)?;
            let id = rec.user.id;
            let update = neko_shared::UpdateUser { disabled: Some(false), ..Default::default() };
            db.call(move |c| db::update_user(c, id, &update, Some(&hash))).await?;
            println!("New password for {}: {password}", rec.user.username);
            println!("Sign in with it, then change it in Settings → Account. Other sessions were signed out.");
            Ok(())
        }
        _ => {
            println!("Usage:");
            println!("  neko-ai                          start the server");
            println!("  neko-ai list-users               show accounts");
            println!("  neko-ai reset-password <user>    set a new random password (and re-enable the account)");
            Ok(())
        }
    }
}

fn print_banner(config: &Config, setup_code: Option<&str>) {
    let urls = devices::reachable_urls(config.bind);
    println!();
    println!("  /\\_/\\   Neko AI is running");
    println!(" ( o.o )  no telemetry · data in {}", config.data_dir.display());
    if config.local_only {
        println!("          LOCAL-ONLY mode: nothing leaves your network");
    }
    println!("  > ^ <");
    for url in &urls {
        println!("   → {url}");
    }
    if let Some(code) = setup_code {
        println!();
        println!("  First run: create your admin account with setup code  {code}");
        println!("  No API key yet? That's fine: look around, then add one in Settings → Providers.");
    }
    if let Some(qr) = urls.first().filter(|u| !u.contains("localhost")).and_then(|u| devices::qr_terminal(u)) {
        println!();
        println!("  Scan to open on your phone (same Wi-Fi):");
        for line in qr.lines() {
            println!("  {line}");
        }
    }
    println!("  Keep this window open while you use Neko AI (close it to stop).");
    println!();
}

/// Best effort: if it fails, the address is in the banner anyway.
fn open_browser(url: &str) {
    use std::process::{Command, Stdio};
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
