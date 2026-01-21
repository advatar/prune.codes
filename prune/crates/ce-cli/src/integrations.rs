use anyhow::{anyhow, Result};
use clap::ValueEnum;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Supported agent clients.
///
/// We keep integration logic in Prune (this repo) and avoid forking any agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Agent {
    Codex,
    Opencode,
    Claude,
    /// Convenience option for `doctor`.
    All,
}

pub enum DoctorStore {
    Sqlite { db_path: PathBuf, hnsw_path: PathBuf },
    Surreal { engine: String, path: PathBuf, persistent: bool },
}

const TEMPLATE_AGENTS: &str = include_str!("../../../integrations/templates/AGENTS.md");
const TEMPLATE_CLAUDE: &str = include_str!("../../../integrations/templates/CLAUDE.md");
const TEMPLATE_SKILL: &str = include_str!("../../../integrations/skills/prune-context/SKILL.md");

pub fn cmd_integrate(repo: &str, agent: Agent, write_global: bool, dry_run: bool) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    match agent {
        Agent::Codex => integrate_codex(&repo_path, write_global, dry_run),
        Agent::Opencode => integrate_opencode(&repo_path, dry_run),
        Agent::Claude => integrate_claude(&repo_path, dry_run),
        Agent::All => {
            integrate_codex(&repo_path, write_global, dry_run)?;
            integrate_opencode(&repo_path, dry_run)?;
            integrate_claude(&repo_path, dry_run)?;
            Ok(())
        }
    }
}

pub fn cmd_doctor(repo: &str, agent: Agent, store: DoctorStore) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    let mut problems: Vec<String> = Vec::new();
    match &store {
        DoctorStore::Sqlite { db_path, hnsw_path } => {
            println!(
                "store: sqlite (db: {}, hnsw: {})",
                db_path.display(),
                hnsw_path.display()
            );
            if !db_path.exists() {
                problems.push(format!(
                    "Missing sqlite db at {} (run: ce index --repo . --db {} --hnsw-dir {})",
                    db_path.display(),
                    db_path.display(),
                    hnsw_path.display()
                ));
            }
            if !hnsw_path.exists() {
                problems.push(format!(
                    "Missing HNSW directory at {} (run: ce index --repo . --db {} --hnsw-dir {})",
                    hnsw_path.display(),
                    db_path.display(),
                    hnsw_path.display()
                ));
            }
        }
        DoctorStore::Surreal { engine, path, persistent } => {
            if *persistent {
                println!("store: surreal (engine: {engine}, path: {})", path.display());
                if !path.exists() {
                    problems.push(format!(
                        "Missing Surreal store at {} (run: ce index --repo . --store surreal --surreal-path {})",
                        path.display(),
                        path.display()
                    ));
                }
            } else {
                println!("store: surreal (engine: {engine}, path: in-memory)");
            }
        }
    }

    let check_codex = agent == Agent::Codex || agent == Agent::All;
    let check_opencode = agent == Agent::Opencode || agent == Agent::All;
    let check_claude = agent == Agent::Claude || agent == Agent::All;

    if check_codex {
        if !repo_path.join("AGENTS.md").exists() {
            problems.push("Missing AGENTS.md (run: ce integrate codex --repo .)".into());
        }
        // Global config is best-effort; we do not error if absent.
    }

    if check_opencode {
        if !repo_path.join("AGENTS.md").exists() {
            problems.push("Missing AGENTS.md (run: ce integrate opencode --repo .)".into());
        }
        if !repo_path.join("opencode.json").exists() {
            problems.push("Missing opencode.json (run: ce integrate opencode --repo .)".into());
        }
        if !repo_path.join("CLAUDE.md").exists() {
            problems.push("Missing CLAUDE.md (recommended for OpenCode compatibility; run: ce integrate opencode --repo .)".into());
        }
        let skill = repo_path.join(".claude").join("skills").join("prune-context").join("SKILL.md");
        if !skill.exists() {
            problems.push("Missing .claude/skills/prune-context/SKILL.md (recommended; run: ce integrate opencode --repo .)".into());
        }
    }

    if check_claude {
        if !repo_path.join("CLAUDE.md").exists() {
            problems.push("Missing CLAUDE.md (run: ce integrate claude --repo .)".into());
        }
        if !repo_path.join(".mcp.json").exists() {
            problems.push("Missing .mcp.json (run: ce integrate claude --repo .)".into());
        }
        let skill = repo_path.join(".claude").join("skills").join("prune-context").join("SKILL.md");
        if !skill.exists() {
            problems.push("Missing .claude/skills/prune-context/SKILL.md (run: ce integrate claude --repo .)".into());
        }
    }

    if problems.is_empty() {
        println!("doctor: OK (repo looks ready)");
        return Ok(());
    }

    println!("doctor: found {} issue(s):", problems.len());
    for p in problems {
        println!("- {p}");
    }
    Err(anyhow!("repo is not ready"))
}

fn integrate_codex(repo: &Path, write_global: bool, dry_run: bool) -> Result<()> {
    // Project instructions
    write_file(repo.join("AGENTS.md"), TEMPLATE_AGENTS, dry_run)?;

    // Codex global MCP config is optional; write a minimal snippet if requested.
    if write_global {
        let home = std::env::var("HOME").map_err(|_| anyhow!("HOME not set"))?;
        let cfg_dir = PathBuf::from(home).join(".codex");
        let cfg_path = cfg_dir.join("config.toml");
        let repo_abs = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
        let snippet = format!(
            "\n[mcp_servers.prune]\ncommand = \"ce\"\nargs = [\"mcp\", \"serve\", \"--repo\", \"{}\"]\n",
            repo_abs.display()
        );
        if dry_run {
            println!("[dry-run] would ensure {} exists and append prune mcp server", cfg_path.display());
        } else {
            fs::create_dir_all(&cfg_dir)?;
            let existing = fs::read_to_string(&cfg_path).unwrap_or_default();
            if !existing.contains("[mcp_servers.prune]") {
                fs::write(&cfg_path, format!("{}{}", existing, snippet))?;
                println!("Patched {}", cfg_path.display());
            } else {
                println!("{} already contains [mcp_servers.prune]", cfg_path.display());
            }
        }
    }
    Ok(())
}

fn integrate_opencode(repo: &Path, dry_run: bool) -> Result<()> {
    // OpenCode reads AGENTS.md and also supports Claude-compatible conventions.
    write_file(repo.join("AGENTS.md"), TEMPLATE_AGENTS, dry_run)?;
    // Provide a CLAUDE.md fallback so the same project can be used with Claude Code too.
    write_file(repo.join("CLAUDE.md"), TEMPLATE_CLAUDE, dry_run)?;

    // Install a project skill (OpenCode supports Claude skills).
    let skill_dir = repo.join(".claude").join("skills").join("prune-context");
    let skill_path = skill_dir.join("SKILL.md");
    if dry_run {
        println!("[dry-run] would write {}", skill_path.display());
    } else {
        fs::create_dir_all(&skill_dir)?;
        fs::write(&skill_path, TEMPLATE_SKILL)?;
        println!("Wrote {}", skill_path.display());
    }

    // Patch or create opencode.json with a local MCP server entry.
    let cfg_path = repo.join("opencode.json");
    let mut root: serde_json::Value = if cfg_path.exists() {
        serde_json::from_str(&fs::read_to_string(&cfg_path)?)
            .unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    // Ensure schema if absent.
    if root.get("$schema").is_none() {
        root["$schema"] = json!("https://opencode.ai/config.json");
    }

    // Ensure mcp map.
    if root.get("mcp").is_none() {
        root["mcp"] = json!({});
    }

    // Insert prune server.
    root["mcp"]["prune"] = json!({
        "type": "local",
        "command": ["ce", "mcp", "serve", "--repo", "."],
        "enabled": true
    });

    if dry_run {
        println!("[dry-run] would write {}", cfg_path.display());
    } else {
        fs::write(&cfg_path, serde_json::to_string_pretty(&root)?)?;
        println!("Wrote {}", cfg_path.display());
    }

    Ok(())
}

fn integrate_claude(repo: &Path, dry_run: bool) -> Result<()> {
    write_file(repo.join("CLAUDE.md"), TEMPLATE_CLAUDE, dry_run)?;

    // Claude MCP config in repo: .mcp.json
    let cfg_path = repo.join(".mcp.json");
    let mcp = json!({
        "mcpServers": {
            "prune": {
                "command": "ce",
                "args": ["mcp", "serve", "--repo", "."]
            }
        }
    });
    if dry_run {
        println!("[dry-run] would write {}", cfg_path.display());
    } else {
        fs::write(&cfg_path, serde_json::to_string_pretty(&mcp)?)?;
        println!("Wrote {}", cfg_path.display());
    }

    // Install a project skill.
    let skill_dir = repo.join(".claude").join("skills").join("prune-context");
    let skill_path = skill_dir.join("SKILL.md");
    if dry_run {
        println!("[dry-run] would write {}", skill_path.display());
    } else {
        fs::create_dir_all(&skill_dir)?;
        fs::write(&skill_path, TEMPLATE_SKILL)?;
        println!("Wrote {}", skill_path.display());
    }
    Ok(())
}

fn write_file(path: PathBuf, contents: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("[dry-run] would write {}", path.display());
        return Ok(());
    }
    fs::write(&path, contents)?;
    println!("Wrote {}", path.display());
    Ok(())
}
