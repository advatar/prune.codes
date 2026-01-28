use anyhow::{anyhow, Context, Result};
use clap::Parser;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

type HmacSha256 = Hmac<Sha256>;

#[derive(Parser, Debug, Clone)]
#[command(name = "prune-sync")]
#[command(about = "Webhook sync service for Prune")]
struct Args {
    /// Host:port to bind (HTTP).
    #[arg(long, default_value = "127.0.0.1:47801")]
    bind: String,
    /// GitHub repository in ORG/REPO form.
    #[arg(long)]
    repo: String,
    /// Default branch to sync.
    #[arg(long, default_value = "main")]
    branch: String,
    /// Local mirror directory (working tree).
    #[arg(long)]
    mirror_dir: String,
    /// SQLite db path for context index.
    #[arg(long)]
    db: String,
    /// HNSW directory for vector index.
    #[arg(long)]
    hnsw_dir: String,
    /// Optional path to write sync status JSON.
    #[arg(long)]
    status_file: Option<String>,
    /// Optional webhook secret (HMAC SHA-256).
    #[arg(long)]
    webhook_secret: Option<String>,
    /// Git binary path.
    #[arg(long, default_value = "git")]
    git_path: String,
    /// CE binary path (defaults to sibling `ce`).
    #[arg(long)]
    ce_path: Option<String>,
    /// Reindex all files (full).
    #[arg(long, default_value_t = false)]
    full: bool,
    /// Remove stale files from index.
    #[arg(long, default_value_t = false)]
    prune: bool,
}

#[derive(Clone)]
struct SyncConfig {
    args: Args,
    webhook_secret: Option<String>,
    git_token: Option<String>,
    git_path: String,
    ce_path: String,
    status_path: Option<PathBuf>,
}

#[derive(Default)]
struct SyncState {
    in_progress: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct SyncStatus {
    last_indexed_sha: Option<String>,
    last_indexed_at: Option<u64>,
    last_error: Option<String>,
    last_event: Option<String>,
}

enum SyncTarget {
    Sha(String),
    Branch,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(build_config(args)?);
    let state = Arc::new(Mutex::new(SyncState::default()));
    let server =
        Server::http(&config.args.bind).map_err(|err| anyhow!("bind server failed: {err}"))?;
    println!("prune-sync listening on {}", config.args.bind);

    for request in server.incoming_requests() {
        if let Err(err) = handle_request(request, &config, &state) {
            eprintln!("request error: {err:#}");
        }
    }
    Ok(())
}

fn build_config(args: Args) -> Result<SyncConfig> {
    let webhook_secret = args
        .webhook_secret
        .clone()
        .or_else(|| std::env::var("PRUNE_WEBHOOK_SECRET").ok());
    let git_token = std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("PRUNE_GITHUB_TOKEN").ok());
    let ce_path = args
        .ce_path
        .clone()
        .or_else(default_ce_path)
        .unwrap_or_else(|| "ce".to_string());
    let status_path = args.status_file.clone().map(PathBuf::from);
    Ok(SyncConfig {
        git_path: args.git_path.clone(),
        args,
        webhook_secret,
        git_token,
        ce_path,
        status_path,
    })
}

fn default_ce_path() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join("ce");
    if candidate.exists() {
        Some(candidate.to_string_lossy().to_string())
    } else {
        None
    }
}

fn handle_request(
    request: Request,
    config: &Arc<SyncConfig>,
    state: &Arc<Mutex<SyncState>>,
) -> Result<()> {
    let method = request.method().clone();
    let url = request.url().to_string();
    match (method, url.as_str()) {
        (Method::Get, "/health") => {
            respond_json(request, 200, "{\"status\":\"ok\"}");
        }
        (Method::Post, "/sync") => {
            if queue_sync(
                SyncTarget::Branch,
                "manual".to_string(),
                default_clone_url(config),
                Arc::clone(config),
                Arc::clone(state),
            ) {
                respond_json(request, 200, "{\"status\":\"sync_started\"}");
            } else {
                respond_json(request, 202, "{\"status\":\"sync_in_progress\"}");
            }
        }
        (Method::Post, "/github/webhook") => {
            handle_webhook(request, config, state)?;
        }
        _ => {
            respond_json(request, 404, "{\"error\":\"not_found\"}");
        }
    }
    Ok(())
}

fn handle_webhook(
    request: Request,
    config: &Arc<SyncConfig>,
    state: &Arc<Mutex<SyncState>>,
) -> Result<()> {
    let mut request = request;
    let mut body = Vec::new();
    if let Err(err) = request.as_reader().read_to_end(&mut body) {
        eprintln!("failed to read webhook body: {err}");
        respond_json(request, 400, "{\"error\":\"invalid_body\"}");
        return Ok(());
    }

    if let Some(secret) = config.webhook_secret.as_deref() {
        let signature = match header_value(&request, "X-Hub-Signature-256") {
            Some(value) => value,
            None => {
                respond_json(request, 401, "{\"error\":\"missing_signature\"}");
                return Ok(());
            }
        };
        if !verify_signature(secret, &body, &signature) {
            respond_json(request, 401, "{\"error\":\"invalid_signature\"}");
            return Ok(());
        }
    }

    let event = header_value(&request, "X-GitHub-Event").unwrap_or_default();
    if event != "push" {
        respond_json(request, 202, "{\"status\":\"ignored\"}");
        return Ok(());
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            respond_json(request, 400, "{\"error\":\"invalid_json\"}");
            return Ok(());
        }
    };
    let repo_full = payload
        .pointer("/repository/full_name")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if repo_full != config.args.repo {
        respond_json(request, 202, "{\"status\":\"repo_mismatch\"}");
        return Ok(());
    }

    let ref_name = payload
        .get("ref")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let expected_ref = format!("refs/heads/{}", config.args.branch);
    if ref_name != expected_ref {
        respond_json(request, 202, "{\"status\":\"branch_mismatch\"}");
        return Ok(());
    }

    let after = payload
        .get("after")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if after.is_empty() || is_zero_sha(after) {
        respond_json(request, 202, "{\"status\":\"empty_sha\"}");
        return Ok(());
    }

    let clone_url = payload
        .pointer("/repository/clone_url")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let clone_url = if clone_url.is_empty() {
        default_clone_url(config)
    } else {
        clone_url
    };

    if queue_sync(
        SyncTarget::Sha(after.to_string()),
        "push".to_string(),
        clone_url,
        Arc::clone(config),
        Arc::clone(state),
    ) {
        respond_json(request, 200, "{\"status\":\"sync_queued\"}");
    } else {
        respond_json(request, 202, "{\"status\":\"sync_in_progress\"}");
    }
    Ok(())
}

fn queue_sync(
    target: SyncTarget,
    event: String,
    clone_url: String,
    config: Arc<SyncConfig>,
    state: Arc<Mutex<SyncState>>,
) -> bool {
    {
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.in_progress {
            return false;
        }
        guard.in_progress = true;
    }

    thread::spawn(move || {
        let event_label = event.clone();
        let result = run_sync(target, event, clone_url, &config);
        if let Err(err) = result {
            eprintln!("sync error: {err:#}");
            let _ = write_status(
                config.status_path.as_deref(),
                SyncStatus {
                    last_indexed_sha: None,
                    last_indexed_at: Some(now_epoch()),
                    last_error: Some(err.to_string()),
                    last_event: Some(event_label),
                },
            );
        }
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.in_progress = false;
    });

    true
}

fn run_sync(
    target: SyncTarget,
    event: String,
    clone_url: String,
    config: &SyncConfig,
) -> Result<()> {
    let mirror_dir = PathBuf::from(&config.args.mirror_dir);
    ensure_parent_dir(&mirror_dir)?;

    let db_path = PathBuf::from(&config.args.db);
    if let Some(parent) = db_path.parent() {
        ensure_parent_dir(parent)?;
    }
    ensure_parent_dir(Path::new(&config.args.hnsw_dir))?;

    let clone_url = with_token(&clone_url, config.git_token.as_deref());
    if !mirror_dir.join(".git").exists() {
        run_command(
            &config.git_path,
            &[
                "clone".to_string(),
                clone_url,
                mirror_dir.to_string_lossy().to_string(),
            ],
            None,
        )?;
    } else {
        run_command(
            &config.git_path,
            &[
                "-C".to_string(),
                mirror_dir.to_string_lossy().to_string(),
                "fetch".to_string(),
                "--prune".to_string(),
                "origin".to_string(),
            ],
            None,
        )?;
    }

    ensure_branch(&config.git_path, &mirror_dir, &config.args.branch)?;
    let target_ref = match target {
        SyncTarget::Sha(sha) => sha,
        SyncTarget::Branch => format!("origin/{}", config.args.branch),
    };
    run_command(
        &config.git_path,
        &[
            "-C".to_string(),
            mirror_dir.to_string_lossy().to_string(),
            "reset".to_string(),
            "--hard".to_string(),
            target_ref,
        ],
        None,
    )?;

    let mut ce_args = vec![
        "index".to_string(),
        "--repo".to_string(),
        mirror_dir.to_string_lossy().to_string(),
        "--db".to_string(),
        config.args.db.clone(),
        "--hnsw-dir".to_string(),
        config.args.hnsw_dir.clone(),
    ];
    if config.args.full {
        ce_args.push("--full".to_string());
    }
    if config.args.prune {
        ce_args.push("--prune".to_string());
    }
    run_command(&config.ce_path, &ce_args, None)?;

    let head_sha = run_command(
        &config.git_path,
        &[
            "-C".to_string(),
            mirror_dir.to_string_lossy().to_string(),
            "rev-parse".to_string(),
            "HEAD".to_string(),
        ],
        None,
    )?
    .trim()
    .to_string();

    write_status(
        config.status_path.as_deref(),
        SyncStatus {
            last_indexed_sha: Some(head_sha),
            last_indexed_at: Some(now_epoch()),
            last_error: None,
            last_event: Some(event),
        },
    )?;

    Ok(())
}

fn ensure_branch(git_path: &str, mirror_dir: &Path, branch: &str) -> Result<()> {
    let checkout = run_command(
        git_path,
        &[
            "-C".to_string(),
            mirror_dir.to_string_lossy().to_string(),
            "checkout".to_string(),
            branch.to_string(),
        ],
        None,
    );
    if checkout.is_ok() {
        return Ok(());
    }
    run_command(
        git_path,
        &[
            "-C".to_string(),
            mirror_dir.to_string_lossy().to_string(),
            "checkout".to_string(),
            "-B".to_string(),
            branch.to_string(),
            format!("origin/{}", branch),
        ],
        None,
    )?;
    Ok(())
}

fn run_command(cmd: &str, args: &[String], cwd: Option<&Path>) -> Result<String> {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().with_context(|| format!("run {}", cmd))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("command failed: {}\n{}\n{}", cmd, stdout, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn write_status(path: Option<&Path>, status: SyncStatus) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        ensure_parent_dir(parent)?;
    }
    let data = serde_json::to_vec_pretty(&status)?;
    fs::write(path, data)?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn with_token(url: &str, token: Option<&str>) -> String {
    let Some(token) = token else {
        return url.to_string();
    };
    if let Some(stripped) = url.strip_prefix("https://") {
        return format!("https://x-access-token:{token}@{stripped}");
    }
    url.to_string()
}

fn default_clone_url(config: &SyncConfig) -> String {
    format!("https://github.com/{}.git", config.args.repo)
}

fn header_value(request: &Request, name: &str) -> Option<String> {
    request.headers().iter().find_map(|header| {
        if header.field.as_str().as_str().eq_ignore_ascii_case(name) {
            Some(header.value.as_str().to_string())
        } else {
            None
        }
    })
}

fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let signature = signature.trim();
    let value = signature.strip_prefix("sha256=").unwrap_or(signature);
    let provided = match hex::decode(value) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(body);
    mac.verify_slice(&provided).is_ok()
}

fn is_zero_sha(value: &str) -> bool {
    value.chars().all(|ch| ch == '0')
}

fn respond_json(request: Request, status: u16, body: &str) {
    let mut response = Response::from_string(body);
    response = response.with_status_code(StatusCode(status));
    if let Ok(header) = Header::from_bytes("Content-Type", "application/json") {
        response.add_header(header);
    }
    let _ = request.respond(response);
}
