use anyhow::{anyhow, Result};
use clap::{Args, Parser, ValueEnum};
use ce_core::model::{FragmentView, SignalBundle, StrategyConfig};
use ce_core::pack::{pack_with_strategy, Candidate, CandidateNeighbor};
use ce_core::snippet;
use ce_core::tokenizer::TokenCounter;
use ce_core::signals;
use ce_store::{Db, Embedder, VecIndex};
use ce_store::query;
use ce_store_core::{CeStore, PackRequest};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::Path;

#[cfg(feature = "surreal")]
use ce_store_surreal::{SurrealConfig, SurrealEngine, SurrealStore};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StoreKind {
    Sqlite,
    Surreal,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SurrealEngineArg {
    Surrealkv,
    Mem,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FtsMode {
    On,
    Off,
}

#[derive(Debug, Clone, Args)]
struct StoreArgs {
    #[arg(long, value_enum, default_value_t = StoreKind::Sqlite)]
    store: StoreKind,

    /// Path to sqlite db (sqlite only).
    #[arg(long)]
    db: Option<String>,

    /// Directory for HNSW dumps (sqlite only).
    #[arg(long)]
    hnsw_dir: Option<String>,

    /// SurrealDB storage directory (surrealkv engine).
    #[arg(long, default_value = ".ce/surreal")]
    surreal_path: String,

    /// SurrealDB engine (surrealkv or mem).
    #[arg(long, value_enum, default_value_t = SurrealEngineArg::Surrealkv)]
    surreal_engine: SurrealEngineArg,

    /// Use SurrealKV versioned mode.
    #[arg(long, default_value_t = false)]
    surreal_versioned: bool,

    /// SurrealDB namespace.
    #[arg(long, default_value = "prune")]
    surreal_ns: String,

    /// SurrealDB database.
    #[arg(long, default_value = "main")]
    surreal_db: String,

    /// Embedding dimension (defaults to embedder dim).
    #[arg(long)]
    embedding_dim: Option<usize>,

    /// Full-text search mode for SurrealDB.
    #[arg(long, value_enum, default_value_t = FtsMode::On)]
    fts: FtsMode,
}

#[derive(Parser, Debug)]
struct CliArgs {
    #[command(flatten)]
    store: StoreArgs,
}

const SURREAL_REPO_ID: &str = "default";

enum Backend {
    Sqlite { db: Db, embedder: Embedder, vec_index: VecIndex },
    #[cfg(feature = "surreal")]
    Surreal { store: SurrealStore, embedder: Embedder, rt: tokio::runtime::Runtime },
}

struct App {
    backend: Backend,
    sessions: HashMap<String, SessionState>,
}

#[derive(Debug, Default, Clone)]
struct SessionState {
    seen: HashSet<String>,
}

fn main() -> Result<()> {
    let args = CliArgs::parse();
    let backend = match args.store.store {
        StoreKind::Sqlite => {
            let db_path = args
                .store
                .db
                .as_ref()
                .ok_or_else(|| anyhow!("--db is required for sqlite store"))?;
            let hnsw_dir = args
                .store
                .hnsw_dir
                .as_ref()
                .ok_or_else(|| anyhow!("--hnsw-dir is required for sqlite store"))?;
            let db = Db::open(db_path)?;
            let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;
            let vec_index = query::load_or_build_hnsw(&db, Path::new(hnsw_dir), query::DEFAULT_HNSW_BASE, false)?;
            Backend::Sqlite { db, embedder, vec_index }
        }
        StoreKind::Surreal => {
            #[cfg(feature = "surreal")]
            {
                let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;
                let embed_dim = args.store.embedding_dim.unwrap_or(embedder.dim());
                let engine = match args.store.surreal_engine {
                    SurrealEngineArg::Mem => SurrealEngine::Mem,
                    SurrealEngineArg::Surrealkv => SurrealEngine::SurrealKv {
                        path: args.store.surreal_path.clone(),
                        versioned: args.store.surreal_versioned,
                    },
                };
                let cfg = SurrealConfig {
                    ns: args.store.surreal_ns.clone(),
                    db: args.store.surreal_db.clone(),
                    engine,
                    embedding_dim: embed_dim,
                    fts_enabled: matches!(args.store.fts, FtsMode::On),
                };
                let rt = tokio::runtime::Runtime::new()?;
                let store = rt.block_on(SurrealStore::connect(cfg))?;
                Backend::Surreal { store, embedder, rt }
            }
            #[cfg(not(feature = "surreal"))]
            {
                return Err(anyhow!("Surreal support not enabled (build with --features surreal)"));
            }
        }
    };

    let app = App { backend, sessions: HashMap::new() };
    serve_stdio(app)
}

fn serve_stdio(mut app: App) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // Notifications/errors: ignore malformed
                eprintln!("invalid json-rpc message: {e}");
                continue;
            }
        };

        if let Some(resp) = handle_message(&mut app, msg)? {
            let out = serde_json::to_string(&resp)?;
            writeln!(stdout, "{out}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_message(app: &mut App, msg: Value) -> Result<Option<Value>> {
    // batch support
    if let Value::Array(arr) = msg {
        let mut out = Vec::new();
        for v in arr {
            if let Some(r) = handle_single(app, v)? { out.push(r); }
        }
        return Ok(if out.is_empty() { None } else { Some(Value::Array(out)) });
    }
    handle_single(app, msg)
}

fn handle_single(app: &mut App, msg: Value) -> Result<Option<Value>> {
    let method = msg.get("method").and_then(|v| v.as_str());
    let id = msg.get("id").cloned();

    // notifications => no response
    if id.is_none() {
        return Ok(None);
    }

    let method = method.ok_or_else(|| anyhow!("missing method"))?;
    let id = id.ok_or_else(|| anyhow!("missing id"))?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => json!({
          "protocolVersion": "2025-03-26",
          "capabilities": { "tools": { "listChanged": false } },
          "serverInfo": { "name": "context-engine", "version": env!("CARGO_PKG_VERSION") },
          "instructions": "Use tools/list then tools/call. Tools: context.pack, context.search, fragment.get, strategy.list, strategy.get"
        }),
        "tools/list" => tools_list(),
        "tools/call" => tools_call(app, params)?,
        _ => {
            return Ok(Some(json!({
              "jsonrpc": "2.0",
              "id": id,
              "error": { "code": -32601, "message": format!("Unknown method: {method}") }
            })));
        }
    };

    Ok(Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })))
}

fn tools_list() -> Value {
    json!({
      "tools": [
        {
          "name": "context.search",
          "description": "Hybrid search (FTS5 + embeddings via HNSW) over indexed fragments.",
          "inputSchema": {
            "type": "object",
            "properties": {
              "query": {"type":"string"},
              "k": {"type":"integer", "default": 8, "minimum": 1, "maximum": 50},
              "alpha": {"type":"number", "default": 0.5, "minimum": 0.0, "maximum": 1.0}
            },
            "required": ["query"],
            "additionalProperties": false
          }
        },
        {
          "name": "context.pack",
          "description": "Create a minimal context pack for a task (hybrid retrieval + lightweight graph expansion; signatures-first, bodies for top items).",
          "inputSchema": {
            "type": "object",
            "properties": {
              "task": {"type":"string"},
              "session_id": {"type":"string", "description": "Optional session identifier. If set, the server can avoid repeating already-seen fragments."},
              "remember": {"type":"boolean", "default": true, "description": "If true and session_id is set, mark included fragments as seen."},
              "strategy_id": {"type":"string", "description": "Optional strategy id stored in the DB."},
              "strategy_overrides": {
                "type":"object",
                "description": "Optional partial StrategyConfig override object (merged into base config).",
                "additionalProperties": true
              },
              "budget_chars": {"type":"integer", "default": 12000, "minimum": 2000},
              "budget_tokens": {"type":"integer", "description": "Optional token budget; if set, enforced using the configured tokenizer (tiktoken w/ heuristic fallback)."},
              "tokenizer": {"type":"string", "description": "Tokenizer spec used for token budgeting/counts (e.g. o200k_base, cl100k_base, model:gpt-4o)."},
              "max_bodies": {"type":"integer", "default": 2, "minimum": 0, "maximum": 10},
              "alpha": {"type":"number", "default": 0.5, "minimum": 0.0, "maximum": 1.0},
              "format": {"type":"string", "enum": ["text", "json", "both"], "default": "text", "description": "Output format for the tool response."}
            },
            "required": ["task"],
            "additionalProperties": false
          }
        },
        {
          "name": "fragment.get",
          "description": "Fetch a fragment by id (signature, body, or compact slice).",
          "inputSchema": {
            "type": "object",
            "properties": {
              "id": {"type":"string"},
              "session_id": {"type":"string", "description": "Optional session identifier; the server will remember this fragment as seen."},
              "view": {"type":"string", "enum": ["signature", "body", "slice"], "default": "signature"},
              "task": {"type":"string", "description": "Optional task text for grep-based slicing (when view=slice)."},
              "line": {"type":"integer", "description": "Optional 1-based file line number to slice around (when view=slice)."},
              "context_lines": {"type":"integer", "default": 4, "minimum": 0, "maximum": 50},
              "max_lines": {"type":"integer", "default": 160, "minimum": 20, "maximum": 1000}
            },
            "required": ["id"],
            "additionalProperties": false
          }
        },
        {
          "name": "strategy.list",
          "description": "List stored strategy configs.",
          "inputSchema": {
            "type": "object",
            "properties": {
              "limit": {"type":"integer", "default": 50, "minimum": 1, "maximum": 200},
              "offset": {"type":"integer", "default": 0, "minimum": 0},
              "show_config": {"type":"boolean", "default": false}
            },
            "required": [],
            "additionalProperties": false
          }
        },
        {
          "name": "strategy.get",
          "description": "Get a strategy config by id.",
          "inputSchema": {
            "type": "object",
            "properties": {
              "id": {"type":"string"},
              "pretty": {"type":"boolean", "default": true}
            },
            "required": ["id"],
            "additionalProperties": false
          }
        }
      ]
    })
}

fn tools_call(app: &mut App, params: Value) -> Result<Value> {
    let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("missing params.name"))?;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    let (text, is_error) = match name {
        "context.search" => tool_search(app, args)?,
        "context.pack" => tool_pack(app, args)?,
        "fragment.get" => tool_get(app, args)?,
        "strategy.list" => tool_strategy_list(app, args)?,
        "strategy.get" => tool_strategy_get(app, args)?,
        _ => (format!("Unknown tool: {name}"), true),
    };

    Ok(json!({
      "content": [{"type":"text", "text": text}],
      "isError": is_error
    }))
}

fn tool_search(app: &mut App, args: Value) -> Result<(String, bool)> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
    let alpha = args.get("alpha").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;

    match &mut app.backend {
        Backend::Sqlite { db, embedder, vec_index } => {
            let hits = query::hybrid_search_with_index(db, embedder, Some(vec_index), query, 50, 50, 50, alpha)?;
            let mut out = String::new();
            out.push_str(&format!("Top {k} hits for query: {query}\n"));
            for (i, h) in hits.into_iter().take(k).enumerate() {
                out.push_str(&format!("\n{}. [{:.3}] {} {:?} {}\n", i + 1, h.score, h.frag_id, h.kind, h.path));
                if let Some(sym) = h.symbol {
                    out.push_str(&format!("   symbol: {sym}\n"));
                }
                out.push_str(&indent(&h.signature.trim(), 3));
                out.push('\n');
            }
            Ok((out, false))
        }
        #[cfg(feature = "surreal")]
        Backend::Surreal { store, embedder, rt } => {
            let qvec = embedder.embed_query(query)?;
            let hits = rt.block_on(store.hybrid_search_rrf(SURREAL_REPO_ID, query, &qvec, 50))?;
            let mut out = String::new();
            out.push_str(&format!("Top {k} hits for query: {query}\n"));
            for (i, h) in hits.into_iter().take(k).enumerate() {
                out.push_str(&format!("\n{}. [{:.3}] {} {:?} {}\n", i + 1, h.score, h.frag_id, h.kind, h.path));
                if let Some(sym) = h.symbol {
                    out.push_str(&format!("   symbol: {sym}\n"));
                }
                out.push_str(&indent(&h.signature.trim(), 3));
                out.push('\n');
            }
            Ok((out, false))
        }
    }
}

fn tool_get(app: &mut App, args: Value) -> Result<(String, bool)> {
    let id = args.get("id").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("missing id"))?;
    let view = args.get("view").and_then(|v| v.as_str()).unwrap_or("signature");
    let session_id = args.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());

    let frag = match &mut app.backend {
        Backend::Sqlite { db, .. } => {
            let got = db.get_fragment_by_id(id)?;
            let Some((_rowid, frag)) = got else {
                return Ok((format!("Fragment not found: {id}"), true));
            };
            frag
        }
        #[cfg(feature = "surreal")]
        Backend::Surreal { store, rt, .. } => {
            let frags = rt.block_on(store.fetch_fragments(SURREAL_REPO_ID, &[id.to_string()]))?;
            let Some(rec) = frags.into_iter().next() else {
                return Ok((format!("Fragment not found: {id}"), true));
            };
            rec.to_fragment()
        }
    };

    // Mark as seen for the session (best-effort; no persistence).
    if let Some(sid) = &session_id {
        let st = app.sessions.entry(sid.clone()).or_default();
        st.seen.insert(frag.id.clone());
    }

    let ctx_lines = args.get("context_lines").and_then(|v| v.as_u64()).unwrap_or(4) as usize;
    let max_lines = args.get("max_lines").and_then(|v| v.as_u64()).unwrap_or(160) as usize;

    let text = match view {
        "body" => decorate_body(&frag),
        "slice" => {
            // Prefer explicit line slice if provided; otherwise use grep slice from task text.
            if let Some(line) = args.get("line").and_then(|v| v.as_u64()) {
                let targets = [line as u32];
                if let Some(s) = snippet::slice_by_file_lines(&frag.body, frag.span.start_line, &targets, ctx_lines, max_lines) {
                    decorate_slice(&frag, &format!("line:{}", line), &s)
                } else {
                    // Fallback: AST-based pruning around the target line.
                    let empty: Vec<String> = Vec::new();
                    if let Some(s) = ce_lang_rust::ast_prune_slice(&frag.body, frag.span.start_line, &targets, &empty, ctx_lines, max_lines) {
                        decorate_slice(&frag, &format!("ast:line:{}", line), &s)
                    } else {
                        // Fallback: head slice
                        decorate_slice(&frag, "head", &head_slice(&frag, max_lines))
                    }
                }
            } else if let Some(task) = args.get("task").and_then(|v| v.as_str()) {
                let toks = ce_core::util::extract_ident_tokens(task);
                // Prefer AST pruning; fall back to grep.
                if let Some(s) = ce_lang_rust::ast_prune_slice(&frag.body, frag.span.start_line, &[], &toks, ctx_lines, max_lines) {
                    decorate_slice(&frag, "ast", &s)
                } else if let Some(s) = snippet::slice_by_grep(&frag.body, frag.span.start_line, &toks, ctx_lines, max_lines) {
                    decorate_slice(&frag, "grep", &s)
                } else {
                    decorate_slice(&frag, "head", &head_slice(&frag, max_lines))
                }
            } else {
                decorate_slice(&frag, "head", &head_slice(&frag, max_lines))
            }
        }
        _ => decorate_signature(&frag),
    };

    Ok((text, false))
}

fn tool_pack(app: &mut App, args: Value) -> Result<(String, bool)> {
    let task = args.get("task").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("missing task"))?;
    let session_id = args.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let remember = args.get("remember").and_then(|v| v.as_bool()).unwrap_or(true);

    match &mut app.backend {
        Backend::Sqlite { db, embedder, vec_index } => {
            let mut strategy = load_strategy_for_pack(db, &args)?;

            if let Some(b) = args.get("budget_chars").and_then(|v| v.as_u64()) {
                strategy.budget_chars = b as usize;
            }
            if let Some(bt) = args.get("budget_tokens").and_then(|v| v.as_u64()) {
                strategy.budget_tokens = Some(bt as usize);
            }
            if let Some(tk) = args.get("tokenizer").and_then(|v| v.as_str()) {
                strategy.tokenizer = tk.to_string();
            }
            if let Some(m) = args.get("max_bodies").and_then(|v| v.as_u64()) {
                strategy.max_bodies = m as usize;
            }
            if let Some(a) = args.get("alpha").and_then(|v| v.as_f64()) {
                strategy.hybrid_alpha = a as f32;
            }

            let seen: HashSet<String> = session_id
                .as_ref()
                .and_then(|sid| app.sessions.get(sid))
                .map(|st| st.seen.clone())
                .unwrap_or_default();

            let pack = build_pack(
                db,
                embedder,
                Some(vec_index),
                task,
                &strategy,
                if session_id.is_some() { Some(&seen) } else { None },
            )?;

            if let Some(sid) = session_id {
                if remember {
                    let st = app.sessions.entry(sid).or_default();
                    for it in &pack.items {
                        st.seen.insert(it.id.clone());
                    }
                    for d in &pack.deferred {
                        st.seen.insert(d.id.clone());
                    }
                }
            }
            let format = args.get("format").and_then(|v| v.as_str()).unwrap_or("text");
            let text = match format {
                "json" => serde_json::to_string_pretty(&pack)?,
                "both" => {
                    let a = render_pack(&pack);
                    let b = serde_json::to_string_pretty(&pack)?;
                    format!("{a}\n---\n{b}")
                }
                _ => render_pack(&pack),
            };
            Ok((text, false))
        }
        #[cfg(feature = "surreal")]
        Backend::Surreal { store, embedder, rt } => {
            let mut strategy = load_strategy_for_pack_surreal(&args)?;

            if let Some(b) = args.get("budget_chars").and_then(|v| v.as_u64()) {
                strategy.budget_chars = b as usize;
            }
            if let Some(bt) = args.get("budget_tokens").and_then(|v| v.as_u64()) {
                strategy.budget_tokens = Some(bt as usize);
            }
            if let Some(tk) = args.get("tokenizer").and_then(|v| v.as_str()) {
                strategy.tokenizer = tk.to_string();
            }
            if let Some(m) = args.get("max_bodies").and_then(|v| v.as_u64()) {
                strategy.max_bodies = m as usize;
            }
            if let Some(a) = args.get("alpha").and_then(|v| v.as_f64()) {
                strategy.hybrid_alpha = a as f32;
            }

            let seen: HashSet<String> = session_id
                .as_ref()
                .and_then(|sid| app.sessions.get(sid))
                .map(|st| st.seen.clone())
                .unwrap_or_default();

            let qvec = embedder.embed_query(task)?;
            let req = PackRequest {
                repo_id: SURREAL_REPO_ID.to_string(),
                query: task.to_string(),
                query_vec: Some(qvec),
                strategy,
                seen: if session_id.is_some() { Some(seen.clone()) } else { None },
            };
            let pack = rt.block_on(store.pack(req))?.pack;

            if let Some(sid) = session_id {
                if remember {
                    let st = app.sessions.entry(sid).or_default();
                    for it in &pack.items {
                        st.seen.insert(it.id.clone());
                    }
                    for d in &pack.deferred {
                        st.seen.insert(d.id.clone());
                    }
                }
            }
            let format = args.get("format").and_then(|v| v.as_str()).unwrap_or("text");
            let text = match format {
                "json" => serde_json::to_string_pretty(&pack)?,
                "both" => {
                    let a = render_pack(&pack);
                    let b = serde_json::to_string_pretty(&pack)?;
                    format!("{a}\n---\n{b}")
                }
                _ => render_pack(&pack),
            };
            Ok((text, false))
        }
    }
}

fn tool_strategy_list(app: &mut App, args: Value) -> Result<(String, bool)> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let show_config = args.get("show_config").and_then(|v| v.as_bool()).unwrap_or(false);

    match &mut app.backend {
        Backend::Sqlite { db, .. } => {
            let rows = db.list_strategies(limit, offset)?;
            if rows.is_empty() {
                return Ok(("No strategies stored.".to_string(), false));
            }

            let mut out = String::new();
            out.push_str(&format!("Strategies (limit={}, offset={}):\n", limit, offset));
            for r in rows {
                out.push_str(&format!(
                    "- {}  name=\"{}\"  score={:?}  parent={:?}  created_at_ms={}\n",
                    short_id(&r.strategy_id),
                    r.name,
                    r.score,
                    r.parent_id,
                    r.created_at_ms
                ));
                if show_config {
                    out.push_str(&indent(&r.config_json, 2));
                    out.push('\n');
                }
            }

            Ok((out, false))
        }
        #[cfg(feature = "surreal")]
        Backend::Surreal { .. } => Ok(("strategy storage not available for Surreal backend".to_string(), true)),
    }
}

fn tool_strategy_get(app: &mut App, args: Value) -> Result<(String, bool)> {
    let id = args.get("id").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("missing id"))?;
    let pretty = args.get("pretty").and_then(|v| v.as_bool()).unwrap_or(true);

    match &mut app.backend {
        Backend::Sqlite { db, .. } => {
            let Some(r) = db.get_strategy(id)? else {
                return Ok((format!("Strategy not found: {id}"), true));
            };

            let mut out = String::new();
            out.push_str(&format!("strategy_id: {}\n", r.strategy_id));
            out.push_str(&format!("name: {}\n", r.name));
            out.push_str(&format!("parent_id: {:?}\n", r.parent_id));
            out.push_str(&format!("score: {:?}\n", r.score));
            out.push_str(&format!("created_at_ms: {}\n", r.created_at_ms));
            out.push_str("config:\n");

            if pretty {
                let v: Value = serde_json::from_str(&r.config_json)?;
                out.push_str(&serde_json::to_string_pretty(&v)?);
                out.push('\n');
            } else {
                out.push_str(&r.config_json);
                out.push('\n');
            }

            Ok((out, false))
        }
        #[cfg(feature = "surreal")]
        Backend::Surreal { .. } => Ok((format!("Strategy not found: {id}"), true)),
    }
}

fn load_strategy_for_pack(db: &Db, args: &Value) -> Result<StrategyConfig> {
    // Base strategy: by id if provided, else defaults.
    let mut cfg = if let Some(id) = args.get("strategy_id").and_then(|v| v.as_str()) {
        let Some(rec) = db.get_strategy(id)? else {
            return Err(anyhow!("strategy not found: {id}"));
        };
        serde_json::from_str::<StrategyConfig>(&rec.config_json)?
    } else {
        StrategyConfig::default()
    };

    // Merge optional overrides (partial JSON object)
    if let Some(ov) = args.get("strategy_overrides") {
        if ov.is_object() {
            let mut base = serde_json::to_value(&cfg)?;
            merge_json(&mut base, ov);
            cfg = serde_json::from_value::<StrategyConfig>(base)?;
        }
    }

    Ok(cfg)
}

fn load_strategy_for_pack_surreal(args: &Value) -> Result<StrategyConfig> {
    if args.get("strategy_id").is_some() {
        return Err(anyhow!("strategy_id not supported for Surreal backend"));
    }
    let mut cfg = StrategyConfig::default();
    if let Some(ov) = args.get("strategy_overrides") {
        if ov.is_object() {
            let mut base = serde_json::to_value(&cfg)?;
            merge_json(&mut base, ov);
            cfg = serde_json::from_value::<StrategyConfig>(base)?;
        }
    }
    Ok(cfg)
}

fn merge_json(dst: &mut Value, src: &Value) {
    match (dst, src) {
        (Value::Object(dst_map), Value::Object(src_map)) => {
            for (k, sv) in src_map {
                match dst_map.get_mut(k) {
                    Some(dv) => merge_json(dv, sv),
                    None => {
                        dst_map.insert(k.clone(), sv.clone());
                    }
                }
            }
        }
        (d, s) => {
            *d = s.clone();
        }
    }
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 { id.to_string() } else { id[..12].to_string() }
}

fn build_pack(
    db: &Db,
    embedder: &Embedder,
    vec_index: Option<&VecIndex>,
    task: &str,
    strategy: &StrategyConfig,
    seen: Option<&HashSet<String>>,
) -> Result<ce_core::model::ContextPack> {
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

    let mut cands: Vec<Candidate> = Vec::new();
    for (rid, sc, why) in ranked.into_iter().take(strategy.candidate_pool_limit) {
        let frag = db.get_fragment_by_rowid(rid)?;

        // Apply a session-level “seen” penalty to reduce repetition.
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

        // Compute symbol-focused tokens for slicing (intersection of task tokens and this fragment's refs).
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
            if let Some((slice_reason, slice_text)) = compute_best_slice(&frag, &file_line_hints, &task_tokens, &focus_tokens, strategy) {
                let decorated = decorate_slice(&frag, &slice_reason, &slice_text);
                let full_toks = token_counter.count(&full_body);
                let slice_toks = token_counter.count(&decorated);
                if full_toks.saturating_sub(slice_toks) >= strategy.body_snippet_min_savings_tokens {
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

    let redundancy_pct = if let Some(seen_set) = seen {
        let repeated = pack.items.iter().filter(|it| seen_set.contains(&it.id)).count();
        if pack.items.is_empty() { 0.0 } else { (repeated as f32 / pack.items.len() as f32) * 100.0 }
    } else {
        0.0
    };
    pack.metrics.redundancy_pct = Some(redundancy_pct);

    if let Some(baseline_tokens) = compute_baseline_tokens(db, task, &signal_bundle, &token_counter)? {
        pack.metrics.baseline_tokens_total = Some(baseline_tokens);
        if baseline_tokens > 0 {
            let saved = (baseline_tokens as f32 - pack.used_tokens as f32) / baseline_tokens as f32;
            pack.metrics.saved_pct = Some(saved * 100.0);
        }
    }

    if let Some(recipe_excerpt) = build_recipe_excerpt(db, task, strategy, &token_counter)? {
        pack.recipe_excerpt = Some(recipe_excerpt);
    }

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
        neighbors.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
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
        let mut rec_tokens: Vec<String> = rec.tokens.split_whitespace().map(|s| s.to_string()).collect();
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
        let block = format!("- recipe #{} (sim {:.2})\n  - failure: {}\n  - pack: {}\n  - patch: {}\n", rec.recipe_id, sim, failure, pack, patch);
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
    if allow_signals || allow_ast {
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

    // A) signal-driven slice
    if allow_signals {
        if !targets.is_empty() {
            if let Some(s) = snippet::slice_by_file_lines(&frag.body, frag.span.start_line, &targets, ctx, max_lines) {
                let frag_path = frag.file.display().to_string();
                let head = targets.get(0).copied().unwrap_or(0);
                let reason = format!("signal:{}:{}", frag_path, head);
                return Some((reason, s));
            }
        }
    }

    // B) symbol-focused grep slice (narrower than full task token grep)
    if allow_symbols {
        if let Some(s) = snippet::slice_by_grep(&frag.body, frag.span.start_line, focus_tokens, ctx, max_lines) {
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
        let toks: &[String] = if !focus_tokens.is_empty() { focus_tokens } else { task_tokens };
        if let Some(s) = ce_lang_rust::ast_prune_slice(&frag.body, frag.span.start_line, &targets, toks, ctx, max_lines) {
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
        let toks: &[String] = if !focus_tokens.is_empty() { focus_tokens } else { task_tokens };
        if let Some(s) = ce_lang_rust::ast_skeleton_slice(&frag.body, frag.span.start_line, frag.kind, &targets, toks, cfg) {
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
        if let Some(s) = snippet::skeletonize_tsx(&frag.body, cfg.tsx_skeleton_max_depth, cfg.tsx_skeleton_max_props) {
            let reason = "tsx_skeleton".to_string();
            return Some((reason, s));
        }
    }

    // F) SwiftUI skeletonization
    if allow_swiftui {
        if let Some(s) = snippet::skeletonize_swiftui(&frag.body, cfg.swiftui_skeleton_max_depth, cfg.swiftui_skeleton_max_modifiers) {
            let reason = "swiftui_skeleton".to_string();
            return Some((reason, s));
        }
    }

    // G) token-grep slice
    if allow_query {
        if let Some(s) = snippet::slice_by_grep(&frag.body, frag.span.start_line, task_tokens, ctx, max_lines) {
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

fn head_slice(frag: &ce_core::model::Fragment, max_lines: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for (i, line) in frag.body.lines().enumerate() {
        if n >= max_lines {
            // Indent-aware placeholder.
            let mut ws_end = 0usize;
            for (j, ch) in line.char_indices() {
                if ch == ' ' || ch == '\t' {
                    ws_end = j + ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push_str(&format!("{}...\n", &line[..ws_end]));
            break;
        }
        let file_line_1based = frag.span.start_line.saturating_add(i as u32).saturating_add(1);
        out.push_str(&format!("L{:>5}: {}\n", file_line_1based, line));
        n += 1;
    }
    out
}

fn render_pack(pack: &ce_core::model::ContextPack) -> String {
    let mut out = String::new();
    out.push_str("[ce-pack v0]\n");
    out.push_str(&format!("pack_id: {}\n", pack.pack_id));
    out.push_str(&format!("budget_chars: {}\n", pack.budget_chars));
    out.push_str(&format!("used_chars: {}\n", pack.used_chars));
    if let Some(bt) = pack.budget_tokens {
        out.push_str(&format!("budget_tokens: {}\n", bt));
    }
    out.push_str(&format!("used_tokens: {}\n", pack.used_tokens));
    out.push_str(&format!("pack_tokens_total: {}\n", pack.metrics.pack_tokens_total));
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
    out.push_str(&format!("unbound_symbol_count: {}\n\n", pack.metrics.unbound_symbol_count));

    if let Some(recipe) = &pack.recipe_excerpt {
        out.push_str("## Recipe Memory\n\n");
        out.push_str(recipe);
        out.push_str("\n\n");
    }

    out.push_str("## Included\n\n");
    for it in &pack.items {
        out.push_str(&format!("### {} ({:?}, score={:.3}, reason={})\n\n", it.id, it.view, it.score, it.reason));
        out.push_str(&it.content);
        out.push_str("\n\n");
    }

    if !pack.unresolved_symbols.is_empty() {
        out.push_str("\n## Unresolved Symbols\n\n");
        for sym in &pack.unresolved_symbols {
            let reason = sym.reason.clone().unwrap_or_else(|| "unresolved".to_string());
            out.push_str(&format!("- {} ({})\n", sym.symbol, reason));
        }
    }

    if !pack.deferred.is_empty() {
        out.push_str("## Deferred\n\n");
        for d in &pack.deferred {
            let span = format!("L{}-L{}", d.span.start_line.saturating_add(1), d.span.end_line.saturating_add(1));
            let sym = d.symbol.clone().unwrap_or_default();
            if sym.is_empty() {
                out.push_str(&format!("- {} {:?} {} [{}] ({})\n", d.id, d.kind, d.path, span, d.reason));
            } else {
                out.push_str(&format!("- {} {:?} {} [{}] sym={} ({})\n", d.id, d.kind, d.path, span, sym, d.reason));
            }
        }
    }

    out
}

fn indent(s: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    s.lines().map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn set_fastembed_cache_dir() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let cache_dir = repo_root.join(".fastembed_cache");
        if cache_dir.exists() {
            std::env::set_var("FASTEMBED_CACHE_DIR", cache_dir);
        }
    }

    fn temp_app() -> Result<(TempDir, App)> {
        set_fastembed_cache_dir();
        let dir = TempDir::new()?;
        let db_path = dir.path().join("index.sqlite");
        let db = Db::open(&db_path)?;
        let embedder = Embedder::new(ce_store::embed::DEFAULT_MODEL)?;
        let vec_index = VecIndex::new(1, 1);
        Ok((dir, App { db, embedder, vec_index, sessions: HashMap::new() }))
    }

    #[test]
    fn initialize_returns_protocol() -> Result<()> {
        let (_dir, mut app) = temp_app()?;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let resp = handle_message(&mut app, msg)?.expect("expected response");
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        Ok(())
    }

    #[test]
    fn tools_list_includes_pack() -> Result<()> {
        let (_dir, mut app) = temp_app()?;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let resp = handle_message(&mut app, msg)?.expect("expected response");
        let tools = resp["result"]["tools"].as_array().expect("tools list");
        assert!(tools.iter().any(|tool| tool["name"] == "context.pack"));
        Ok(())
    }
}
