use anyhow::{anyhow, Context, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CORTEX_REPO: &str = "https://github.com/hjertefolger/cortex";

#[derive(Subcommand, Debug)]
pub enum VendorCmd {
    /// Install a vendored dependency.
    Install {
        #[arg(value_enum)]
        target: VendorTarget,
        /// Optional git ref (tag/branch/commit).
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Update a vendored dependency.
    Update {
        #[arg(value_enum)]
        target: VendorTarget,
        /// Optional git ref (tag/branch/commit).
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Diagnose a vendored dependency.
    Doctor {
        #[arg(value_enum)]
        target: VendorTarget,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VendorTarget {
    Cortex,
}

pub fn cmd_vendor(cmd: VendorCmd) -> Result<()> {
    match cmd {
        VendorCmd::Install { target, r#ref } => match target {
            VendorTarget::Cortex => {
                let repo_root = resolve_repo_root()?;
                ensure_cortex_installed(&repo_root, r#ref.as_deref())?;
                println!("Cortex installed at {}", cortex_dir(&repo_root).display());
                Ok(())
            }
        },
        VendorCmd::Update { target, r#ref } => match target {
            VendorTarget::Cortex => {
                let repo_root = resolve_repo_root()?;
                update_cortex(&repo_root, r#ref.as_deref())?;
                println!("Cortex updated at {}", cortex_dir(&repo_root).display());
                Ok(())
            }
        },
        VendorCmd::Doctor { target } => match target {
            VendorTarget::Cortex => {
                let repo_root = resolve_repo_root()?;
                cortex_doctor(&repo_root)
            }
        },
    }
}

pub fn ensure_cortex_installed(repo_root: &Path, r#ref: Option<&str>) -> Result<PathBuf> {
    let dir = cortex_dir(repo_root);
    if dir.exists() {
        if let Some(r) = r#ref {
            update_cortex(repo_root, Some(r))?;
            return Ok(dir);
        }

        let mcp = dir.join("dist").join("mcp-server.js");
        if mcp.exists() {
            return Ok(dir);
        }

        build_cortex(&dir)?;
        return Ok(dir);
    }

    let parent = dir
        .parent()
        .ok_or_else(|| anyhow!("invalid cortex vendor dir"))?;
    fs::create_dir_all(parent)?;

    let mut clone = Command::new("git");
    clone
        .arg("clone")
        .arg(CORTEX_REPO)
        .arg(&dir);
    run(&mut clone, "git clone cortex")?;

    if let Some(r) = r#ref {
        checkout_ref(&dir, r)?;
    }

    build_cortex(&dir)?;
    Ok(dir)
}

fn update_cortex(repo_root: &Path, r#ref: Option<&str>) -> Result<()> {
    let dir = cortex_dir(repo_root);
    if !dir.exists() {
        return Err(anyhow!(
            "Cortex not installed. Run: ce vendor install cortex"
        ));
    }

    let mut fetch = Command::new("git");
    fetch.args(["-C", dir.to_str().unwrap_or("."), "fetch", "--all", "--tags"]);
    run(&mut fetch, "git fetch cortex")?;

    if let Some(r) = r#ref {
        checkout_ref(&dir, r)?;
    } else {
        let mut pull = Command::new("git");
        pull.args(["-C", dir.to_str().unwrap_or("."), "pull", "--ff-only"]);
        run(&mut pull, "git pull cortex")?;
    }

    build_cortex(&dir)?;
    Ok(())
}

fn checkout_ref(dir: &Path, r#ref: &str) -> Result<()> {
    let mut checkout = Command::new("git");
    checkout.args(["-C", dir.to_str().unwrap_or("."), "checkout", r#ref]);
    run(&mut checkout, "git checkout cortex")
}

fn build_cortex(dir: &Path) -> Result<()> {
    let mut npm_install = Command::new("npm");
    npm_install.arg("install").current_dir(dir);
    run(&mut npm_install, "npm install cortex")?;

    let mut npm_build = Command::new("npm");
    npm_build.arg("run").arg("build:mcp").current_dir(dir);
    run(&mut npm_build, "npm run build:mcp")?;
    Ok(())
}

fn cortex_doctor(repo_root: &Path) -> Result<()> {
    let dir = cortex_dir(repo_root);
    if !dir.exists() {
        return Err(anyhow!("Cortex not installed. Run: ce vendor install cortex"));
    }

    let node_major = node_major_version()?;
    if node_major < 18 {
        return Err(anyhow!(
            "Node >= 18 is required (found v{node_major})."
        ));
    }

    let mcp = dir.join("dist").join("mcp-server.js");
    if !mcp.exists() {
        return Err(anyhow!(
            "Missing dist/mcp-server.js (run: ce vendor update cortex)"
        ));
    }

    let ok = cortex_tools_list(&dir)?;
    if !ok {
        return Err(anyhow!("Cortex MCP tools/list check failed"));
    }

    println!("cortex doctor: OK");
    Ok(())
}

fn cortex_tools_list(dir: &Path) -> Result<bool> {
    let mut child = Command::new("node")
        .arg("dist/mcp-server.js")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn cortex mcp server")?;

    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("missing stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("missing stdout"))?;
    let mut stdout = BufReader::new(stdout);

    let payload = r#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#;
    writeln!(stdin, "{payload}")?;
    stdin.flush()?;

    let mut line = String::new();
    for _ in 0..5 {
        line.clear();
        if stdout.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            if v.get("result").and_then(|r| r.get("tools")).is_some() {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(true);
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    Ok(false)
}

fn node_major_version() -> Result<u32> {
    let output = Command::new("node")
        .arg("--version")
        .output()
        .context("failed to run node --version")?;
    if !output.status.success() {
        return Err(anyhow!("node --version failed"));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim().trim_start_matches('v');
    let major = trimmed
        .split('.')
        .next()
        .ok_or_else(|| anyhow!("unexpected node version"))?
        .parse::<u32>()
        .map_err(|_| anyhow!("unexpected node version"))?;
    Ok(major)
}

pub fn cortex_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".prune").join("vendors").join("cortex")
}

pub fn cortex_wrapper_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".prune").join("bin").join("cortex-mcp")
}

pub fn ensure_cortex_wrapper(repo_root: &Path, dry_run: bool) -> Result<PathBuf> {
    let path = cortex_wrapper_path(repo_root);
    if path.exists() {
        return Ok(path);
    }
    if dry_run {
        println!("[dry-run] would write {}", path.display());
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec node "$ROOT/.prune/vendors/cortex/dist/mcp-server.js"
"#;
    fs::write(&path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }
    Ok(path)
}

fn resolve_repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join(".git").exists() || dir.join(".prune").exists() || dir.join("Cargo.toml").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(cwd)
}

fn run(cmd: &mut Command, label: &str) -> Result<()> {
    let status = cmd.status().with_context(|| format!("failed to run {label}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{label} failed with status {status}"))
    }
}
