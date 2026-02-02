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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum IntegrationPreset {
    #[value(name = "prune-memory")]
    PruneMemory,
}

pub enum DoctorStore {
    Sqlite {
        db_path: PathBuf,
        hnsw_path: PathBuf,
    },
    Surreal {
        engine: String,
        path: PathBuf,
        persistent: bool,
    },
}

const TEMPLATE_AGENTS: &str = include_str!("../../../integrations/templates/AGENTS.md");
const TEMPLATE_CLAUDE: &str = include_str!("../../../integrations/templates/CLAUDE.md");
const TEMPLATE_SKILL: &str = include_str!("../../../integrations/skills/prune-context/SKILL.md");
const TEMPLATE_MEMORY_PLUGIN: &str = r#"// Optional OpenCode plugin scaffold for Prune Memory.
// Enable it in opencode.json if you want automated prompts/hooks.
export default {
  name: "prune-memory-autosave",
  onSessionCreated() {
    // Suggest a memory recall at session start.
  },
  onSessionIdle() {
    // Suggest saving key decisions via memory.remember or memory.save_session.
  }
};
"#;
const CONTEXT7_AGENTS_BLOCK: &str = r#"
## External Docs (Context7)
- Use Prune for repo context packs; use Context7 for external library docs.
- Before writing code that touches external APIs: fetch the minimal Context7 snippet, then proceed with a Prune pack.
- Keep queries short and avoid sending proprietary code. Set CONTEXT7_API_KEY in your environment.
"#;
const MEMORY_AGENTS_BLOCK: &str = r#"
## Prune Memory (persistent)
- At task start: call `memory.recall` with a short query describing the goal and area.
- After decisions: call `memory.remember` with the decision, constraints, and commands.
- For long tasks: call `memory.save_session` with a short summary.
- Keep memories concise and avoid secrets.
"#;
const CODEX_PRUNE_SNIPPET: &str = r#"
[mcp_servers.prune]
command = "ce"
args = ["mcp", "serve"]
cwd = "."
enabled = true
startup_timeout_sec = 20
tool_timeout_sec = 60
"#;
const CODEX_CONTEXT7_SNIPPET: &str = r#"
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
enabled = true
startup_timeout_sec = 20
tool_timeout_sec = 60
"#;

pub fn cmd_integrate(
    repo: &str,
    agent: Agent,
    write_global: bool,
    with_context7: bool,
    preset: Option<IntegrationPreset>,
    dry_run: bool,
) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    match agent {
        Agent::Codex => integrate_codex(&repo_path, write_global, with_context7, preset, dry_run),
        Agent::Opencode => integrate_opencode(&repo_path, with_context7, preset, dry_run),
        Agent::Claude => integrate_claude(&repo_path, dry_run),
        Agent::All => {
            integrate_codex(&repo_path, write_global, with_context7, preset, dry_run)?;
            integrate_opencode(&repo_path, with_context7, preset, dry_run)?;
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
        DoctorStore::Surreal {
            engine,
            path,
            persistent,
        } => {
            if *persistent {
                println!(
                    "store: surreal (engine: {engine}, path: {})",
                    path.display()
                );
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
        let skill = repo_path
            .join(".claude")
            .join("skills")
            .join("prune-context")
            .join("SKILL.md");
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
        let skill = repo_path
            .join(".claude")
            .join("skills")
            .join("prune-context")
            .join("SKILL.md");
        if !skill.exists() {
            problems.push(
                "Missing .claude/skills/prune-context/SKILL.md (run: ce integrate claude --repo .)"
                    .into(),
            );
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

fn integrate_codex(
    repo: &Path,
    write_global: bool,
    with_context7: bool,
    preset: Option<IntegrationPreset>,
    dry_run: bool,
) -> Result<()> {
    // Project instructions
    let mut agents_body = if with_context7 {
        format!("{}\n{}", TEMPLATE_AGENTS, context7_agents_block())
    } else {
        TEMPLATE_AGENTS.to_string()
    };
    if preset == Some(IntegrationPreset::PruneMemory) {
        agents_body.push_str("\n");
        agents_body.push_str(MEMORY_AGENTS_BLOCK);
        ensure_memory_config(repo, dry_run)?;
    }
    write_file(repo.join("AGENTS.md"), &agents_body, dry_run)?;

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
            println!(
                "[dry-run] would ensure {} exists and append prune mcp server",
                cfg_path.display()
            );
        } else {
            fs::create_dir_all(&cfg_dir)?;
            let existing = fs::read_to_string(&cfg_path).unwrap_or_default();
            if !existing.contains("[mcp_servers.prune]") {
                fs::write(&cfg_path, format!("{}{}", existing, snippet))?;
                println!("Patched {}", cfg_path.display());
            } else {
                println!(
                    "{} already contains [mcp_servers.prune]",
                    cfg_path.display()
                );
            }
        }
    }

    if with_context7 {
        let cfg_dir = repo.join(".codex");
        let cfg_path = cfg_dir.join("config.toml");
        let prune_snippet = codex_prune_server_snippet();
        let context7_snippet = codex_context7_server_snippet();
        if dry_run {
            println!(
                "[dry-run] would ensure {} exists and append context7 mcp servers",
                cfg_path.display()
            );
        } else {
            fs::create_dir_all(&cfg_dir)?;
            let existing = fs::read_to_string(&cfg_path).unwrap_or_default();
            let mut updated = existing.clone();
            if !existing.contains("[mcp_servers.prune]") {
                updated.push_str("\n");
                updated.push_str(&prune_snippet);
            }
            if !existing.contains("[mcp_servers.context7]") {
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push_str(&context7_snippet);
            }
            if updated != existing {
                fs::write(&cfg_path, updated)?;
                println!("Patched {}", cfg_path.display());
            } else {
                println!(
                    "{} already contains prune/context7 servers",
                    cfg_path.display()
                );
            }
        }
    }
    Ok(())
}

fn integrate_opencode(
    repo: &Path,
    with_context7: bool,
    preset: Option<IntegrationPreset>,
    dry_run: bool,
) -> Result<()> {
    // OpenCode reads AGENTS.md and also supports Claude-compatible conventions.
    let mut agents_body = if with_context7 {
        format!("{}\n{}", TEMPLATE_AGENTS, context7_agents_block())
    } else {
        TEMPLATE_AGENTS.to_string()
    };
    if preset == Some(IntegrationPreset::PruneMemory) {
        agents_body.push_str("\n");
        agents_body.push_str(MEMORY_AGENTS_BLOCK);
    }
    write_file(repo.join("AGENTS.md"), &agents_body, dry_run)?;
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
        serde_json::from_str(&fs::read_to_string(&cfg_path)?).unwrap_or_else(|_| json!({}))
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

    if with_context7 {
        root["mcp"]["context7"] = json!({
            "type": "remote",
            "url": "https://mcp.context7.com/mcp",
            "enabled": true,
            "headers": {
                "Authorization": "Bearer ${CONTEXT7_API_KEY}"
            }
        });
    }

    if dry_run {
        println!("[dry-run] would write {}", cfg_path.display());
    } else {
        fs::write(&cfg_path, serde_json::to_string_pretty(&root)?)?;
        println!("Wrote {}", cfg_path.display());
    }

    if preset == Some(IntegrationPreset::PruneMemory) {
        let plugin_dir = repo.join(".opencode").join("plugins");
        let plugin_path = plugin_dir.join("prune_memory_autosave.ts");
        if dry_run {
            println!("[dry-run] would write {}", plugin_path.display());
        } else {
            fs::create_dir_all(&plugin_dir)?;
            fs::write(&plugin_path, TEMPLATE_MEMORY_PLUGIN)?;
            println!("Wrote {}", plugin_path.display());
        }
        ensure_memory_config(repo, dry_run)?;
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

fn context7_agents_block() -> &'static str {
    CONTEXT7_AGENTS_BLOCK
}

fn ensure_memory_config(repo: &Path, dry_run: bool) -> Result<()> {
    let path = repo.join(".prune").join("memory.json");
    if path.exists() {
        return Ok(());
    }
    if dry_run {
        println!("[dry-run] would write {}", path.display());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let contents = ce_memory::MemoryConfig::default_json()?;
    fs::write(&path, contents)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn codex_prune_server_snippet() -> &'static str {
    CODEX_PRUNE_SNIPPET
}

fn codex_context7_server_snippet() -> &'static str {
    CODEX_CONTEXT7_SNIPPET
}
