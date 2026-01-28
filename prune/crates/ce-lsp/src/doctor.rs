use crate::config::LspConfig;
use crate::session::LspSession;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::time::Duration;
use which::which;

#[derive(Debug)]
pub struct ServerDoctorReport {
    pub name: String,
    pub command: Vec<String>,
    pub command_path: Option<PathBuf>,
    pub root_marker: Option<PathBuf>,
    pub init_ok: bool,
    pub capabilities: Option<Value>,
    pub error: Option<String>,
}

pub async fn run_doctor(config: &LspConfig, repo_root: &Path) -> Vec<ServerDoctorReport> {
    let mut reports = Vec::new();
    for (name, server) in &config.servers {
        let root_marker = config.find_root_marker(repo_root, server);
        let command_path = resolve_command_path(server.command.first().map(String::as_str));
        let mut init_ok = false;
        let mut capabilities = None;
        let mut error = None;
        if command_path.is_some() {
            let init_timeout = Duration::from_millis(config.initialize_timeout_ms);
            let default_timeout = Duration::from_millis(config.default_timeout_ms);
            let session_res = LspSession::start(
                name,
                server,
                repo_root,
                &config.features,
                default_timeout,
                init_timeout,
                config.max_requests_per_pack,
            )
            .await;
            match session_res {
                Ok(session) => {
                    init_ok = true;
                    capabilities = Some(session.capabilities().clone());
                    drop(session);
                }
                Err(e) => {
                    error = Some(format!("init failed: {e}"));
                }
            }
        } else if server.command.is_empty() {
            error = Some("missing command".to_string());
        } else {
            error = Some("command not found in PATH".to_string());
        }
        reports.push(ServerDoctorReport {
            name: name.clone(),
            command: server.command.clone(),
            command_path,
            root_marker,
            init_ok,
            capabilities,
            error,
        });
    }
    reports
}

fn resolve_command_path(cmd: Option<&str>) -> Option<PathBuf> {
    let cmd = cmd?;
    let candidate = PathBuf::from(cmd);
    if candidate.is_absolute() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    which(cmd).ok()
}
