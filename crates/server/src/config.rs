use std::{net::SocketAddr, path::PathBuf};

/// Runtime settings, read from `NEKO_*` environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    /// Lets anyone create an account after the first (admin) one exists.
    pub allow_signup: bool,
    /// Adds the `Secure` flag to the session cookie. Turn on when serving over HTTPS.
    pub secure_cookies: bool,
    /// Lets `fetch_url` reach private/LAN addresses. Off by default to stop prompt-injected
    /// models from probing your network.
    pub fetch_allow_private: bool,
    /// Self-hosted lockdown: providers and MCP servers must be on this machine or the LAN, and
    /// the web tools are off, so no prompt or file ever leaves your network.
    pub local_only: bool,
    /// Take the client address from `X-Forwarded-For` (only behind a reverse proxy you run).
    pub trust_proxy: bool,
    /// Audit entries older than this are deleted. `None` keeps them forever.
    pub audit_retention_days: Option<u32>,
    /// Running from a downloaded release (web files next to the program): open the browser.
    pub bundled: bool,
}

/// Where the web UI and data live when not set explicitly. In a source checkout that's the
/// build output; in a downloaded release it's the folders next to the program, so double-clicking
/// works no matter which directory the system starts it in.
fn default_dirs() -> (PathBuf, PathBuf, bool) {
    let dev = PathBuf::from("crates/web/dist");
    if dev.join("index.html").exists() {
        return (PathBuf::from("data"), dev, false);
    }
    match std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)) {
        Some(dir) if dir.join("web").join("index.html").exists() => (dir.join("data"), dir.join("web"), true),
        _ => (PathBuf::from("data"), dev, false),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = env_or("NEKO_BIND", "0.0.0.0:8080")
            .parse()
            .map_err(|e| anyhow::anyhow!("NEKO_BIND is not a valid address: {e}"))?;
        let (data_dir, web_dir, bundled) = default_dirs();
        Ok(Self {
            bind,
            data_dir: std::env::var("NEKO_DATA_DIR").map(PathBuf::from).unwrap_or(data_dir),
            web_dir: std::env::var("NEKO_WEB_DIR").map(PathBuf::from).unwrap_or(web_dir),
            bundled: bundled && !env_flag("NEKO_NO_BROWSER"),
            allow_signup: env_flag("NEKO_ALLOW_SIGNUP"),
            secure_cookies: env_flag("NEKO_SECURE_COOKIES"),
            fetch_allow_private: env_flag("NEKO_FETCH_ALLOW_PRIVATE"),
            local_only: env_flag("NEKO_LOCAL_ONLY"),
            trust_proxy: env_flag("NEKO_TRUST_PROXY"),
            audit_retention_days: match std::env::var("NEKO_AUDIT_RETENTION_DAYS") {
                Ok(v) => match v.trim().parse::<u32>() {
                    Ok(0) => None,
                    Ok(d) => Some(d),
                    Err(_) => anyhow::bail!("NEKO_AUDIT_RETENTION_DAYS must be a number of days (0 = keep forever)"),
                },
                Err(_) => Some(365),
            },
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_flag(key: &str) -> bool {
    matches!(std::env::var(key).unwrap_or_default().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}
