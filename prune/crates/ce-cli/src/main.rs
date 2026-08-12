use anyhow::{anyhow, Result};
use ce_core::model::{ContextPack, FragmentView, SignalBundle, StrategyConfig};
use ce_core::pack::{pack_with_strategy, Candidate, CandidateNeighbor};
use ce_core::signals;
use ce_core::snippet;
use ce_core::tokenizer::TokenCounter;
use ce_lang_rust::RustAdapter;
use ce_store::query;
use ce_store::{Db, Embedder, GraphReportOptions};
use clap::{Parser, Subcommand, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json as json;

mod inception;
mod stage_b;
mod tasks;
use ce_cli::integrations;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PackFormat {
    Text,
    Json,
    Both,
}

#[derive(Parser, Debug)]
#[command(name = "ce")]
#[command(about = "Context Engine CLI (index/search/pack)")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Index a repository into SQLite + embeddings.
    Index {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        db: String,
        #[arg(long)]
        hnsw_dir: String,
        /// Reindex all files even if unchanged.
        #[arg(long, default_value_t = false)]
        full: bool,
        /// Remove indexed files that no longer exist on disk (prune stale entries).
        #[arg(long, default_value_t = false)]
        prune: bool,

        /// Skip rebuilding resolved edges (faster indexing, but weaker subgraph expansion).
        #[arg(long, default_value_t = false)]
        skip_edges: bool,
        #[arg(long, default_value_t = 20000)]
        max_files: usize,
    },

    /// Bootstrap a repo with Prune defaults (.prune/*) and (optionally) build an index.
    ///
    /// This is intended for "inception" / first-run. It writes a minimal set of
    /// config files, golden path candidates, and onboarding hints.
    Bootstrap {
        #[arg(long)]
        repo: String,

        /// Template name: `web`, `mobile`, or `rust`.
        #[arg(long)]
        template: inception::ProjectTemplate,

        /// Optional template subtype (e.g. `cli`).
        #[arg(long)]
        subtype: Option<inception::ProjectSubtype>,

        /// Overwrite existing .prune files.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Skip indexing (only write files).
        #[arg(long, default_value_t = false)]
        skip_index: bool,

        /// Pass-through: reindex all files even if unchanged.
        #[arg(long, default_value_t = false)]
        full: bool,

        /// Pass-through: prune stale index entries.
        #[arg(long, default_value_t = false)]
        prune: bool,

        /// Pass-through: skip rebuilding resolved edges.
        #[arg(long, default_value_t = false)]
        skip_edges: bool,

        /// Pass-through: max number of files to walk.
        #[arg(long, default_value_t = 20000)]
        max_files: usize,

        /// Skip writing onboarding hints.
        #[arg(long, default_value_t = false)]
        skip_onboarding: bool,

        /// Skip writing golden path candidates.
        #[arg(long, default_value_t = false)]
        skip_golden_paths: bool,
    },

    /// Hybrid search over the repo index.
    Search {
        #[arg(long)]
        db: String,
        #[arg(long)]
        hnsw_dir: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 8)]
        k: usize,
        #[arg(long, default_value_t = 0.5)]
        alpha: f32,
    },

    /// Explain the indexed repo graph as a compact Markdown report.
    GraphReport {
        #[arg(long)]
        db: String,
        /// Optional output path. If omitted, the report is printed to stdout.
        #[arg(long)]
        out: Option<String>,
        /// Maximum connected fragments to include in the hub table.
        #[arg(long, default_value_t = 10)]
        max_hubs: usize,
        /// Maximum cross-file relationships to include.
        #[arg(long, default_value_t = 12)]
        max_edges: usize,
    },

    /// Build a minimal context pack for a task.
    Pack {
        #[arg(long)]
        db: String,
        #[arg(long)]
        hnsw_dir: String,
        #[arg(long)]
        task: String,
        /// Optional strategy id stored in the DB.
        #[arg(long)]
        strategy_id: Option<String>,
        /// Optional strategy config file (.json or .toml). Overrides strategy_id.
        #[arg(long)]
        strategy_file: Option<String>,
        /// Inline strategy JSON (overrides strategy_id).
        #[arg(long)]
        strategy_json: Option<String>,
        /// Inline strategy TOML (overrides strategy_id).
        #[arg(long)]
        strategy_toml: Option<String>,

        /// Override the strategy's budget (chars). If omitted, uses strategy config or default.
        #[arg(long)]
        budget_chars: Option<usize>,

        /// Override the strategy's budget (tokens). If set, token budget is enforced.
        #[arg(long)]
        budget_tokens: Option<usize>,

        /// Override tokenizer spec used for token counting/budgeting.
        ///
        /// Examples:
        /// - `o200k_base` (recommended for GPT-5 / o-series models)
        /// - `cl100k_base`
        /// - `model:gpt-4o`
        #[arg(long)]
        tokenizer: Option<String>,
        /// Override the strategy's max bodies. If omitted, uses strategy config or default.
        #[arg(long)]
        max_bodies: Option<usize>,
        /// Override the strategy's hybrid alpha. If omitted, uses strategy config or default.
        #[arg(long)]
        alpha: Option<f32>,

        /// Output format for the pack.
        #[arg(long, value_enum, default_value_t = PackFormat::Text)]
        format: PackFormat,
    },

    /// Evaluate context retrieval/packing quality against a JSONL task set.
    ///
    /// This does NOT run an LLM. It scores the Context Engine based on whether
    /// expected paths/symbols appear in the produced context pack.
    Eval {
        #[arg(long)]
        db: String,
        #[arg(long)]
        hnsw_dir: String,
        /// Path to a JSONL file where each line is a task spec.
        #[arg(long)]
        tasks: String,

        /// Optional strategy id stored in the DB.
        #[arg(long)]
        strategy_id: Option<String>,
        /// Optional strategy config file (.json or .toml). Overrides strategy_id.
        #[arg(long)]
        strategy_file: Option<String>,
        /// Inline strategy JSON (overrides strategy_id).
        #[arg(long)]
        strategy_json: Option<String>,
        /// Inline strategy TOML (overrides strategy_id).
        #[arg(long)]
        strategy_toml: Option<String>,

        /// Optional limit on number of tasks to evaluate (0 = no limit).
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// Optional output path for per-task results (JSONL).
        #[arg(long)]
        out: Option<String>,
    },

    /// Manage repair recipe memory.
    Recipe {
        #[command(subcommand)]
        cmd: RecipeCmd,
    },

    /// Manage repository decisions and golden paths.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },

    /// Manage stored strategy configs (DGM “genomes”).
    Strategy {
        #[command(subcommand)]
        cmd: StrategyCmd,
    },

    /// Utilities for working with task datasets.
    ///
    /// In particular, you can convert SWE-bench-ish instances (with `problem_statement` + `patch`)
    /// into Context Engine eval JSONL (with derived `expect_paths`/`expect_symbols`).
    Tasks {
        #[command(subcommand)]
        cmd: TasksCmd,
    },

    /// Integrate Prune with a specific coding agent (Codex/OpenCode/Claude).
    Integrate {
        #[arg(long)]
        repo: String,
        #[arg(long, value_enum)]
        agent: integrations::Agent,
        /// Also patch the user's global config where applicable (e.g., Codex config).
        #[arg(long, default_value_t = false)]
        write_global: bool,
        /// Print changes but do not write files.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Check whether a repo is ready for a given agent integration.
    Doctor {
        #[arg(long)]
        repo: String,
        #[arg(long, value_enum, default_value_t = integrations::Agent::All)]
        agent: integrations::Agent,
    },

    /// Run the MCP server (stdio) for a repo, ensuring an index exists.
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
}

#[derive(Subcommand, Debug)]
enum McpCmd {
    /// Serve MCP over stdio using the ce-mcp binary, ensuring the repo is indexed.
    Serve {
        #[arg(long)]
        repo: String,
        /// Path to sqlite db (defaults to <repo>/.ce/index.sqlite)
        #[arg(long)]
        db: Option<String>,
        /// Directory for HNSW dumps (defaults to <repo>/.ce/hnsw)
        #[arg(long)]
        hnsw_dir: Option<String>,
        /// Path to ce-mcp binary (defaults to `ce-mcp` in PATH).
        #[arg(long)]
        ce_mcp_path: Option<String>,
        /// If the index is missing, build it automatically.
        #[arg(long, default_value_t = true)]
        auto_index: bool,
    },
}

#[derive(Subcommand, Debug)]
enum RecipeCmd {
    /// Add a repair recipe entry.
    Add {
        #[arg(long)]
        db: String,
        /// Failure text (or use --failure_file).
        #[arg(long)]
        failure: Option<String>,
        /// Failure text file path.
        #[arg(long)]
        failure_file: Option<String>,
        /// Pack summary text.
        #[arg(long, default_value = "")]
        pack_summary: String,
        /// Patch metadata or diff summary.
        #[arg(long, default_value = "")]
        patch_meta: String,
        /// Optional tags (comma-separated).
        #[arg(long)]
        tags: Option<String>,
        /// Optional success token count.
        #[arg(long)]
        success_tokens: Option<i64>,
        /// Optional iterations count.
        #[arg(long)]
        iterations: Option<i64>,
    },

    /// List stored recipes.
    List {
        #[arg(long)]
        db: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },

    /// Export recipes to JSONL.
    Export {
        #[arg(long)]
        db: String,
        #[arg(long)]
        out: String,
    },
}

#[derive(Subcommand, Debug)]
enum MemoryCmd {
    Add {
        #[arg(long)]
        db: String,
        #[arg(long, value_parser = ["decision", "golden_path"])]
        kind: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        content: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        tags: Option<String>,
    },
    List {
        #[arg(long)]
        db: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
}

#[derive(Subcommand, Debug)]
enum TasksCmd {
    /// Convert SWE-bench-ish JSON/JSONL into Context Engine eval JSONL.
    ///
    /// Input examples:
    /// - JSON array of objects
    /// - JSONL where each line is an object
    ///
    /// Required fields per instance:
    /// - `problem_statement` (or `task`/`prompt`)
    /// - `patch` (or `gold_patch`/`solution_patch`) for deriving expectations
    ImportSweBench {
        /// Input path (.json or .jsonl)
        #[arg(long)]
        input: String,
        /// Output JSONL path
        #[arg(long)]
        out: String,
        /// Optional limit (0 = no limit)
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// Also attempt to derive Rust symbol names from the patch.
        #[arg(long, default_value_t = false)]
        derive_symbols: bool,
    },
    /// Run checkout, index, pack, optional patch agent, and official harness evaluation.
    RunSweBench {
        #[arg(long)]
        input: String,
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        out: String,
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long)]
        agent_command: Option<String>,
        #[arg(
            long,
            default_value = "python -m swebench.harness.run_evaluation --dataset_name {dataset} --predictions_path {predictions} --run_id {run_id}"
        )]
        harness_command: String,
        #[arg(long, default_value = "prune-stage-b")]
        run_id: String,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum StrategyCmd {
    /// Add (or upsert) a strategy config into the DB.
    Add {
        #[arg(long)]
        db: String,
        #[arg(long)]
        name: String,
        /// Strategy config file path (.json or .toml). If omitted and no inline config is provided, uses defaults.
        #[arg(long)]
        config: Option<String>,
        /// Inline JSON string for StrategyConfig.
        #[arg(long)]
        config_json: Option<String>,
        /// Inline TOML string for StrategyConfig.
        #[arg(long)]
        config_toml: Option<String>,
        /// Optional parent strategy id (for genealogy).
        #[arg(long)]
        parent_id: Option<String>,
    },

    /// List stored strategies.
    List {
        #[arg(long)]
        db: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Also print the full config JSON for each strategy.
        #[arg(long, default_value_t = false)]
        show_config: bool,
    },

    /// Get a single strategy by id.
    Get {
        #[arg(long)]
        db: String,
        #[arg(long)]
        id: String,
        /// Pretty-print JSON.
        #[arg(long, default_value_t = false)]
        pretty: bool,
    },

    /// Evolve strategy configs by random mutation + evaluation.
    ///
    /// This is a simple, local, DGM-ish hillclimber:
    /// - start from a base StrategyConfig (defaults, DB id, or file)
    /// - generate `population` mutations per generation
    /// - evaluate each on a JSONL task set (like `ce eval`)
    /// - keep the best; write it to DB with its score
    Evolve {
        #[arg(long)]
        db: String,
        /// Path to a JSONL file where each line is a task spec.
        #[arg(long)]
        tasks: String,

        /// Optional base strategy id stored in the DB.
        #[arg(long)]
        base_strategy_id: Option<String>,
        /// Optional base strategy config file (.json or .toml). Overrides base_strategy_id.
        #[arg(long)]
        base_strategy_file: Option<String>,
        /// Inline base strategy JSON (overrides base_strategy_id).
        #[arg(long)]
        base_strategy_json: Option<String>,
        /// Inline base strategy TOML (overrides base_strategy_id).
        #[arg(long)]
        base_strategy_toml: Option<String>,

        /// Generations to run.
        #[arg(long, default_value_t = 20)]
        generations: usize,
        /// Candidates per generation.
        #[arg(long, default_value_t = 25)]
        population: usize,
        /// Optional limit on number of tasks to evaluate (0 = no limit).
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// Optional deterministic RNG seed (0 = derive from time).
        #[arg(long, default_value_t = 0)]
        seed: u64,

        /// Strategy name prefix used when storing results.
        #[arg(long, default_value = "evolved")]
        name_prefix: String,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Index {
            repo,
            db,
            hnsw_dir,
            full,
            prune,
            skip_edges,
            max_files,
        } => cmd_index(&repo, &db, &hnsw_dir, full, prune, skip_edges, max_files),
        Cmd::Bootstrap {
            repo,
            template,
            subtype,
            force,
            skip_index,
            full,
            prune,
            skip_edges,
            max_files,
            skip_onboarding,
            skip_golden_paths,
        } => cmd_bootstrap(
            &repo,
            template,
            subtype,
            force,
            skip_index,
            full,
            prune,
            skip_edges,
            max_files,
            skip_onboarding,
            skip_golden_paths,
        ),
        Cmd::Search {
            db,
            hnsw_dir,
            query,
            k,
            alpha,
        } => cmd_search(&db, &hnsw_dir, &query, k, alpha),
        Cmd::GraphReport {
            db,
            out,
            max_hubs,
            max_edges,
        } => cmd_graph_report(&db, out.as_deref(), max_hubs, max_edges),
        Cmd::Pack {
            db,
            hnsw_dir,
            task,
            strategy_id,
            strategy_file,
            strategy_json,
            strategy_toml,
            budget_chars,
            budget_tokens,
            tokenizer,
            max_bodies,
            alpha,
            format,
        } => cmd_pack(
            &db,
            &hnsw_dir,
            &task,
            strategy_id.as_deref(),
            strategy_file.as_deref(),
            strategy_json.as_deref(),
            strategy_toml.as_deref(),
            budget_chars,
            budget_tokens,
            tokenizer.as_deref(),
            max_bodies,
            alpha,
            format,
        ),

        Cmd::Eval {
            db,
            hnsw_dir,
            tasks,
            strategy_id,
            strategy_file,
            strategy_json,
            strategy_toml,
            limit,
            out,
        } => cmd_eval(
            &db,
            &hnsw_dir,
            &tasks,
            strategy_id.as_deref(),
            strategy_file.as_deref(),
            strategy_json.as_deref(),
            strategy_toml.as_deref(),
            limit,
            out.as_deref(),
        ),

        Cmd::Recipe { cmd } => cmd_recipe(cmd),
        Cmd::Memory { cmd } => cmd_memory(cmd),
        Cmd::Strategy { cmd } => cmd_strategy(cmd),
        Cmd::Tasks { cmd } => cmd_tasks(cmd),

        Cmd::Integrate {
            repo,
            agent,
            write_global,
            dry_run,
        } => integrations::cmd_integrate(&repo, agent, write_global, dry_run),
        Cmd::Doctor { repo, agent } => integrations::cmd_doctor(&repo, agent),
        Cmd::Mcp { cmd } => match cmd {
            McpCmd::Serve {
                repo,
                db,
                hnsw_dir,
                ce_mcp_path,
                auto_index,
            } => cmd_mcp_serve(
                &repo,
                db.as_deref(),
                hnsw_dir.as_deref(),
                ce_mcp_path.as_deref(),
                auto_index,
            ),
        },
    }
}

fn cmd_mcp_serve(
    repo: &str,
    db: Option<&str>,
    hnsw_dir: Option<&str>,
    ce_mcp_path: Option<&str>,
    auto_index: bool,
) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    let db_path = db
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_path.join(".ce").join("index.sqlite"));
    let hnsw_path = hnsw_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_path.join(".ce").join("hnsw"));

    if auto_index {
        let needs_index = !db_path.exists() || !hnsw_path.exists();
        if needs_index {
            println!("Index missing; building index before starting MCP server...");
            fs::create_dir_all(repo_path.join(".ce"))?;
            cmd_index(
                repo_path.to_string_lossy().as_ref(),
                db_path.to_string_lossy().as_ref(),
                hnsw_path.to_string_lossy().as_ref(),
                false,
                true,
                false,
                20000,
            )?;
        }
    }

    let bin = ce_mcp_path.unwrap_or("ce-mcp");
    let status = std::process::Command::new(bin)
        .arg("--db")
        .arg(db_path)
        .arg("--hnsw-dir")
        .arg(hnsw_path)
        .status();

    match status {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => Err(anyhow!("ce-mcp exited with status {st}")),
        Err(e) => Err(anyhow!("failed to start ce-mcp ({bin}): {e}")),
    }
}

fn cmd_index(
    repo: &str,
    db_path: &str,
    hnsw_dir: &str,
    full: bool,
    prune: bool,
    skip_edges: bool,
    max_files: usize,
) -> Result<()> {
    use ce_lang_swift::SwiftAdapter;
    use ce_lang_tsreact::TsReactAdapter;

    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    fs::create_dir_all(Path::new(hnsw_dir))?;

    let db = Db::open(db_path)?;
    let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;

    // Ensure tree-sitter grammars load (fail fast on missing grammars)
    let _ = RustAdapter::new()?;
    let _ = SwiftAdapter::new()?;
    let _ = TsReactAdapter::new_ts()?;
    let _ = TsReactAdapter::new_tsx()?;

    // Collect supported source files (gitignore-aware)
    #[derive(Clone)]
    struct CandidateFile {
        disk_path: PathBuf,
        language: String,
    }

    let mut files: Vec<CandidateFile> = Vec::new();
    let mut truncated = false;
    let walker = WalkBuilder::new(&repo_path)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Avoid indexing Prune artifacts even if they aren't ignored.
        let path_str = path.to_string_lossy();
        if path_str.contains("/.ce/") || path_str.contains("/.prune/") {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let lang: Option<&str> = match ext {
                "rs" => Some("rust"),
                "swift" => Some("swift"),
                // TypeScript + friends
                "ts" | "mts" | "cts" | "js" => Some("ts"),
                "tsx" | "jsx" => Some("tsx"),
                _ => None,
            };

            if let Some(lang) = lang {
                files.push(CandidateFile {
                    disk_path: path.to_path_buf(),
                    language: lang.to_string(),
                });
                if files.len() >= max_files {
                    truncated = true;
                    break;
                }
            }
        }
    }

    let n_rust = files.iter().filter(|f| f.language == "rust").count();
    let n_swift = files.iter().filter(|f| f.language == "swift").count();
    let n_ts = files.iter().filter(|f| f.language == "ts").count();
    let n_tsx = files.iter().filter(|f| f.language == "tsx").count();
    println!(
        "Indexing {} files (rust={}, swift={}, ts={}, tsx={})…",
        files.len(),
        n_rust,
        n_swift,
        n_ts,
        n_tsx
    );

    #[derive(Clone)]
    struct FileInfo {
        /// Actual path on disk (absolute or relative, as returned by the walker).
        disk_path: PathBuf,
        /// Repo-relative, normalized path used as the stable key in the DB.
        index_path: PathBuf,
        index_path_str: String,
        language: String,
        src: String,
        size_bytes: i64,
        mtime_ms: i64,
        content_hash: String,
    }

    // Read all candidate files and compute a cheap whole-file hash.
    // We use this to skip unchanged files (unless --full).
    let file_infos: Vec<FileInfo> = files
        .par_iter()
        .filter_map(|p| {
            let disk_path = p.disk_path.clone();
            let language = p.language.clone();

            // Store repo-relative paths in the DB so the index is stable across clones.
            let rel = disk_path
                .strip_prefix(&repo_path)
                .unwrap_or(disk_path.as_path());
            let index_path_str = rel.to_string_lossy().to_string().replace('\\', "/");
            let index_path = PathBuf::from(&index_path_str);

            let src = fs::read_to_string(&disk_path).ok()?;
            let size_bytes = src.len() as i64;

            let mtime_ms = fs::metadata(&disk_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            let content_hash =
                ce_core::util::hash_text_hex(&ce_core::util::normalize_whitespace(&src));

            Some(FileInfo {
                disk_path,
                index_path,
                index_path_str,
                language,
                src,
                size_bytes,
                mtime_ms,
                content_hash,
            })
        })
        .collect();

    let mut scanned: HashSet<String> = HashSet::new();
    let mut to_index: Vec<FileInfo> = Vec::new();

    for fi in file_infos {
        scanned.insert(fi.index_path_str.clone());
        if full {
            to_index.push(fi);
            continue;
        }

        let existing = db.get_file_info(&fi.index_path_str)?;
        if let Some((_file_id, old_hash)) = existing {
            if old_hash == fi.content_hash {
                continue; // unchanged
            }
        }
        to_index.push(fi);
    }

    println!(
        "Files to (re)index: {} (full={}, prune={})",
        to_index.len(),
        full,
        prune
    );

    // Parse only changed files (in parallel) into fragments.
    let parsed: Vec<(FileInfo, Vec<ce_core::model::Fragment>)> = to_index
        .par_iter()
        .filter_map(|fi| {
            let lang = fi.language.as_str();

            let mut frags: Vec<ce_core::model::Fragment> = match lang {
                "rust" => {
                    let mut adapter = RustAdapter::new().ok()?;
                    let tree = adapter.parse(&fi.src).ok()?;
                    adapter.extract_fragments(&fi.index_path, &fi.src, &tree)
                }
                "swift" => {
                    let mut adapter = SwiftAdapter::new().ok()?;
                    let tree = adapter.parse(&fi.src).ok()?;
                    adapter.extract_fragments(&fi.index_path, &fi.src, &tree)
                }
                "ts" => {
                    let mut adapter = TsReactAdapter::new_ts().ok()?;
                    let tree = adapter.parse(&fi.src).ok()?;
                    adapter.extract_fragments(&fi.index_path, &fi.src, &tree)
                }
                "tsx" => {
                    let mut adapter = TsReactAdapter::new_tsx().ok()?;
                    let tree = adapter.parse(&fi.src).ok()?;
                    adapter.extract_fragments(&fi.index_path, &fi.src, &tree)
                }
                _ => return None,
            };

            // Language-agnostic file-level ApiSummary (generated at index time).
            let file_refs: Vec<String> = match lang {
                "rust" => ce_lang_rust::collect_file_level_refs(&fi.src),
                "swift" => ce_lang_swift::collect_file_level_refs(&fi.src),
                "ts" | "tsx" => ce_lang_tsreact::collect_file_level_refs(&fi.src),
                _ => Vec::new(),
            };

            if let Some(api) = ce_core::api_summary::build_api_summary(
                &fi.index_path,
                lang,
                &fi.src,
                &frags,
                &file_refs,
                &ce_core::api_summary::ApiSummaryOptions::default(),
            ) {
                frags.push(api);
            }

            Some((fi.clone(), frags))
        })
        .collect();

    let mut inserted_fragments = 0usize;
    let mut touched_file_ids: Vec<i64> = Vec::new();

    // Insert into DB and embed.
    for (fi, frags) in parsed {
        let file_id = db.upsert_file(
            &fi.index_path_str,
            &fi.language,
            fi.size_bytes,
            fi.mtime_ms,
            &fi.content_hash,
        )?;
        touched_file_ids.push(file_id);
        // Clear existing fragments for this file to avoid stale index entries.
        db.delete_fragments_by_file_id(file_id)?;

        // Embed in small batches for this file.
        let texts: Vec<String> = frags.iter().map(|f| f.retrieval_text.clone()).collect();
        let vectors = if texts.is_empty() {
            vec![]
        } else {
            embedder.embed_passages(&texts)?
        };

        for (frag, vec) in frags.into_iter().zip(vectors.into_iter()) {
            let rowid = db.upsert_fragment(file_id, &frag)?;
            db.replace_symbols_for_fragment(rowid, &frag)?;
            db.replace_refs_for_fragment(rowid, &frag.refs)?;

            let dim = vec.len() as i64;
            let blob: Vec<u8> = bytemuck::cast_slice::<f32, u8>(&vec).to_vec();
            db.insert_embedding(rowid, embedder.model_name(), dim, &blob)?;
            inserted_fragments += 1;
        }
    }

    touched_file_ids.sort();
    touched_file_ids.dedup();

    // Optionally prune files that disappeared from disk.
    if prune {
        if truncated {
            eprintln!(
                "warning: --prune was requested but file scan was truncated by --max-files; skipping prune to avoid deleting valid entries.",
            );
        } else {
            let mut removed = 0usize;
            for lang in ["rust", "swift", "ts", "tsx"] {
                let existing = db.list_files_by_language(lang)?;
                for (file_id, path) in existing {
                    if !scanned.contains(&path) {
                        db.delete_file_by_id(file_id)?;
                        removed += 1;
                    }
                }
            }
            if removed > 0 {
                println!("Pruned {removed} stale files from index.");
            }
        }
    }

    // Rebuild resolved edges (refers) for subgraph expansion.
    if !skip_edges {
        let max_refs_per_fragment = 64;
        let max_defs_per_ref = 4;
        touched_file_ids.sort();
        touched_file_ids.dedup();

        // Build lightweight Rust module graph edges (`mod`/`use`) between file-level ApiSummary nodes.
        // These help graph expansion answer “why this file?” and pull in related modules.
        let m = db.rebuild_rust_module_edges_all(repo_path.as_path())?;
        if m > 0 {
            println!("Rebuilt Rust module/import edges: {m}");
        }

        // Build TypeScript/TSX import edges between file-level ApiSummary nodes.
        let tm = db.rebuild_ts_module_edges_all(repo_path.as_path())?;
        if tm > 0 {
            println!("Rebuilt TypeScript import edges: {tm}");
        }

        // If the indexer only touched a small number of files, do an incremental rebuild
        // (much faster for iterative agent loops). Fall back to full rebuild when
        // a large portion of the repo changes.
        let do_full = full || touched_file_ids.len() > 256;
        if do_full {
            let n = db.rebuild_ref_edges_all(max_refs_per_fragment, max_defs_per_ref)?;
            println!("Rebuilt resolved edges (full): {n}");
        } else {
            let n = db.rebuild_ref_edges_incremental(
                &touched_file_ids,
                max_refs_per_fragment,
                max_defs_per_ref,
            )?;
            println!(
                "Rebuilt resolved edges (incremental; touched_files={}): {n}",
                touched_file_ids.len()
            );
        }
    } else {
        println!("Skipped edge rebuild (--skip-edges)");
    }

    // Dump HNSW index used by `ce search`, `ce pack`, `ce eval`, and the MCP server.
    // We rebuild from DB to include embeddings for skipped (unchanged) files.
    //
    // Before dumping, persist repo/embedding state metadata so later processes can
    // do fast, reliable staleness checks for the dump.
    let repo_hash = db.update_repo_meta()?;
    let emb_meta = db.update_embeddings_meta()?;

    let (vec_index, _map) = query::build_hnsw_from_db(&db)?;
    let dump_base = "fragments";
    let dump_path = vec_index.dump(Path::new(hnsw_dir), dump_base)?;
    println!("HNSW dumped at base path: {dump_path}");

    // Record dump metadata in DB meta table (model/dim/hash).
    db.set_meta("hnsw.base", dump_base)?;
    db.set_meta_usize("hnsw.nb_points", emb_meta.total_count)?;
    db.set_meta_usize("hnsw.dim", emb_meta.dim)?;
    db.set_meta("hnsw.model", &emb_meta.model)?;
    db.set_meta("hnsw.repo_state_hash", &repo_hash)?;
    db.set_meta("hnsw.embeddings_state_hash", &emb_meta.state_hash)?;
    db.set_meta_i64("hnsw.dump_created_at_ms", Db::now_ms())?;

    println!("Done. Inserted/updated fragments: {inserted_fragments}");
    Ok(())
}

fn cmd_search(db_path: &str, hnsw_dir: &str, query: &str, k: usize, alpha: f32) -> Result<()> {
    let db = Db::open(db_path)?;
    let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;

    // Load a persisted HNSW dump if available; otherwise rebuild once and dump it.
    let vec_index =
        query::load_or_build_hnsw(&db, Path::new(hnsw_dir), query::DEFAULT_HNSW_BASE, false)?;

    let hits =
        query::hybrid_search_with_index(&db, &embedder, Some(&vec_index), query, 50, 50, k, alpha)?;
    for h in hits {
        println!("\n[{:.3}] {} {:?} {}", h.score, h.frag_id, h.kind, h.path);
        if let Some(sym) = &h.symbol {
            println!("  symbol: {sym}");
        }
        println!("  {}", indent(&h.signature.trim(), 2));
    }

    Ok(())
}

fn cmd_graph_report(
    db_path: &str,
    out_path: Option<&str>,
    max_hubs: usize,
    max_edges: usize,
) -> Result<()> {
    let db = Db::open(db_path)?;
    let report = db.graph_report(GraphReportOptions {
        max_hubs,
        max_edges,
    })?;
    let markdown = report.to_markdown();

    if let Some(out_path) = out_path {
        fs::write(out_path, markdown)?;
        println!("wrote graph report to {out_path}");
    } else {
        print!("{markdown}");
    }

    Ok(())
}

fn cmd_pack(
    db_path: &str,
    hnsw_dir: &str,
    task: &str,
    strategy_id: Option<&str>,
    strategy_file: Option<&str>,
    strategy_json: Option<&str>,
    strategy_toml: Option<&str>,
    budget_chars: Option<usize>,
    budget_tokens: Option<usize>,
    tokenizer: Option<&str>,
    max_bodies: Option<usize>,
    alpha: Option<f32>,
    format: PackFormat,
) -> Result<()> {
    let db = Db::open(db_path)?;
    let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;

    let automatic = strategy_id.is_none()
        && strategy_file.is_none()
        && strategy_json.is_none()
        && strategy_toml.is_none();
    let (mut strategy, selection) = if automatic {
        ce_core::strategy_select::select_strategy(task, &repository_signals(&db)?)
    } else {
        (
            load_strategy_for_pack(
                &db,
                strategy_id,
                strategy_file,
                strategy_json,
                strategy_toml,
            )?,
            ce_core::model::StrategySelection {
                task_class: "manual".into(),
                repository_archetype: "manual".into(),
                confidence: 1.0,
                reason: "explicit strategy supplied".into(),
            },
        )
    };

    // Apply per-invocation overrides (if provided)
    if let Some(b) = budget_chars {
        strategy.budget_chars = b;
    }
    if let Some(bt) = budget_tokens {
        strategy.budget_tokens = Some(bt);
    }
    if let Some(tk) = tokenizer {
        strategy.tokenizer = tk.to_string();
    }
    if let Some(m) = max_bodies {
        strategy.max_bodies = m;
    }
    if let Some(a) = alpha {
        strategy.hybrid_alpha = a;
    }

    // If we didn't load a strategy (defaults), ensure per-CLI defaults.
    // This keeps backward-friendly behavior when no strategy is supplied.
    if strategy_id.is_none()
        && strategy_file.is_none()
        && strategy_json.is_none()
        && strategy_toml.is_none()
    {
        strategy.budget_chars = strategy.budget_chars.max(2000);
    }

    // Load a persisted HNSW dump if available; otherwise rebuild once and dump it.
    let vec_index =
        query::load_or_build_hnsw(&db, Path::new(hnsw_dir), query::DEFAULT_HNSW_BASE, false)?;

    let mut pack = build_pack(&db, &embedder, Some(&vec_index), task, &strategy, None)?;
    pack.strategy_selection = Some(selection);

    match format {
        PackFormat::Text => {
            println!("{}", render_pack(&pack));
        }
        PackFormat::Json => {
            println!("{}", json::to_string_pretty(&pack)?);
        }
        PackFormat::Both => {
            println!("{}", render_pack(&pack));
            println!("\n---\n");
            println!("{}", json::to_string_pretty(&pack)?);
        }
    }
    Ok(())
}

fn repository_signals(db: &Db) -> Result<ce_core::strategy_select::RepositorySignals> {
    Ok(ce_core::strategy_select::RepositorySignals {
        rust_files: db.list_files_by_language("rust")?.len(),
        ts_files: db.list_files_by_language("ts")?.len(),
        tsx_files: db.list_files_by_language("tsx")?.len(),
        swift_files: db.list_files_by_language("swift")?.len(),
    })
}

fn cmd_eval(
    db_path: &str,
    hnsw_dir: &str,
    tasks_path: &str,
    strategy_id: Option<&str>,
    strategy_file: Option<&str>,
    strategy_json: Option<&str>,
    strategy_toml: Option<&str>,
    limit: usize,
    out_path: Option<&str>,
) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};

    let db = Db::open(db_path)?;
    let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;
    let strategy = load_strategy_for_pack(
        &db,
        strategy_id,
        strategy_file,
        strategy_json,
        strategy_toml,
    )?;

    // Load a persisted HNSW dump if available; otherwise rebuild once and dump it.
    let vec_index =
        query::load_or_build_hnsw(&db, Path::new(hnsw_dir), query::DEFAULT_HNSW_BASE, false)?;

    let f = fs::File::open(tasks_path)?;
    let reader = BufReader::new(f);

    let mut out_file = if let Some(p) = out_path {
        let f = fs::File::create(p)?;
        Some(std::io::BufWriter::new(f))
    } else {
        None
    };

    let mut total = 0usize;
    let mut path_cases = 0usize;
    let mut path_hits = 0usize;
    let mut sym_cases = 0usize;
    let mut sym_hits = 0usize;
    let mut sum_used_chars = 0usize;
    let mut sum_used_tokens = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut sum_baseline_tokens = 0usize;
    let mut baseline_cases = 0usize;
    let mut sum_saved_pct = 0f64;
    let mut sum_unbound = 0usize;
    let mut sum_iterations = 0f64;
    let mut iteration_cases = 0usize;
    let mut sum_redundancy = 0f64;
    let mut redundancy_cases = 0usize;

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if limit > 0 && total >= limit {
            break;
        }

        let v: json::Value = json::from_str(line)?;
        let t = crate::tasks::parse_eval_task(&v)?;

        let mut pack = build_pack(
            &db,
            &embedder,
            Some(&vec_index),
            &t.task,
            &strategy,
            Some(&seen_ids),
        )?;

        let repeated = pack
            .items
            .iter()
            .filter(|it| seen_ids.contains(&it.id))
            .count();
        let redundancy_pct = if pack.items.is_empty() {
            0.0
        } else {
            (repeated as f32 / pack.items.len() as f32) * 100.0
        };
        pack.metrics.redundancy_pct = Some(redundancy_pct);
        sum_redundancy += redundancy_pct as f64;
        redundancy_cases += 1;
        for it in &pack.items {
            seen_ids.insert(it.id.clone());
        }

        let path_hit = if t.expect_paths.is_empty() {
            None
        } else {
            path_cases += 1;
            let hit = t.expect_paths.iter().any(|p| {
                pack.items
                    .iter()
                    .any(|it| it.path == *p || it.path.ends_with(p))
            });
            if hit {
                path_hits += 1;
            }
            Some(hit)
        };

        if let Some(hit) = path_hit {
            pack.metrics.hit_rate_paths = Some(if hit { 1.0 } else { 0.0 });
        }

        let sym_hit = if t.expect_symbols.is_empty() {
            None
        } else {
            sym_cases += 1;
            let hit = t.expect_symbols.iter().any(|s| {
                pack.items
                    .iter()
                    .any(|it| it.symbol.as_deref() == Some(s.as_str()) || it.content.contains(s))
            });
            if hit {
                sym_hits += 1;
            }
            Some(hit)
        };

        if let Some(iters) = t.iterations {
            pack.metrics.avg_iterations_per_fix = Some(iters);
            sum_iterations += iters as f64;
            iteration_cases += 1;
        }

        total += 1;
        sum_used_chars += pack.used_chars;
        sum_used_tokens += pack.used_tokens;
        sum_unbound += pack.metrics.unbound_symbol_count;
        if let Some(b) = pack.metrics.baseline_tokens_total {
            sum_baseline_tokens += b;
            baseline_cases += 1;
            if let Some(saved) = pack.metrics.saved_pct {
                sum_saved_pct += saved as f64;
            }
        }

        if let Some(w) = out_file.as_mut() {
            let top_paths: Vec<String> = pack
                .items
                .iter()
                .take(8)
                .map(|it| it.path.clone())
                .collect();
            let r = json::json!({
                "id": t.id,
                "path_hit": path_hit,
                "symbol_hit": sym_hit,
                "hit_rate_paths": pack.metrics.hit_rate_paths,
                "avg_iterations_per_fix": pack.metrics.avg_iterations_per_fix,
                "redundancy_pct": pack.metrics.redundancy_pct,
                "used_chars": pack.used_chars,
                "used_tokens": pack.used_tokens,
                "baseline_tokens": pack.metrics.baseline_tokens_total,
                "saved_pct": pack.metrics.saved_pct,
                "unbound_symbol_count": pack.metrics.unbound_symbol_count,
                "n_items": pack.items.len(),
                "top_paths": top_paths,
            });
            writeln!(w, "{}", json::to_string(&r)?)?;
        }
    }

    println!("[ce-eval v0]");
    println!("tasks: {}", total);
    if total > 0 {
        println!(
            "avg_used_chars: {:.1}",
            sum_used_chars as f64 / total as f64
        );
        println!(
            "avg_used_tokens: {:.1}",
            sum_used_tokens as f64 / total as f64
        );
        println!(
            "avg_unbound_symbol_count: {:.2}",
            sum_unbound as f64 / total as f64
        );
        if baseline_cases > 0 {
            println!(
                "avg_baseline_tokens: {:.1}",
                sum_baseline_tokens as f64 / baseline_cases as f64
            );
            println!(
                "avg_saved_pct: {:.1}",
                sum_saved_pct / baseline_cases as f64
            );
        }
    }
    if path_cases > 0 {
        println!(
            "path_hit_rate: {:.3} ({}/{})",
            path_hits as f64 / path_cases as f64,
            path_hits,
            path_cases
        );
    } else {
        println!("path_hit_rate: n/a (no expect_paths)");
    }
    if iteration_cases > 0 {
        println!(
            "avg_iterations_per_fix: {:.2}",
            sum_iterations / iteration_cases as f64
        );
    } else {
        println!("avg_iterations_per_fix: n/a (no iterations)");
    }
    if redundancy_cases > 0 {
        println!(
            "avg_redundancy_pct: {:.1}",
            sum_redundancy / redundancy_cases as f64
        );
    } else {
        println!("avg_redundancy_pct: n/a (no tasks)");
    }
    if sym_cases > 0 {
        println!(
            "symbol_hit_rate: {:.3} ({}/{})",
            sym_hits as f64 / sym_cases as f64,
            sym_hits,
            sym_cases
        );
    } else {
        println!("symbol_hit_rate: n/a (no expect_symbols)");
    }

    if let Some(p) = out_path {
        println!("wrote per-task results: {p}");
    }

    Ok(())
}

fn cmd_recipe(cmd: RecipeCmd) -> Result<()> {
    match cmd {
        RecipeCmd::Add {
            db,
            failure,
            failure_file,
            pack_summary,
            patch_meta,
            tags,
            success_tokens,
            iterations,
        } => cmd_recipe_add(
            &db,
            failure,
            failure_file,
            pack_summary,
            patch_meta,
            tags,
            success_tokens,
            iterations,
        ),
        RecipeCmd::List { db, limit, offset } => cmd_recipe_list(&db, limit, offset),
        RecipeCmd::Export { db, out } => cmd_recipe_export(&db, &out),
    }
}

fn cmd_memory(cmd: MemoryCmd) -> Result<()> {
    match cmd {
        MemoryCmd::Add {
            db,
            kind,
            title,
            content,
            path,
            tags,
        } => {
            let db = Db::open(&db)?;
            let tokens = ce_core::util::failure_tokens(&format!(
                "{title} {content} {}",
                path.as_deref().unwrap_or_default()
            ))
            .join(" ");
            let id = db.add_repository_memory(
                &kind,
                &title,
                &content,
                &tokens,
                path.as_deref(),
                tags.as_deref(),
            )?;
            println!("memory_id: {id}");
        }
        MemoryCmd::List {
            db,
            kind,
            limit,
            offset,
        } => {
            let db = Db::open(&db)?;
            for record in db.list_repository_memory(kind.as_deref(), limit, offset)? {
                println!(
                    "- #{} [{}] {} path={} tags={}\n  {}",
                    record.memory_id,
                    record.kind,
                    record.title,
                    record.path.as_deref().unwrap_or("-"),
                    record.tags.as_deref().unwrap_or("-"),
                    record.content
                );
            }
        }
    }
    Ok(())
}

fn cmd_recipe_add(
    db_path: &str,
    failure: Option<String>,
    failure_file: Option<String>,
    pack_summary: String,
    patch_meta: String,
    tags: Option<String>,
    success_tokens: Option<i64>,
    iterations: Option<i64>,
) -> Result<()> {
    if failure.is_some() && failure_file.is_some() {
        return Err(anyhow!("use either --failure or --failure_file (not both)"));
    }

    let failure_text = if let Some(text) = failure {
        text
    } else if let Some(path) = failure_file {
        fs::read_to_string(&path).map_err(|e| anyhow!("failed to read failure_file {path}: {e}"))?
    } else {
        return Err(anyhow!("missing --failure or --failure_file"));
    };

    let fingerprint = ce_core::util::fingerprint_failure(&failure_text);
    let fingerprint_hash = ce_core::util::hash_text_hex(&fingerprint);
    let tokens = ce_core::util::failure_tokens(&failure_text).join(" ");

    let excerpt = failure_text.lines().take(12).collect::<Vec<_>>().join("\n");
    let failure_excerpt = truncate_line(&excerpt, 400);

    let pack_summary = pack_summary.trim().to_string();
    let patch_meta = patch_meta.trim().to_string();

    let tags = tags.and_then(|t| {
        let cleaned: Vec<String> = t
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.join(","))
        }
    });

    let db = Db::open(db_path)?;
    let recipe_id = db.add_recipe(
        &fingerprint,
        &fingerprint_hash,
        &tokens,
        &failure_excerpt,
        &pack_summary,
        &patch_meta,
        tags.as_deref(),
        success_tokens,
        iterations,
    )?;

    println!("added recipe {recipe_id}");
    Ok(())
}

fn cmd_recipe_list(db_path: &str, limit: usize, offset: usize) -> Result<()> {
    let db = Db::open(db_path)?;
    let recipes = db.list_recipes(limit, offset)?;

    for rec in recipes {
        let tags = rec.tags.as_deref().unwrap_or("-");
        println!("#{} {} tags={}", rec.recipe_id, rec.created_at_ms, tags);
        println!("  failure: {}", rec.failure_excerpt);
        if !rec.pack_summary.trim().is_empty() {
            println!("  pack: {}", rec.pack_summary);
        }
        if !rec.patch_meta.trim().is_empty() {
            println!("  patch: {}", rec.patch_meta);
        }
        if let Some(tokens) = rec.success_tokens {
            println!("  success_tokens: {}", tokens);
        }
        if let Some(iters) = rec.iterations {
            println!("  iterations: {}", iters);
        }
    }

    Ok(())
}

fn cmd_recipe_export(db_path: &str, out_path: &str) -> Result<()> {
    use std::io::Write;

    let db = Db::open(db_path)?;
    let recipes = db.load_recipes(2000)?;

    let mut file = std::io::BufWriter::new(fs::File::create(out_path)?);
    for rec in recipes {
        writeln!(file, "{}", json::to_string(&rec)?)?;
    }

    println!("exported recipes to {out_path}");
    Ok(())
}

// -----------------------------------------------------------------------------
// Task utilities (SWE-bench-ish -> eval JSONL)
// -----------------------------------------------------------------------------

fn cmd_tasks(cmd: TasksCmd) -> Result<()> {
    match cmd {
        TasksCmd::ImportSweBench {
            input,
            out,
            limit,
            derive_symbols,
        } => cmd_tasks_import_swebench(&input, &out, limit, derive_symbols),
        TasksCmd::RunSweBench {
            input,
            workspace,
            out,
            instance_id,
            agent_command,
            harness_command,
            run_id,
            dry_run,
        } => stage_b::run(stage_b::StageBOptions {
            input: input.into(),
            workspace: workspace.into(),
            out: out.into(),
            instance_id,
            agent_command,
            harness_command,
            run_id,
            dry_run,
        }),
    }
}

fn cmd_tasks_import_swebench(
    input: &str,
    out: &str,
    limit: usize,
    derive_symbols: bool,
) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};

    let in_file = fs::File::open(input)?;
    let mut writer = std::io::BufWriter::new(fs::File::create(out)?);

    let mut total = 0usize;

    let opts = crate::tasks::ParseOptions { derive_symbols };

    if input.ends_with(".jsonl") {
        let reader = BufReader::new(in_file);
        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if limit > 0 && total >= limit {
                break;
            }

            let v: json::Value = json::from_str(line)?;
            let t = crate::tasks::parse_eval_task_with(&v, opts)?;
            let out_obj = build_imported_task_object(&v, t);
            writeln!(writer, "{}", json::to_string(&out_obj)?)?;
            total += 1;
        }
    } else {
        // Assume JSON array.
        let v: json::Value = json::from_reader(in_file)?;
        let arr = v
            .as_array()
            .ok_or_else(|| anyhow!("expected JSON array at {input}"))?;
        for item in arr {
            if limit > 0 && total >= limit {
                break;
            }
            let t = crate::tasks::parse_eval_task_with(item, opts)?;
            let out_obj = build_imported_task_object(item, t);
            writeln!(writer, "{}", json::to_string(&out_obj)?)?;
            total += 1;
        }
    }

    println!("wrote {total} tasks -> {out}");
    Ok(())
}

fn build_imported_task_object(src: &json::Value, t: crate::tasks::EvalTask) -> json::Value {
    use serde_json::{Map, Value};

    let mut m = Map::new();

    if let Some(id) = t.id {
        m.insert("id".to_string(), Value::String(id));
    } else if let Some(id) = src.get("instance_id").and_then(|x| x.as_str()) {
        m.insert("id".to_string(), Value::String(id.to_string()));
    }

    m.insert("task".to_string(), Value::String(t.task));
    m.insert(
        "expect_paths".to_string(),
        Value::Array(t.expect_paths.into_iter().map(Value::String).collect()),
    );
    m.insert(
        "expect_symbols".to_string(),
        Value::Array(t.expect_symbols.into_iter().map(Value::String).collect()),
    );
    if let Some(iters) = t.iterations {
        m.insert("iterations".to_string(), Value::from(iters as f64));
    }

    // Preserve a few common metadata fields if present.
    for k in ["repo", "base_commit", "instance_id", "url"] {
        if let Some(s) = src.get(k).and_then(|x| x.as_str()) {
            m.insert(k.to_string(), Value::String(s.to_string()));
        }
    }

    Value::Object(m)
}

// -----------------------------------------------------------------------------
// Strategy evolution (simple DGM-ish hillclimber)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct EvalSummary {
    total: usize,
    path_cases: usize,
    path_hits: usize,
    sym_cases: usize,
    sym_hits: usize,
    avg_used_chars: f64,
    avg_used_tokens: f64,
    avg_latency_ms: f64,
    redundancy_pct: f64,
    missing_definition_risk: f64,
    score: f64,
}

impl EvalSummary {
    fn objectives(&self) -> ce_core::evolution::ObjectiveVector {
        let cases = self.path_cases + self.sym_cases;
        let hits = self.path_hits + self.sym_hits;
        ce_core::evolution::ObjectiveVector {
            resolved_rate: if cases == 0 {
                0.0
            } else {
                hits as f64 / cases as f64
            },
            tokens: self.avg_used_tokens,
            latency_ms: self.avg_latency_ms,
            redundancy_pct: self.redundancy_pct,
            missing_definition_risk: self.missing_definition_risk,
        }
    }
}

fn eval_strategy(
    db: &Db,
    embedder: &Embedder,
    vec_index: &ce_store::VecIndex,
    tasks_path: &str,
    strategy: &StrategyConfig,
    limit: usize,
) -> Result<EvalSummary> {
    use std::io::{BufRead, BufReader};

    let f = fs::File::open(tasks_path)?;
    let reader = BufReader::new(f);

    let mut total = 0usize;
    let mut path_cases = 0usize;
    let mut path_hits = 0usize;
    let mut sym_cases = 0usize;
    let mut sym_hits = 0usize;
    let mut sum_used_chars = 0usize;
    let mut sum_used_tokens = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut repeated_items = 0usize;
    let mut packed_items = 0usize;
    let mut missing_definitions = 0usize;
    let mut latency_ms = 0.0f64;

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if limit > 0 && total >= limit {
            break;
        }

        let v: json::Value = json::from_str(line)?;
        let t = crate::tasks::parse_eval_task(&v)?;

        let started = std::time::Instant::now();
        let pack = build_pack(
            db,
            embedder,
            Some(vec_index),
            &t.task,
            strategy,
            Some(&seen_ids),
        )?;
        latency_ms += started.elapsed().as_secs_f64() * 1_000.0;
        missing_definitions += pack.metrics.unbound_symbol_count;

        if !t.expect_paths.is_empty() {
            path_cases += 1;
            let hit = t.expect_paths.iter().any(|p| {
                pack.items
                    .iter()
                    .any(|it| it.path == *p || it.path.ends_with(p))
            });
            if hit {
                path_hits += 1;
            }
        }

        if !t.expect_symbols.is_empty() {
            sym_cases += 1;
            let hit = t.expect_symbols.iter().any(|s| {
                pack.items
                    .iter()
                    .any(|it| it.symbol.as_deref() == Some(s.as_str()) || it.content.contains(s))
            });
            if hit {
                sym_hits += 1;
            }
        }

        total += 1;
        sum_used_chars += pack.used_chars;
        sum_used_tokens += pack.used_tokens;
        for it in &pack.items {
            packed_items += 1;
            if !seen_ids.insert(it.id.clone()) {
                repeated_items += 1;
            }
        }
    }

    let avg_used_chars = if total == 0 {
        0.0
    } else {
        sum_used_chars as f64 / total as f64
    };
    let avg_used_tokens = if total == 0 {
        0.0
    } else {
        sum_used_tokens as f64 / total as f64
    };

    let path_hr = if path_cases == 0 {
        0.0
    } else {
        path_hits as f64 / path_cases as f64
    };
    let sym_hr = if sym_cases == 0 {
        0.0
    } else {
        sym_hits as f64 / sym_cases as f64
    };

    // Fitness: prioritize retrieval correctness, lightly penalize token usage.
    let score = (0.6 * path_hr + 0.4 * sym_hr) - (avg_used_tokens / 200_000.0);
    let redundancy_pct = if packed_items == 0 {
        0.0
    } else {
        repeated_items as f64 * 100.0 / packed_items as f64
    };
    let missing_definition_risk = if total == 0 {
        0.0
    } else {
        missing_definitions as f64 / total as f64
    };

    Ok(EvalSummary {
        total,
        path_cases,
        path_hits,
        sym_cases,
        sym_hits,
        avg_used_chars,
        avg_used_tokens,
        avg_latency_ms: if total == 0 {
            0.0
        } else {
            latency_ms / total as f64
        },
        redundancy_pct,
        missing_definition_risk,
        score,
    })
}

fn cmd_strategy_evolve(
    db_path: &str,
    tasks_path: &str,
    base_strategy_id: Option<&str>,
    base_strategy_file: Option<&str>,
    base_strategy_json: Option<&str>,
    base_strategy_toml: Option<&str>,
    generations: usize,
    population: usize,
    limit: usize,
    seed: u64,
    name_prefix: &str,
) -> Result<()> {
    let db = Db::open(db_path)?;
    let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;

    // Build HNSW once for the whole evolution run.
    let (vec_index, _map) = query::build_hnsw_from_db(&db)?;

    let base = load_strategy_for_pack(
        &db,
        base_strategy_id,
        base_strategy_file,
        base_strategy_json,
        base_strategy_toml,
    )?;

    let mut rng = Rng64::new(seed);

    let base_eval = eval_strategy(&db, &embedder, &vec_index, tasks_path, &base, limit)?;
    println!("[ce-evolve v0]");
    println!(
        "base: score={:.4} path_hr={:.3} sym_hr={:.3} avg_tokens={:.1}",
        base_eval.score,
        if base_eval.path_cases == 0 {
            0.0
        } else {
            base_eval.path_hits as f64 / base_eval.path_cases as f64
        },
        if base_eval.sym_cases == 0 {
            0.0
        } else {
            base_eval.sym_hits as f64 / base_eval.sym_cases as f64
        },
        base_eval.avg_used_tokens,
    );

    let mut archive: Vec<(StrategyConfig, EvalSummary, Option<String>)> = vec![(
        base.clone(),
        base_eval.clone(),
        base_strategy_id.map(str::to_string),
    )];

    let generations = generations.max(1);
    let population = population.max(1);

    for gen in 0..generations {
        let archive_objectives: Vec<_> = archive
            .iter()
            .map(|(_, eval, _)| eval.objectives())
            .collect();
        let parent_front = ce_core::evolution::pareto_front(&archive_objectives);
        let parents: Vec<StrategyConfig> = parent_front
            .iter()
            .map(|&index| archive[index].0.clone())
            .collect();
        let mut generation: Vec<(StrategyConfig, EvalSummary)> = Vec::new();

        for i in 0..population {
            let cfg = if i == 0 {
                parents[0].clone()
            } else {
                let left = &parents[rng.range_usize(0, parents.len() - 1)];
                let right = &parents[rng.range_usize(0, parents.len() - 1)];
                let child = ce_core::evolution::crossover_strategy(left, right, rng.next_u64());
                mutate_strategy(&child, &mut rng)
            };

            let ev = eval_strategy(&db, &embedder, &vec_index, tasks_path, &cfg, limit)?;
            generation.push((cfg, ev));
        }

        let objectives: Vec<_> = generation
            .iter()
            .map(|(_, eval)| eval.objectives())
            .collect();
        let front = ce_core::evolution::pareto_front(&objectives);
        let mut next_archive = Vec::new();
        for (rank, &index) in front.iter().enumerate() {
            let (cfg, eval) = generation[index].clone();
            let cfg_json = json::to_string(&cfg)?;
            let strategy_id = ce_core::util::hash_text_hex(&cfg_json);
            let name = format!("{}_g{:02}_p{:02}", name_prefix, gen, rank);
            db.upsert_strategy(&strategy_id, &name, &cfg_json, None, Some(eval.score))?;
            next_archive.push((cfg, eval, Some(strategy_id)));
        }
        next_archive.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        next_archive.truncate(population.max(2));
        let champion = &next_archive[0];
        let objectives = champion.1.objectives();
        println!(
            "gen {:02}: pareto={} resolved={:.3} tokens={:.1} latency_ms={:.1} redundancy={:.1}% missing_risk={:.2} id={}",
            gen,
            next_archive.len(),
            objectives.resolved_rate,
            objectives.tokens,
            objectives.latency_ms,
            objectives.redundancy_pct,
            objectives.missing_definition_risk,
            short_id(champion.2.as_deref().unwrap_or_default())
        );
        archive = next_archive;
    }

    let (best_cfg, _best_eval, best_id) = archive
        .iter()
        .max_by(|a, b| {
            a.1.score
                .partial_cmp(&b.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("evolution archive is non-empty");
    if let Some(id) = best_id {
        println!("best_strategy_id: {id}");
    }
    println!("best_strategy_json:\n{}", json::to_string_pretty(best_cfg)?);
    Ok(())
}

fn cmd_strategy(cmd: StrategyCmd) -> Result<()> {
    match cmd {
        StrategyCmd::Add {
            db,
            name,
            config,
            config_json,
            config_toml,
            parent_id,
        } => {
            let db = Db::open(&db)?;
            let cfg = load_strategy_from_sources(
                config.as_deref(),
                config_json.as_deref(),
                config_toml.as_deref(),
            )?;
            let cfg_json = json::to_string(&cfg)?;
            let id = db.add_strategy(&name, &cfg_json, parent_id.as_deref())?;
            println!("strategy_id: {id}");
            Ok(())
        }
        StrategyCmd::List {
            db,
            limit,
            offset,
            show_config,
        } => {
            let db = Db::open(&db)?;
            let rows = db.list_strategies(limit, offset)?;
            for r in rows {
                println!(
                    "- {}  name=\"{}\"  score={:?}  parent={:?}  created_at_ms={}",
                    short_id(&r.strategy_id),
                    r.name,
                    r.score,
                    r.parent_id,
                    r.created_at_ms
                );
                if show_config {
                    println!("{}", indent(&r.config_json, 2));
                }
            }
            Ok(())
        }
        StrategyCmd::Get { db, id, pretty } => {
            let db = Db::open(&db)?;
            let Some(r) = db.get_strategy(&id)? else {
                return Err(anyhow!("strategy not found: {id}"));
            };
            println!("strategy_id: {}", r.strategy_id);
            println!("name: {}", r.name);
            println!("parent_id: {:?}", r.parent_id);
            println!("score: {:?}", r.score);
            println!("created_at_ms: {}", r.created_at_ms);
            println!("config:");
            if pretty {
                let v: json::Value = json::from_str(&r.config_json)?;
                println!("{}", json::to_string_pretty(&v)?);
            } else {
                println!("{}", r.config_json);
            }
            Ok(())
        }

        StrategyCmd::Evolve {
            db,
            tasks,
            base_strategy_id,
            base_strategy_file,
            base_strategy_json,
            base_strategy_toml,
            generations,
            population,
            limit,
            seed,
            name_prefix,
        } => cmd_strategy_evolve(
            &db,
            &tasks,
            base_strategy_id.as_deref(),
            base_strategy_file.as_deref(),
            base_strategy_json.as_deref(),
            base_strategy_toml.as_deref(),
            generations,
            population,
            limit,
            seed,
            &name_prefix,
        ),
    }
}

fn load_strategy_for_pack(
    db: &Db,
    strategy_id: Option<&str>,
    strategy_file: Option<&str>,
    strategy_json: Option<&str>,
    strategy_toml: Option<&str>,
) -> Result<StrategyConfig> {
    // Precedence:
    // 1) explicit file/json/toml overrides
    // 2) stored strategy by id
    // 3) default
    if strategy_file.is_some() || strategy_json.is_some() || strategy_toml.is_some() {
        return load_strategy_from_sources(strategy_file, strategy_json, strategy_toml);
    }
    if let Some(id) = strategy_id {
        let Some(rec) = db.get_strategy(id)? else {
            return Err(anyhow!("strategy not found: {id}"));
        };
        let cfg: StrategyConfig = json::from_str(&rec.config_json)?;
        return Ok(cfg);
    }
    Ok(StrategyConfig::default())
}

fn load_strategy_from_sources(
    file: Option<&str>,
    inline_json: Option<&str>,
    inline_toml: Option<&str>,
) -> Result<StrategyConfig> {
    if let Some(s) = inline_json {
        let cfg: StrategyConfig = json::from_str(s)?;
        return Ok(cfg);
    }
    if let Some(s) = inline_toml {
        let cfg: StrategyConfig = toml::from_str(s)?;
        return Ok(cfg);
    }
    if let Some(path) = file {
        let raw = fs::read_to_string(path)?;
        if path.ends_with(".toml") {
            let cfg: StrategyConfig = toml::from_str(&raw)?;
            return Ok(cfg);
        }
        // default to JSON
        let cfg: StrategyConfig = json::from_str(&raw)?;
        return Ok(cfg);
    }
    Ok(StrategyConfig::default())
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        id[..12].to_string()
    }
}

fn build_pack(
    db: &Db,
    embedder: &Embedder,
    vec_index: Option<&ce_store::VecIndex>,
    task: &str,
    strategy: &StrategyConfig,
    seen: Option<&HashSet<String>>,
) -> Result<ContextPack> {
    let span_cap = strategy.signal_max_spans.max(strategy.signal_file_line_max);
    let path_cap = strategy.signal_max_paths.max(1);
    let signal_bundle = signals::extract_signals(task, span_cap, path_cap);

    let ranked = query::candidate_rowids_for_pack_with_index(
        db,
        embedder,
        vec_index,
        task,
        strategy,
        Some(&signal_bundle),
    )?;

    // Precompute slice hints (used for body compaction).
    let file_line_hints = ce_core::util::extract_file_line_hints(task, span_cap);
    let task_tokens = ce_core::util::extract_ident_tokens(task);
    let token_counter = TokenCounter::new(&strategy.tokenizer);

    // Load fragments and build pack candidates
    let mut cands: Vec<Candidate> = Vec::new();
    for (rid, sc, why) in ranked.into_iter().take(strategy.candidate_pool_limit) {
        let frag = db.get_fragment_by_rowid(rid)?;
        let mut score = sc;
        let mut reason = why;
        if strategy.avoid_seen {
            if let Some(seen_set) = seen {
                if seen_set.contains(&frag.id) {
                    score *= strategy.seen_score_mul;
                    reason = format!("{reason};seen");
                }
            }
        }
        let signature = decorate_signature(&frag);

        // Compute symbol-focused tokens for slicing.
        //
        // Goal: avoid broad grep slices by restricting to identifiers that appear BOTH
        // in (a) the task text and (b) this fragment's referenced identifiers.
        let mut focus_tokens: Vec<String> = Vec::new();
        {
            let raw_refs = db.refs_for_fragment(rid, 128).unwrap_or_default();
            for r in raw_refs {
                let t = r.to_ascii_lowercase();
                if t.len() >= 2 && task_tokens.binary_search(&t).is_ok() {
                    focus_tokens.push(t);
                }
            }
            if let Some(sym) = &frag.symbol {
                for part in sym.split("::") {
                    let t = part.trim().to_ascii_lowercase();
                    if t.len() >= 2 && task_tokens.binary_search(&t).is_ok() {
                        focus_tokens.push(t);
                    }
                }
            }
            focus_tokens.sort();
            focus_tokens.dedup();
            focus_tokens.truncate(16);
        }

        // Default upgrade content: full body.
        let full_body = decorate_body(&frag);
        let mut body_view = FragmentView::Body;
        let mut body_text = full_body.clone();

        // Optional compaction: replace full body with a slice when it saves meaningful tokens.
        if strategy.body_snippet_mode != "full" {
            if let Some((slice_reason, slice_text)) = compute_best_slice(
                &frag,
                &file_line_hints,
                &task_tokens,
                &focus_tokens,
                strategy,
            ) {
                let decorated = decorate_slice(&frag, &slice_reason, &slice_text);
                let full_toks = token_counter.count(&full_body);
                let slice_toks = token_counter.count(&decorated);
                if full_toks.saturating_sub(slice_toks) >= strategy.body_snippet_min_savings_tokens
                {
                    body_view = FragmentView::Slice;
                    body_text = decorated;
                }
            }
        }

        cands.push(Candidate {
            id: frag.id.clone(),
            rowid: rid,
            path: frag.file.display().to_string(),
            kind: frag.kind,
            symbol: frag.symbol.clone(),
            span: frag.span,
            score,
            reason,
            signature,
            body: body_text,
            neighbors: Vec::new(),
            body_view,
        });
    }

    attach_candidate_neighbors(db, strategy, &mut cands)?;

    let mut pack = pack_with_strategy(strategy, cands);
    let (signals_used, signals_used_stats) = signals::signals_used(&signal_bundle, &pack.items);
    pack.signals = signal_bundle.clone();
    pack.signals_used = signals_used;
    pack.metrics.signals_extracted = signals::signal_stats(&pack.signals);
    pack.metrics.signals_used = signals_used_stats;
    pack.metrics.redundancy_pct = Some(0.0);

    if let Some(baseline_tokens) =
        compute_baseline_tokens(db, task, &signal_bundle, &token_counter)?
    {
        pack.metrics.baseline_tokens_total = Some(baseline_tokens);
        if baseline_tokens > 0 {
            let saved = (baseline_tokens as f32 - pack.used_tokens as f32) / baseline_tokens as f32;
            pack.metrics.saved_pct = Some(saved * 100.0);
        }
    }

    if let Some(recipe_excerpt) = build_recipe_excerpt(db, task, strategy, &token_counter)? {
        pack.recipe_excerpt = Some(recipe_excerpt);
    }
    pack.repository_memory_excerpt =
        build_repository_memory_excerpt(db, task, strategy, &token_counter)?;

    Ok(pack)
}

fn attach_candidate_neighbors(
    db: &Db,
    strategy: &StrategyConfig,
    cands: &mut [Candidate],
) -> Result<()> {
    if cands.is_empty() {
        return Ok(());
    }

    let mut id_by_rowid: HashMap<i64, String> = HashMap::new();
    let mut by_path: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, cand) in cands.iter().enumerate() {
        id_by_rowid.insert(cand.rowid, cand.id.clone());
        by_path.entry(cand.path.clone()).or_default().push(idx);
    }

    let max_edges = strategy.edge_max_edges_per_node.max(1);
    for idx in 0..cands.len() {
        let rowid = cands[idx].rowid;
        let mut neighbor_map: HashMap<String, f32> = HashMap::new();

        for (to, _ty, w) in db.edges_outgoing(rowid, max_edges)? {
            if let Some(id) = id_by_rowid.get(&to) {
                let e = neighbor_map.entry(id.clone()).or_insert(w);
                if w > *e {
                    *e = w;
                }
            }
        }
        for (from, _ty, w) in db.edges_incoming(rowid, max_edges)? {
            if let Some(id) = id_by_rowid.get(&from) {
                let e = neighbor_map.entry(id.clone()).or_insert(w);
                if w > *e {
                    *e = w;
                }
            }
        }

        if let Some(peers) = by_path.get(&cands[idx].path) {
            for &j in peers {
                if j == idx {
                    continue;
                }
                let id = cands[j].id.clone();
                neighbor_map.entry(id).or_insert(0.2);
            }
        }

        let mut neighbors: Vec<CandidateNeighbor> = neighbor_map
            .into_iter()
            .map(|(id, weight)| CandidateNeighbor { id, weight })
            .collect();
        neighbors.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        neighbors.truncate(24);
        cands[idx].neighbors = neighbors;
    }

    Ok(())
}

fn build_recipe_excerpt(
    db: &Db,
    task: &str,
    strategy: &StrategyConfig,
    token_counter: &TokenCounter,
) -> Result<Option<String>> {
    if !strategy.recipes_enabled {
        return Ok(None);
    }

    let mut task_tokens = ce_core::util::failure_tokens(task);
    if task_tokens.is_empty() {
        return Ok(None);
    }
    task_tokens.sort();
    task_tokens.dedup();

    let recipes = db.load_recipes(200)?;
    let mut scored: Vec<(f32, ce_store::types::RecipeRecord)> = Vec::new();
    for rec in recipes {
        let mut rec_tokens: Vec<String> = rec
            .tokens
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        rec_tokens.sort();
        rec_tokens.dedup();
        let sim = ce_core::util::jaccard_sorted(&task_tokens, &rec_tokens);
        if sim >= strategy.recipes_min_similarity {
            scored.push((sim, rec));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if scored.is_empty() {
        return Ok(None);
    }

    let max_tokens = strategy.recipes_max_tokens.max(1);
    let mut out = String::new();
    out.push_str("[recipe-memory]\n");
    out.push_str("Prior fix patterns (non-authoritative):\n");

    for (sim, rec) in scored.into_iter().take(3) {
        let failure = truncate_line(&rec.failure_excerpt, 160);
        let pack = truncate_line(&rec.pack_summary, 160);
        let patch = truncate_line(&rec.patch_meta, 160);
        let block = format!(
            "- recipe #{} (sim {:.2})\n  - failure: {}\n  - pack: {}\n  - patch: {}\n",
            rec.recipe_id, sim, failure, pack, patch
        );
        let next = format!("{}{}", out, block);
        if token_counter.count(&next) > max_tokens {
            break;
        }
        out.push_str(&block);
    }

    if out.lines().count() <= 2 {
        return Ok(None);
    }

    Ok(Some(out))
}

fn build_repository_memory_excerpt(
    db: &Db,
    task: &str,
    strategy: &StrategyConfig,
    token_counter: &TokenCounter,
) -> Result<Option<String>> {
    if !strategy.repository_memory_enabled {
        return Ok(None);
    }
    let records =
        db.search_repository_memory(task, strategy.repository_memory_min_similarity, 6)?;
    if records.is_empty() {
        return Ok(None);
    }
    let mut out = String::from("[repository-memory]\nRelevant decisions and golden paths:\n");
    for (score, record) in records {
        let block = format!(
            "- [{}] {} (relevance {:.2}, path {})\n  {}\n",
            record.kind,
            record.title,
            score,
            record.path.as_deref().unwrap_or("-"),
            truncate_line(&record.content, 240)
        );
        if token_counter.count(&format!("{out}{block}")) > strategy.repository_memory_max_tokens {
            break;
        }
        out.push_str(&block);
    }
    Ok((out.lines().count() > 2).then_some(out))
}

fn truncate_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if out.len() >= max {
            out.push_str("...");
            break;
        }
        if ch.is_control() {
            continue;
        }
        out.push(ch);
    }
    out
}

fn compute_baseline_tokens(
    db: &Db,
    task: &str,
    signals: &SignalBundle,
    token_counter: &TokenCounter,
) -> Result<Option<usize>> {
    let mut texts: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();

    let mut add_rowid = |rid: i64| -> Result<()> {
        if seen.insert(rid) {
            let frag = db.get_fragment_by_rowid(rid)?;
            texts.push(decorate_body(&frag));
        }
        Ok(())
    };

    if let Some(span) = signals.spans.first() {
        for rid in db.fragment_rowids_covering_line(&span.path, span.line, 1)? {
            add_rowid(rid)?;
        }
        for rid in lexical_hits(db, task, 2)? {
            add_rowid(rid)?;
        }
    } else {
        for rid in lexical_hits(db, task, 3)? {
            add_rowid(rid)?;
        }
    }

    if texts.is_empty() {
        return Ok(None);
    }

    let mut total = 0usize;
    for t in texts {
        total += token_counter.count(&t);
    }
    Ok(Some(total))
}

fn lexical_hits(db: &Db, task: &str, k: usize) -> Result<Vec<i64>> {
    if k == 0 {
        return Ok(vec![]);
    }

    let hits = match db.search_fts(task, k) {
        Ok(v) => v,
        Err(_) => {
            let toks = ce_core::util::extract_ident_tokens(task);
            let q = toks.join(" ");
            if q.trim().is_empty() {
                vec![]
            } else {
                db.search_fts(&q, k).unwrap_or_default()
            }
        }
    };

    Ok(hits.into_iter().map(|(rid, _)| rid).collect())
}

fn decorate_signature(frag: &ce_core::model::Fragment) -> String {
    format!(
        "[frag:{}]\npath: {}\nkind: {:?}\nsymbol: {}\nspan: L{}-L{}\n\n{}",
        frag.id,
        frag.file.display(),
        frag.kind,
        frag.symbol.clone().unwrap_or_default(),
        frag.span.start_line.saturating_add(1),
        frag.span.end_line.saturating_add(1),
        frag.signature.trim_end()
    )
}

fn decorate_body(frag: &ce_core::model::Fragment) -> String {
    format!(
        "[frag:{} BODY]\npath: {}\nkind: {:?}\nsymbol: {}\nspan: L{}-L{}\n\n{}",
        frag.id,
        frag.file.display(),
        frag.kind,
        frag.symbol.clone().unwrap_or_default(),
        frag.span.start_line.saturating_add(1),
        frag.span.end_line.saturating_add(1),
        frag.body.trim_end()
    )
}

fn decorate_slice(frag: &ce_core::model::Fragment, reason: &str, slice: &str) -> String {
    format!(
        "[frag:{} SLICE]\npath: {}\nkind: {:?}\nsymbol: {}\nfocus: {}\nspan: L{}-L{}\n\n{}\n\n{}",
        frag.id,
        frag.file.display(),
        frag.kind,
        frag.symbol.clone().unwrap_or_default(),
        reason,
        frag.span.start_line.saturating_add(1),
        frag.span.end_line.saturating_add(1),
        frag.signature.trim_end(),
        slice.trim_end()
    )
}

fn compute_best_slice(
    frag: &ce_core::model::Fragment,
    file_line_hints: &[(String, u32)],
    task_tokens: &[String],
    focus_tokens: &[String],
    cfg: &StrategyConfig,
) -> Option<(String, String)> {
    let mode = cfg.body_snippet_mode.as_str();
    let ctx = cfg.body_snippet_context_lines;
    let max_lines = cfg.body_snippet_max_lines;

    // Supported modes are stringly-typed on purpose (DGM configs can mutate them).
    // We treat the value as a "capability list" and check for substrings:
    // - "signals" enables file:line slicing
    // - "symbols" enables symbol-focused token grep
    // - "ast" enables AST-based pruning (Rust)
    // - "skeleton" enables AST skeletonization (Rust)
    // - "tsx_skeleton" enables TSX/JSX skeletonization
    // - "swiftui_skeleton" enables SwiftUI skeletonization
    // - "query_grep" enables full task-token grep
    //
    // Examples:
    // - "signals"
    // - "signals_or_symbols_or_query_grep"
    // - "signals_or_symbols_or_ast_or_query_grep"

    let allow_signals = mode.contains("signals");
    let allow_symbols = mode.contains("symbols");
    let allow_ast = mode.contains("ast");
    let allow_skeleton = mode.contains("skeleton");
    let allow_tsx = mode.contains("tsx_skeleton");
    let allow_swiftui = mode.contains("swiftui_skeleton");
    let allow_query = mode.contains("query_grep");

    // Precompute signal targets once (also useful for AST pruning).
    let mut targets: Vec<u32> = Vec::new();
    if allow_signals || allow_ast || allow_skeleton {
        let frag_path = frag.file.display().to_string();
        for (p, line1) in file_line_hints {
            let path_match = frag_path == *p || frag_path.ends_with(p);
            if !path_match {
                continue;
            }
            let line0 = line1.saturating_sub(1);
            if line0 >= frag.span.start_line && line0 <= frag.span.end_line {
                targets.push(*line1);
            }
        }
        targets.sort();
        targets.dedup();
    }

    // Uniform policy-driven AST slicing. All supported languages implement the
    // same semantic contract; legacy modes below remain compatibility fallbacks.
    let policy_tokens = if focus_tokens.is_empty() {
        task_tokens
    } else {
        focus_tokens
    };
    let request = ce_core::slicing::AstSliceRequest {
        source: &frag.body,
        fragment_start_line: frag.span.start_line,
        target_lines: &targets,
        focus_symbols: policy_tokens,
        policy: &cfg.ast_slice_policy,
    };
    let extension = frag
        .file
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let policy_slice = match extension {
        "rs" => ce_lang_rust::policy_ast_slice(request),
        "ts" | "js" => ce_lang_tsreact::policy_ast_slice(request, false),
        "tsx" | "jsx" => ce_lang_tsreact::policy_ast_slice(request, true),
        "swift" => ce_lang_swift::policy_ast_slice(request),
        _ => Ok(None),
    };
    if let Ok(Some(slice)) = policy_slice {
        return Some((
            format!("policy-ast:{}:{}-nodes", extension, slice.included_nodes),
            slice.text,
        ));
    }

    // A) signal-driven slice
    if allow_signals {
        if !targets.is_empty() {
            if let Some(s) = snippet::slice_by_file_lines(
                &frag.body,
                frag.span.start_line,
                &targets,
                ctx,
                max_lines,
            ) {
                let frag_path = frag.file.display().to_string();
                let head = targets.get(0).copied().unwrap_or(0);
                let reason = format!("signal:{}:{}", frag_path, head);
                return Some((reason, s));
            }
        }
    }

    // B) symbol-focused grep slice (narrower than full task token grep)
    if allow_symbols {
        if let Some(s) = snippet::slice_by_grep(
            &frag.body,
            frag.span.start_line,
            focus_tokens,
            ctx,
            max_lines,
        ) {
            let mut show: Vec<String> = focus_tokens.iter().take(8).cloned().collect();
            show.retain(|t| !t.is_empty());
            let reason = if show.is_empty() {
                "symbols".to_string()
            } else {
                format!("symbols:{}", show.join(","))
            };
            return Some((reason, s));
        }
    }

    // C) AST-based pruning slice (Rust)
    if allow_ast {
        // Prefer symbol-focused tokens; fall back to task tokens.
        let toks: &[String] = if !focus_tokens.is_empty() {
            focus_tokens
        } else {
            task_tokens
        };
        if let Some(s) = ce_lang_rust::ast_prune_slice(
            &frag.body,
            frag.span.start_line,
            &targets,
            toks,
            ctx,
            max_lines,
        ) {
            let mut show: Vec<String> = toks.iter().take(6).cloned().collect();
            show.retain(|t| !t.is_empty());
            let reason = if show.is_empty() {
                "ast".to_string()
            } else {
                format!("ast:{}", show.join(","))
            };
            return Some((reason, s));
        }
    }

    // D) AST skeletonization (Rust)
    if allow_skeleton {
        // Prefer symbol-focused tokens; fall back to task tokens.
        let toks: &[String] = if !focus_tokens.is_empty() {
            focus_tokens
        } else {
            task_tokens
        };
        if let Some(s) = ce_lang_rust::ast_skeleton_slice(
            &frag.body,
            frag.span.start_line,
            frag.kind,
            &targets,
            toks,
            cfg,
        ) {
            let mut show: Vec<String> = toks.iter().take(6).cloned().collect();
            show.retain(|t| !t.is_empty());
            let reason = if show.is_empty() {
                "skeleton".to_string()
            } else {
                format!("skeleton:{}", show.join(","))
            };
            return Some((reason, s));
        }
    }

    // E) TSX skeletonization
    if allow_tsx {
        if let Some(s) = snippet::skeletonize_tsx(
            &frag.body,
            cfg.tsx_skeleton_max_depth,
            cfg.tsx_skeleton_max_props,
        ) {
            let reason = "tsx_skeleton".to_string();
            return Some((reason, s));
        }
    }

    // F) SwiftUI skeletonization
    if allow_swiftui {
        if let Some(s) = snippet::skeletonize_swiftui(
            &frag.body,
            cfg.swiftui_skeleton_max_depth,
            cfg.swiftui_skeleton_max_modifiers,
        ) {
            let reason = "swiftui_skeleton".to_string();
            return Some((reason, s));
        }
    }

    // G) token-grep slice
    if allow_query {
        if let Some(s) = snippet::slice_by_grep(
            &frag.body,
            frag.span.start_line,
            task_tokens,
            ctx,
            max_lines,
        ) {
            let mut show: Vec<String> = task_tokens.iter().take(6).cloned().collect();
            show.retain(|t| !t.is_empty());
            let reason = if show.is_empty() {
                "grep".to_string()
            } else {
                format!("grep:{}", show.join(","))
            };
            return Some((reason, s));
        }
    }

    None
}

fn render_pack(pack: &ContextPack) -> String {
    let mut out = String::new();
    out.push_str("[ce-pack v0]\n");
    out.push_str(&format!("pack_id: {}\n", pack.pack_id));
    out.push_str(&format!("budget_chars: {}\n", pack.budget_chars));
    out.push_str(&format!("used_chars: {}\n", pack.used_chars));
    if let Some(bt) = pack.budget_tokens {
        out.push_str(&format!("budget_tokens: {}\n", bt));
    }
    out.push_str(&format!("used_tokens: {}\n", pack.used_tokens));
    out.push_str(&format!(
        "pack_tokens_total: {}\n",
        pack.metrics.pack_tokens_total
    ));
    if let Some(bt) = pack.metrics.baseline_tokens_total {
        out.push_str(&format!("baseline_tokens_total: {}\n", bt));
    }
    if let Some(saved) = pack.metrics.saved_pct {
        out.push_str(&format!("saved_pct: {:.1}\n", saved));
    }
    if let Some(hit) = pack.metrics.hit_rate_paths {
        out.push_str(&format!("hit_rate_paths: {:.3}\n", hit));
    } else {
        out.push_str("hit_rate_paths: n/a\n");
    }
    if let Some(avg) = pack.metrics.avg_iterations_per_fix {
        out.push_str(&format!("avg_iterations_per_fix: {:.2}\n", avg));
    } else {
        out.push_str("avg_iterations_per_fix: n/a\n");
    }
    if let Some(redundancy) = pack.metrics.redundancy_pct {
        out.push_str(&format!("redundancy_pct: {:.1}\n", redundancy));
    } else {
        out.push_str("redundancy_pct: n/a\n");
    }
    if let Some(score) = pack.metrics.connectivity_score {
        out.push_str(&format!("connectivity_score: {:.2}\n", score));
    }
    out.push_str(&format!(
        "unbound_symbol_count: {}\n\n",
        pack.metrics.unbound_symbol_count
    ));
    if let Some(selection) = &pack.strategy_selection {
        out.push_str(&format!(
            "strategy_selection: task={} repo={} confidence={:.2} reason={}\n\n",
            selection.task_class,
            selection.repository_archetype,
            selection.confidence,
            selection.reason
        ));
    }

    if let Some(recipe) = &pack.recipe_excerpt {
        out.push_str("## Recipe Memory\n\n");
        out.push_str(recipe);
        out.push_str("\n\n");
    }
    if let Some(memory) = &pack.repository_memory_excerpt {
        out.push_str("## Repository Memory\n\n");
        out.push_str(memory);
        out.push_str("\n\n");
    }

    out.push_str("## Included\n\n");
    for it in &pack.items {
        out.push_str(&format!(
            "### {} ({:?}, score={:.3}, reason={})\n\n",
            it.id, it.view, it.score, it.reason
        ));
        out.push_str(&it.content);
        out.push_str("\n\n");
    }

    if !pack.unresolved_symbols.is_empty() {
        out.push_str("\n## Unresolved Symbols\n\n");
        for sym in &pack.unresolved_symbols {
            let reason = sym
                .reason
                .clone()
                .unwrap_or_else(|| "unresolved".to_string());
            out.push_str(&format!("- {} ({})\n", sym.symbol, reason));
        }
    }

    if pack.missing_links.degraded {
        out.push_str("\n## Missing Links\n\n");
        out.push_str(&format!(
            "Selected the best connected component ({} fragments); {} omitted component(s) could not fit or connect under budget.\n",
            pack.missing_links.selected_component_size,
            pack.missing_links.omitted_component_count
        ));
        for id in &pack.missing_links.omitted_fragment_ids {
            out.push_str(&format!("- {id}\n"));
        }
    }

    if !pack.deferred.is_empty() {
        out.push_str("## Deferred\n\n");
        for d in &pack.deferred {
            let span = format!(
                "L{}-L{}",
                d.span.start_line.saturating_add(1),
                d.span.end_line.saturating_add(1)
            );
            let sym = d.symbol.clone().unwrap_or_default();
            if sym.is_empty() {
                out.push_str(&format!(
                    "- {} {:?} {} [{}] ({})\n",
                    d.id, d.kind, d.path, span, d.reason
                ));
            } else {
                out.push_str(&format!(
                    "- {} {:?} {} [{}] sym={} ({})\n",
                    d.id, d.kind, d.path, span, sym, d.reason
                ));
            }
        }
    }

    out
}

fn indent(s: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    s.lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// -----------------------------------------------------------------------------
// Tiny deterministic RNG + mutation helpers (no external deps)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Rng64 {
    state: u64,
}

impl Rng64 {
    fn new(seed: u64) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut s = if seed == 0 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
        } else {
            seed
        };

        // scramble; avoid zero state
        s ^= 0x9E3779B97F4A7C15;
        if s == 0 {
            s = 1;
        }
        Self { state: s }
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    #[inline]
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }

    #[inline]
    fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }

    fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            return lo;
        }
        lo + (hi - lo) * self.next_f64()
    }

    fn range_usize(&mut self, lo: usize, hi_inclusive: usize) -> usize {
        if hi_inclusive <= lo {
            return lo;
        }
        let span = (hi_inclusive - lo) as u64 + 1;
        lo + (self.next_u64() % span) as usize
    }
}

fn clamp_f32(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

fn scale_usize(rng: &mut Rng64, x: usize, lo: f64, hi: f64, min: usize, max: usize) -> usize {
    let factor = rng.range_f64(lo, hi);
    let mut y = ((x as f64) * factor).round() as i64;
    if y < min as i64 {
        y = min as i64;
    }
    if y > max as i64 {
        y = max as i64;
    }
    y as usize
}

fn jitter_f32(rng: &mut Rng64, x: f32, delta: f32, lo: f32, hi: f32) -> f32 {
    let d = (rng.range_f64(-(delta as f64), delta as f64)) as f32;
    clamp_f32(x + d, lo, hi)
}

fn mutate_strategy(base: &StrategyConfig, rng: &mut Rng64) -> StrategyConfig {
    let mut cfg = base.clone();

    // Retrieval knobs
    if rng.chance(0.70) {
        cfg.lexical_k = scale_usize(rng, cfg.lexical_k, 0.6, 1.6, 10, 140);
    }
    if rng.chance(0.70) {
        cfg.semantic_k = scale_usize(rng, cfg.semantic_k, 0.6, 1.6, 0, 140);
    }
    if rng.chance(0.70) {
        cfg.hybrid_alpha = jitter_f32(rng, cfg.hybrid_alpha, 0.20, 0.05, 0.95);
    }

    // Expansion knobs
    if rng.chance(0.10) {
        cfg.graph_expand = !cfg.graph_expand;
    }
    if rng.chance(0.50) {
        cfg.graph_seed_k = scale_usize(rng, cfg.graph_seed_k, 0.6, 1.6, 5, 80);
    }
    if rng.chance(0.35) {
        cfg.edge_radius = rng.range_usize(0, 4);
    }

    // Edge-type specific traversal: these dials tend to matter a lot for large repos.
    if rng.chance(0.40) {
        cfg.edge_refers_radius = rng.range_usize(0, 4);
    }
    if rng.chance(0.40) {
        cfg.edge_module_radius = rng.range_usize(0, 3);
    }
    if rng.chance(0.35) {
        cfg.edge_reverse_radius = rng.range_usize(0, 3);
    }
    if rng.chance(0.15) {
        cfg.edge_prioritize_by_type = !cfg.edge_prioritize_by_type;
    }
    if rng.chance(0.50) {
        cfg.edge_max_nodes_per_seed =
            scale_usize(rng, cfg.edge_max_nodes_per_seed, 0.6, 1.8, 16, 140);
    }
    if rng.chance(0.50) {
        cfg.neighbors_k = scale_usize(rng, cfg.neighbors_k, 0.4, 2.0, 0, 12);
    }
    if rng.chance(0.50) {
        cfg.refs_per_seed = scale_usize(rng, cfg.refs_per_seed, 0.4, 2.0, 0, 24);
    }
    if rng.chance(0.50) {
        cfg.defs_per_ref = scale_usize(rng, cfg.defs_per_ref, 0.4, 2.0, 0, 10);
    }

    // Weights
    if rng.chance(0.35) {
        cfg.edge_out_weight = jitter_f32(rng, cfg.edge_out_weight, 0.55, 0.0, 8.0);
    }
    if rng.chance(0.35) {
        cfg.edge_in_weight = jitter_f32(rng, cfg.edge_in_weight, 0.30, 0.0, 8.0);
    }

    // Edge-type multipliers (strategy search dials)
    if rng.chance(0.35) {
        cfg.edge_mul_refers = jitter_f32(rng, cfg.edge_mul_refers, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.35) {
        cfg.edge_mul_mod = jitter_f32(rng, cfg.edge_mul_mod, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.35) {
        cfg.edge_mul_use = jitter_f32(rng, cfg.edge_mul_use, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.30) {
        cfg.edge_mul_imported_by = jitter_f32(rng, cfg.edge_mul_imported_by, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.30) {
        cfg.edge_mul_modded_by = jitter_f32(rng, cfg.edge_mul_modded_by, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.30) {
        cfg.edge_mul_other = jitter_f32(rng, cfg.edge_mul_other, 0.35, 0.0, 3.0);
    }
    if rng.chance(0.35) {
        cfg.neighbor_weight = jitter_f32(rng, cfg.neighbor_weight, 0.35, 0.0, 6.0);
    }
    if rng.chance(0.35) {
        cfg.def_weight = jitter_f32(rng, cfg.def_weight, 0.55, 0.0, 6.0);
    }

    // Signals
    if rng.chance(0.10) {
        cfg.signals_enabled = !cfg.signals_enabled;
    }
    if rng.chance(0.30) {
        cfg.signal_file_line_boost = jitter_f32(rng, cfg.signal_file_line_boost, 2.0, 0.0, 10.0);
    }
    if rng.chance(0.35) {
        cfg.signal_max_spans = scale_usize(rng, cfg.signal_max_spans.max(1), 0.5, 1.6, 1, 24);
    }
    if rng.chance(0.35) {
        cfg.signal_max_paths = scale_usize(rng, cfg.signal_max_paths.max(1), 0.5, 1.6, 1, 24);
    }
    if rng.chance(0.30) {
        cfg.signal_span_boost = jitter_f32(rng, cfg.signal_span_boost, 2.0, 0.0, 10.0);
    }

    // Packing knobs
    if rng.chance(0.60) {
        cfg.budget_chars = scale_usize(rng, cfg.budget_chars, 0.7, 1.4, 4000, 28000);
    }
    if rng.chance(0.25) {
        match cfg.budget_tokens {
            None => {
                if rng.chance(0.70) {
                    cfg.budget_tokens = Some(rng.range_usize(1200, 8000));
                }
            }
            Some(t) => {
                if rng.chance(0.35) {
                    cfg.budget_tokens = None;
                } else {
                    cfg.budget_tokens = Some(scale_usize(rng, t, 0.7, 1.4, 600, 10000));
                }
            }
        }
    }
    if rng.chance(0.50) {
        cfg.max_bodies = scale_usize(rng, cfg.max_bodies.max(1), 0.5, 1.8, 0, 6);
    }

    // Body compaction
    if rng.chance(0.25) {
        // Switch between supported modes.
        let modes = [
            "full",
            "signals",
            "query_grep",
            "signals_or_query_grep",
            "signals_or_ast_or_query_grep",
            "signals_or_ast_or_skeleton_or_query_grep",
            "signals_or_symbols_or_ast_or_query_grep",
            "signals_or_symbols_or_ast_or_skeleton_or_query_grep",
            "signals_or_tsx_skeleton_or_query_grep",
            "signals_or_swiftui_skeleton_or_query_grep",
            "signals_or_symbols_or_tsx_skeleton_or_query_grep",
            "signals_or_symbols_or_swiftui_skeleton_or_query_grep",
        ];
        cfg.body_snippet_mode =
            modes[rng.range_usize(0, modes.len().saturating_sub(1))].to_string();
    }
    if rng.chance(0.45) {
        cfg.body_snippet_context_lines =
            scale_usize(rng, cfg.body_snippet_context_lines.max(0), 0.5, 1.8, 0, 20);
    }
    if rng.chance(0.45) {
        cfg.body_snippet_max_lines =
            scale_usize(rng, cfg.body_snippet_max_lines.max(20), 0.6, 1.8, 40, 400);
    }
    if rng.chance(0.35) {
        cfg.body_snippet_min_savings_tokens = scale_usize(
            rng,
            cfg.body_snippet_min_savings_tokens.max(0),
            0.5,
            2.0,
            0,
            300,
        );
    }

    // Diversity controls
    if rng.chance(0.70) {
        cfg.mmr_lambda = jitter_f32(rng, cfg.mmr_lambda, 0.10, 0.70, 1.0);
    }
    if rng.chance(0.50) {
        cfg.mmr_top_n = scale_usize(rng, cfg.mmr_top_n, 0.6, 1.8, 10, 300);
    }
    if rng.chance(0.45) {
        cfg.per_file_cap_signatures =
            scale_usize(rng, cfg.per_file_cap_signatures, 0.6, 1.8, 2, 30);
    }
    if rng.chance(0.30) {
        cfg.per_file_cap_bodies = rng.range_usize(1, 3);
    }
    if rng.chance(0.15) {
        cfg.support_enabled = !cfg.support_enabled;
    }
    if rng.chance(0.35) {
        cfg.support_max_defs = scale_usize(rng, cfg.support_max_defs.max(0), 0.6, 1.8, 0, 32);
    }
    if rng.chance(0.20) {
        cfg.support_signature_only = !cfg.support_signature_only;
    }
    if rng.chance(0.30) {
        cfg.support_min_confidence = jitter_f32(rng, cfg.support_min_confidence, 0.10, 0.0, 1.0);
    }
    if rng.chance(0.15) {
        cfg.subgraph_enabled = !cfg.subgraph_enabled;
    }
    if rng.chance(0.35) {
        cfg.beam_width = scale_usize(rng, cfg.beam_width.max(1), 0.5, 1.8, 1, 24);
    }
    if rng.chance(0.35) {
        cfg.max_hops = rng.range_usize(0, 4);
    }
    if rng.chance(0.30) {
        cfg.connectivity_penalty = jitter_f32(rng, cfg.connectivity_penalty, 0.25, 0.0, 4.0);
    }
    if rng.chance(0.15) {
        cfg.recipes_enabled = !cfg.recipes_enabled;
    }
    if rng.chance(0.35) {
        cfg.recipes_max_tokens =
            scale_usize(rng, cfg.recipes_max_tokens.max(32), 0.6, 1.8, 64, 800);
    }
    if rng.chance(0.30) {
        cfg.recipes_min_similarity = jitter_f32(rng, cfg.recipes_min_similarity, 0.15, 0.0, 1.0);
    }

    // Candidate pool
    if rng.chance(0.35) {
        cfg.candidate_pool_limit = scale_usize(rng, cfg.candidate_pool_limit, 0.6, 1.6, 60, 600);
    }

    cfg
}

// HNSW building and hybrid retrieval utilities live in ce-store::query.

fn cmd_bootstrap(
    repo: &str,
    template: inception::ProjectTemplate,
    subtype: Option<inception::ProjectSubtype>,
    force: bool,
    skip_index: bool,
    full: bool,
    prune: bool,
    skip_edges: bool,
    max_files: usize,
    skip_onboarding: bool,
    skip_golden_paths: bool,
) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("Repo path does not exist: {}", repo));
    }
    inception::apply_template(&repo_path, template, subtype, force)?;

    let ce_dir = repo_path.join(".ce");
    fs::create_dir_all(&ce_dir)?;
    let db_path = ce_dir.join("index.sqlite");
    let hnsw_dir = ce_dir.join("hnsw");

    if !skip_index {
        cmd_index(
            repo,
            db_path.to_string_lossy().as_ref(),
            hnsw_dir.to_string_lossy().as_ref(),
            full,
            prune,
            skip_edges,
            max_files,
        )?;
    }

    let db = Db::open(db_path.to_string_lossy().as_ref())?;
    if !skip_onboarding {
        inception::write_onboarding(&repo_path, &db, template)?;
    }
    if !skip_golden_paths {
        inception::write_golden_paths(&repo_path, &db, template)?;
    }
    println!("Bootstrap complete.");
    Ok(())
}
