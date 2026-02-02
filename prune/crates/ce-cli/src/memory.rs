use anyhow::{anyhow, Result};
use ce_memory::{embedding_model_for_config, MemoryConfig, MemoryManager};
use clap::Subcommand;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum MemoryCmd {
    /// Check memory configuration and store availability.
    Doctor {
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },

    /// Recall relevant memories.
    Recall {
        /// Search query.
        query: String,
        /// Optional project id filter.
        #[arg(long)]
        project: Option<String>,
        /// Override default top-k.
        #[arg(long)]
        k: Option<usize>,
        /// Override token budget.
        #[arg(long)]
        token_budget: Option<usize>,
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },

    /// Remember a new decision or workflow.
    Remember {
        /// Content to store.
        content: String,
        /// Optional project id.
        #[arg(long)]
        project: Option<String>,
        /// Optional tags (comma-delimited or repeated).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Optional source label.
        #[arg(long)]
        source: Option<String>,
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },

    /// Save a session summary (or raw content) into memory.
    SaveSession {
        /// Path to a jsonl or markdown file.
        #[arg(long)]
        from: String,
        /// Optional project id.
        #[arg(long)]
        project: Option<String>,
        /// Optional tags (comma-delimited or repeated).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },

    /// Show memory stats.
    Stats {
        /// Optional project id filter.
        #[arg(long)]
        project: Option<String>,
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },

    /// Delete a memory by id.
    Delete {
        /// Memory id (e.g. mem:123 or mem:project:123).
        id: String,
        /// Optional project id filter.
        #[arg(long)]
        project: Option<String>,
        /// Repo root (defaults to current directory search).
        #[arg(long)]
        repo: Option<String>,
    },
}

#[derive(Serialize)]
struct MemoryItemsResult {
    items: Vec<ce_memory::MemoryItem>,
}

pub fn cmd_memory(cmd: MemoryCmd) -> Result<()> {
    match cmd {
        MemoryCmd::Doctor { repo } => cmd_doctor(repo.as_deref()),
        MemoryCmd::Recall {
            query,
            project,
            k,
            token_budget,
            repo,
        } => cmd_recall(&query, project.as_deref(), k, token_budget, repo.as_deref()),
        MemoryCmd::Remember {
            content,
            project,
            tags,
            source,
            repo,
        } => cmd_remember(&content, project.as_deref(), &tags, source.as_deref(), repo.as_deref()),
        MemoryCmd::SaveSession {
            from,
            project,
            tags,
            repo,
        } => cmd_save_session(&from, project.as_deref(), &tags, repo.as_deref()),
        MemoryCmd::Stats { project, repo } => cmd_stats(project.as_deref(), repo.as_deref()),
        MemoryCmd::Delete { id, project, repo } => cmd_delete(&id, project.as_deref(), repo.as_deref()),
    }
}

fn cmd_doctor(repo: Option<&str>) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let config_path = MemoryConfig::config_path(&repo_root);
    let mut problems: Vec<String> = Vec::new();

    let cfg = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)?;
        match serde_json::from_str::<MemoryConfig>(&raw) {
            Ok(cfg) => Some(cfg),
            Err(err) => {
                problems.push(format!("Invalid memory config at {} ({err})", config_path.display()));
                None
            }
        }
    } else {
        problems.push(format!(
            "Missing {} (run: ce bootstrap --repo . --template <web|mobile|rust> or create manually)",
            config_path.display()
        ));
        None
    };

    if let Some(cfg) = cfg {
        if !cfg.enabled {
            problems.push("Memory is disabled in .prune/memory.json".to_string());
        }

        let mode = cfg.store_mode();
        if matches!(mode, ce_memory::MemoryStoreMode::Project | ce_memory::MemoryStoreMode::Both) {
            let path = cfg.project_db_path(&repo_root);
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    problems.push(format!(
                        "Project memory directory missing: {}",
                        parent.display()
                    ));
                }
            }
        }
        if matches!(mode, ce_memory::MemoryStoreMode::Global | ce_memory::MemoryStoreMode::Both) {
            let path = cfg.global_db_path();
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    problems.push(format!(
                        "Global memory directory missing: {}",
                        parent.display()
                    ));
                }
            }
        }

        if cfg.embeddings.enabled {
            if let Err(err) = embedding_model_for_config(&cfg.embeddings) {
                problems.push(format!("Embedding model error: {err}"));
            }
        }
    }

    if problems.is_empty() {
        println!("memory doctor: OK");
        return Ok(());
    }

    println!("memory doctor: found {} issue(s):", problems.len());
    for p in problems {
        println!("- {p}");
    }
    Err(anyhow!("memory config is not ready"))
}

fn cmd_recall(
    query: &str,
    project: Option<&str>,
    k: Option<usize>,
    token_budget: Option<usize>,
    repo: Option<&str>,
) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let manager = MemoryManager::load(&repo_root)?;
    let result = manager.recall(query, project, k, token_budget)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn cmd_remember(
    content: &str,
    project: Option<&str>,
    tags: &[String],
    source: Option<&str>,
    repo: Option<&str>,
) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let manager = MemoryManager::load(&repo_root)?;
    let items = manager.remember(content, project, tags, source)?;
    let out = MemoryItemsResult { items };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_save_session(from: &str, project: Option<&str>, tags: &[String], repo: Option<&str>) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let content = fs::read_to_string(from).map_err(|err| anyhow!("failed to read {from}: {err}"))?;
    if content.trim().is_empty() {
        return Err(anyhow!("session file is empty"));
    }
    let manager = MemoryManager::load(&repo_root)?;
    let items = manager.save_session(&content, project, tags)?;
    let out = MemoryItemsResult { items };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_stats(project: Option<&str>, repo: Option<&str>) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let manager = MemoryManager::load(&repo_root)?;
    let stats = manager.stats(project)?;
    println!("{}", serde_json::to_string_pretty(&stats)?);
    Ok(())
}

fn cmd_delete(id: &str, project: Option<&str>, repo: Option<&str>) -> Result<()> {
    let repo_root = resolve_repo_root(repo)?;
    let manager = MemoryManager::load(&repo_root)?;
    manager.delete(id, project)?;
    println!("{}", json!({"deleted": id}));
    Ok(())
}

fn resolve_repo_root(repo: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = repo {
        let repo_path = PathBuf::from(path);
        if !repo_path.exists() {
            return Err(anyhow!("repo not found: {path}"));
        }
        return Ok(repo_path);
    }

    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join(".git").exists() || dir.join("Cargo.toml").exists() || dir.join(".prune").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(cwd)
}
