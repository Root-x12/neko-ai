//! Skills: cyber "agent integrations". A skill is a GitHub-hosted security tool (ScopeSentry,
//! a recon agent, ...) that Neko clones onto the server, runs under supervision, and reaches
//! over its HTTP/MCP endpoint, with a playbook that teaches the assistant how to drive it.
//!
//! Safety model: cloning and running are **admin only**, the same bar as local-command MCP
//! servers. Importing a repo only clones it — nothing executes. The repo's `neko-skill.json`
//! manifest (its setup and run commands) is shown to the admin, who runs it as a separate,
//! explicit step. Clones live under `<data>/skills/` and are removed when the skill is deleted.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use axum::{
    extract::{Path as AxPath, State},
    routing::{get, post},
    Json, Router,
};
use neko_shared::{Role, Skill, SkillImport, SkillLog, SkillManifest, SkillPatch, SkillStatus};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

use crate::{
    admin::{audit, ClientIp},
    auth::AuthUser,
    crypto, db,
    error::{AppError, AppResult},
    AppState,
};

/// Keep at most this many recent output lines per skill.
const LOG_LINES: usize = 500;
/// A skill's MCP tools get aliases `mcp<base+id>__…`; the offset keeps them clear of real
/// MCP servers' ids in the alias namespace.
pub const SKILL_MCP_BASE: i64 = 2_000_000_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/skills", get(list).post(import))
        .route("/skills/{id}", axum::routing::patch(patch).delete(remove))
        .route("/skills/{id}/run", post(run))
        .route("/skills/{id}/stop", post(stop))
        .route("/skills/{id}/logs", get(logs))
}

// ---------- process supervisor ----------

/// Supervises skill processes and keeps a ring buffer of their output. One per server, in
/// `AppState`; clones share the same inner state.
#[derive(Clone, Default)]
pub struct SkillRunner {
    inner: Arc<Mutex<Supervisor>>,
}

#[derive(Default)]
struct Supervisor {
    /// The long-running `run` child of each skill, kept so it can be stopped.
    running: HashMap<i64, Child>,
    /// Captured output per skill, surviving after the process exits.
    logs: HashMap<i64, Arc<Mutex<VecDeque<String>>>>,
}

impl SkillRunner {
    async fn log_buf(&self, id: i64) -> Arc<Mutex<VecDeque<String>>> {
        let mut g = self.inner.lock().await;
        g.logs.entry(id).or_default().clone()
    }

    async fn clear_logs(&self, id: i64) {
        if let Some(buf) = self.inner.lock().await.logs.get(&id) {
            buf.lock().await.clear();
        }
    }

    pub async fn snapshot(&self, id: i64) -> Vec<String> {
        let buf = self.log_buf(id).await;
        let g = buf.lock().await;
        g.iter().cloned().collect()
    }

    pub async fn is_up(&self, id: i64) -> bool {
        let mut g = self.inner.lock().await;
        match g.running.get_mut(&id) {
            // `try_wait` returns Ok(None) while the child is still alive.
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Runs the one-off setup command to completion. Returns whether it succeeded.
    async fn setup(&self, id: i64, dir: &Path, cmd: &str, env: &[(String, String)]) -> bool {
        let buf = self.log_buf(id).await;
        push(&buf, &format!("$ {cmd}")).await;
        let mut command = shell(dir, cmd, env);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                push(&buf, &format!("could not start: {e}")).await;
                return false;
            }
        };
        pipe(&buf, child.stdout.take());
        pipe(&buf, child.stderr.take());
        match child.wait().await {
            Ok(status) if status.success() => {
                push(&buf, "✓ setup finished").await;
                true
            }
            Ok(status) => {
                push(&buf, &format!("✗ setup exited with {status}")).await;
                false
            }
            Err(e) => {
                push(&buf, &format!("✗ setup error: {e}")).await;
                false
            }
        }
    }

    /// Starts the long-running `run` command and keeps supervising it. Returns whether it
    /// launched (not whether it stays up).
    async fn start(&self, id: i64, dir: &Path, cmd: &str, env: &[(String, String)]) -> bool {
        self.stop(id).await;
        let buf = self.log_buf(id).await;
        push(&buf, &format!("$ {cmd}")).await;
        let mut command = shell(dir, cmd, env);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                push(&buf, &format!("could not start: {e}")).await;
                return false;
            }
        };
        pipe(&buf, child.stdout.take());
        pipe(&buf, child.stderr.take());
        self.inner.lock().await.running.insert(id, child);
        true
    }

    /// Stops a skill's running process, if any.
    pub async fn stop(&self, id: i64) {
        let child = self.inner.lock().await.running.remove(&id);
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

/// Builds a shell command for `cmd`, run in `dir` with `env` set and no inherited profile.
fn shell(dir: &Path, cmd: &str, env: &[(String, String)]) -> Command {
    let mut c = Command::new("bash");
    c.arg("-c").arg(cmd).current_dir(dir);
    c.env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in env {
        c.env(k, v);
    }
    c
}

/// Reads a child pipe line by line into the ring buffer.
fn pipe<R>(buf: &Arc<Mutex<VecDeque<String>>>, reader: Option<R>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let Some(reader) = reader else { return };
    let buf = buf.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            push(&buf, &line).await;
        }
    });
}

async fn push(buf: &Arc<Mutex<VecDeque<String>>>, line: &str) {
    let mut g = buf.lock().await;
    if g.len() >= LOG_LINES {
        g.pop_front();
    }
    g.push_back(line.to_string());
}

// ---------- git ----------

/// Accepts only `https://github.com/<owner>/<repo>` and returns `(owner, repo)`. This keeps
/// cloning to GitHub over HTTPS: no ssh, no other hosts, no local paths.
fn parse_github(url: &str) -> Result<(String, String), AppError> {
    let bad = || AppError::bad_request("Enter a GitHub repository URL like https://github.com/owner/repo");
    let rest = url.trim().strip_prefix("https://github.com/").ok_or_else(bad)?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|s| !s.is_empty()).ok_or_else(bad)?;
    let repo = parts.next().filter(|s| !s.is_empty()).ok_or_else(bad)?;
    if parts.next().is_some() {
        return Err(bad());
    }
    let ok = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok(owner) || !ok(repo) {
        return Err(bad());
    }
    Ok((owner.to_string(), repo.to_string()))
}

/// Shallow-clones a repo into `dest`. `git_ref` picks a branch or tag.
async fn clone_repo(url: &str, git_ref: &str, dest: &Path) -> Result<(), String> {
    let mut c = Command::new("git");
    // Flags (including --config) must come before the `--` that ends option parsing.
    c.arg("clone").arg("--depth").arg("1").arg("--config").arg("core.hooksPath=/dev/null");
    if !git_ref.is_empty() {
        c.arg("--branch").arg(git_ref);
    }
    c.arg("--").arg(url).arg(dest);
    // No prompts, no system config, hooks disabled, HTTPS only.
    c.env("GIT_TERMINAL_PROMPT", "0").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_ALLOW_PROTOCOL", "https");
    let out = c.output().await.map_err(|e| format!("git not available: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("clone failed: {}", err.lines().last().unwrap_or("unknown error")))
    }
}

/// Reads `neko-skill.json` from the clone, or synthesises a blank manifest to edit.
async fn read_manifest(dir: &Path, repo: &str) -> SkillManifest {
    match tokio::fs::read_to_string(dir.join("neko-skill.json")).await {
        Ok(text) => {
            let mut m: SkillManifest = serde_json::from_str(&text).unwrap_or_default();
            if m.name.trim().is_empty() {
                m.name = repo.to_string();
            }
            m
        }
        Err(_) => SkillManifest {
            name: repo.to_string(),
            mode: "cyber".into(),
            instructions: format!(
                "This skill wraps the {repo} tool. Fill in its setup command, run command and \
                 HTTP endpoint, then write here how you want the assistant to use it."
            ),
            ..Default::default()
        },
    }
}

// ---------- handlers ----------

fn require_admin(a: &AuthUser) -> AppResult<()> {
    if a.user.role == Role::Admin {
        Ok(())
    } else {
        Err(AppError::forbidden("Skills run tools on the server, so only the admin can manage them"))
    }
}

async fn list(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<Skill>>> {
    let uid = a.user.id;
    let mut skills = s.db.call(move |c| db::list_skills(c, uid)).await?;
    // Reconcile stored status with what's actually running.
    for sk in &mut skills {
        if sk.status == SkillStatus::Running && !s.skills.is_up(sk.id).await {
            sk.status = SkillStatus::Stopped;
            let id = sk.id;
            let _ = s.db.call(move |c| db::set_skill_status(c, uid, id, SkillStatus::Stopped)).await;
        }
    }
    Ok(Json(skills))
}

async fn import(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(input): Json<SkillImport>,
) -> AppResult<Json<Skill>> {
    require_admin(&a)?;
    let (_, repo) = parse_github(&input.repo_url)?;
    let git_ref = input.git_ref.trim().to_string();
    if git_ref.len() > 100 || git_ref.contains(|c: char| c.is_whitespace()) {
        return Err(AppError::bad_request("That branch or tag name doesn't look right"));
    }
    let repo_url = format!("https://github.com/{}/{repo}", parse_github(&input.repo_url)?.0);

    let dir_name = format!("{}-{}", slug(&repo), &crypto::random_token()[..8]);
    let dir = skills_dir(&s).join(&dir_name);
    tokio::fs::create_dir_all(skills_dir(&s)).await.map_err(AppError::internal)?;

    clone_repo(&repo_url, &git_ref, &dir).await.map_err(AppError::bad_request)?;
    let manifest = read_manifest(&dir, &repo).await;

    let uid = a.user.id;
    let dir_str = dir.to_string_lossy().to_string();
    let name = if manifest.name.trim().is_empty() { repo.clone() } else { manifest.name.clone() };
    let (name_c, ref_c, man_c, url_c) = (name.clone(), git_ref.clone(), manifest.clone(), repo_url.clone());
    let id =
        s.db.call(move |c| db::insert_skill(c, uid, &name_c, &url_c, &ref_c, &dir_str, &man_c)).await.map_err(|e| {
            // Don't leave an orphaned clone behind if the row fails to write.
            let _ = std::fs::remove_dir_all(&dir);
            AppError::internal(e)
        })?;
    audit(&s, Some(&a.user), &ip.0, "skill.import", &name, &repo_url).await;
    let skill = s.db.call(move |c| db::get_skill(c, uid, id)).await?;
    skill.map(|r| Json(r.skill)).ok_or_else(|| AppError::not_found("Skill"))
}

async fn run(State(s): State<AppState>, a: AuthUser, ip: ClientIp, AxPath(id): AxPath<i64>) -> AppResult<Json<Skill>> {
    require_admin(&a)?;
    let uid = a.user.id;
    let rec = s.db.call(move |c| db::get_skill(c, uid, id)).await?.ok_or_else(|| AppError::not_found("Skill"))?;
    let m = &rec.skill.manifest;
    if m.run.trim().is_empty() {
        return Err(AppError::bad_request("This skill has no run command. Add one in its manifest first."));
    }
    let dir = PathBuf::from(&rec.dir);
    let env = rec.env(&s.vault);
    s.skills.clear_logs(id).await;

    // Setup is optional and runs once, before the tool is started.
    if !m.setup.trim().is_empty() {
        let _ = s.db.call(move |c| db::set_skill_status(c, uid, id, SkillStatus::Installing)).await;
        if !s.skills.setup(id, &dir, &m.setup, &env).await {
            let _ = s.db.call(move |c| db::set_skill_status(c, uid, id, SkillStatus::Failed)).await;
            return current(&s, uid, id).await;
        }
    }
    let started = s.skills.start(id, &dir, &m.run, &env).await;
    let status = if started { SkillStatus::Running } else { SkillStatus::Failed };
    let _ = s.db.call(move |c| db::set_skill_status(c, uid, id, status)).await;
    audit(&s, Some(&a.user), &ip.0, "skill.run", &rec.skill.name, m.run.clone()).await;
    current(&s, uid, id).await
}

async fn stop(State(s): State<AppState>, a: AuthUser, ip: ClientIp, AxPath(id): AxPath<i64>) -> AppResult<Json<Skill>> {
    require_admin(&a)?;
    let uid = a.user.id;
    let rec = s.db.call(move |c| db::get_skill(c, uid, id)).await?.ok_or_else(|| AppError::not_found("Skill"))?;
    s.skills.stop(id).await;
    let _ = s.db.call(move |c| db::set_skill_status(c, uid, id, SkillStatus::Stopped)).await;
    audit(&s, Some(&a.user), &ip.0, "skill.stop", &rec.skill.name, "").await;
    current(&s, uid, id).await
}

async fn logs(State(s): State<AppState>, a: AuthUser, AxPath(id): AxPath<i64>) -> AppResult<Json<SkillLog>> {
    require_admin(&a)?;
    let uid = a.user.id;
    let rec = s.db.call(move |c| db::get_skill(c, uid, id)).await?.ok_or_else(|| AppError::not_found("Skill"))?;
    let mut status = rec.skill.status;
    if status == SkillStatus::Running && !s.skills.is_up(id).await {
        status = SkillStatus::Stopped;
    }
    Ok(Json(SkillLog { status, lines: s.skills.snapshot(id).await }))
}

async fn patch(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    AxPath(id): AxPath<i64>,
    Json(p): Json<SkillPatch>,
) -> AppResult<Json<Skill>> {
    require_admin(&a)?;
    let uid = a.user.id;
    let rec = s.db.call(move |c| db::get_skill(c, uid, id)).await?.ok_or_else(|| AppError::not_found("Skill"))?;

    let mut manifest = rec.skill.manifest.clone();
    if let Some(instr) = p.instructions {
        manifest.instructions = instr;
    }
    if let Some(endpoint) = p.endpoint {
        let endpoint = endpoint.trim().to_string();
        if !endpoint.is_empty() && !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
            return Err(AppError::bad_request("The endpoint must start with http:// or https://"));
        }
        manifest.endpoint = endpoint;
    }
    let man_c = manifest.clone();
    s.db.call(move |c| db::set_skill_manifest(c, uid, id, &man_c)).await?;

    if let Some(env) = p.env {
        let pairs: Vec<(String, String)> =
            env.into_iter().map(|(k, v)| (k.trim().to_string(), v)).filter(|(k, _)| !k.is_empty()).collect();
        for (k, _) in &pairs {
            let valid = k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return Err(AppError::bad_request(format!("“{k}” is not a valid environment variable name")));
            }
        }
        let names: Vec<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
        let blob = s.vault.encrypt(&serde_json::to_string(&pairs).unwrap_or_default());
        s.db.call(move |c| db::set_skill_env(c, uid, id, blob, &names)).await?;
    }

    if let Some(enabled) = p.enabled {
        s.db.call(move |c| db::set_skill_enabled(c, uid, id, enabled)).await?;
    }
    audit(&s, Some(&a.user), &ip.0, "skill.update", &rec.skill.name, "").await;
    current(&s, uid, id).await
}

async fn remove(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    AxPath(id): AxPath<i64>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let uid = a.user.id;
    s.skills.stop(id).await;
    let dir = s.db.call(move |c| db::delete_skill(c, uid, id)).await?;
    let Some(dir) = dir else { return Err(AppError::not_found("Skill")) };
    // Only remove directories inside our own skills folder.
    if Path::new(&dir).starts_with(skills_dir(&s)) {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    audit(&s, Some(&a.user), &ip.0, "skill.delete", id.to_string(), dir).await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- helpers ----------

async fn current(s: &AppState, uid: i64, id: i64) -> AppResult<Json<Skill>> {
    let skill = s.db.call(move |c| db::get_skill(c, uid, id)).await?;
    skill.map(|r| Json(r.skill)).ok_or_else(|| AppError::not_found("Skill"))
}

fn skills_dir(s: &AppState) -> PathBuf {
    s.config.data_dir.join("skills")
}

/// A filesystem-safe short name from the repo.
fn slug(repo: &str) -> String {
    let s: String =
        repo.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "skill".into()
    } else {
        s.chars().take(32).collect()
    }
}

/// System-prompt block describing the active skills so the assistant can drive them.
pub fn playbook_prompt(skills: &[db::SkillRecord]) -> Option<String> {
    let active: Vec<&db::SkillRecord> = skills.iter().collect();
    if active.is_empty() {
        return None;
    }
    let mut out = String::from(
        "# Active skills\nThese security tools are running and connected. Use them when relevant and \
         follow each playbook.\n",
    );
    for rec in active {
        let m = &rec.skill.manifest;
        out.push_str(&format!("\n## {}\n", rec.skill.name));
        if !m.endpoint.is_empty() {
            out.push_str(&format!("Endpoint: {}\n", m.endpoint));
        }
        let instr = m.instructions.trim();
        if !instr.is_empty() {
            out.push_str(instr);
            out.push('\n');
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_github_https_repos() {
        assert!(parse_github("https://github.com/jungl+/x").is_err());
        assert_eq!(
            parse_github("https://github.com/Autumn-27/ScopeSentry").unwrap(),
            ("Autumn-27".into(), "ScopeSentry".into())
        );
        assert_eq!(parse_github("https://github.com/Autumn-27/ScopeSentry.git").unwrap().1, "ScopeSentry");
        assert_eq!(parse_github("https://github.com/owner/repo/").unwrap(), ("owner".into(), "repo".into()));
        assert!(parse_github("git@github.com:owner/repo.git").is_err());
        assert!(parse_github("https://gitlab.com/owner/repo").is_err());
        assert!(parse_github("https://github.com/owner").is_err());
        assert!(parse_github("https://github.com/owner/repo/sub").is_err());
        assert!(parse_github("file:///etc/passwd").is_err());
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("ScopeSentry"), "scopesentry");
        assert_eq!(slug("my.tool/../x"), "my-tool----x");
        assert_eq!(slug("..."), "skill");
    }
}
