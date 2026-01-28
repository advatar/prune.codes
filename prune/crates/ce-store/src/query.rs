use anyhow::Result;
use ce_core::model::{SignalBundle, StrategyConfig};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::types::SearchHit;
use crate::{Db, Embedder, VecIndex};

/// Default basename used for persisting the HNSW dump (`{base}.hnsw.{graph,data}`).
pub const DEFAULT_HNSW_BASE: &str = "fragments";

/// A small stop-list of identifiers that frequently appear in Rust code and
/// tend to explode the graph expansion with unhelpful “definitions”.
///
/// This is intentionally conservative. You can tune it via StrategyConfig later
/// (or evolve it in your DGM loop).
fn is_stop_ref(s: &str) -> bool {
    matches!(
        s,
        // Rust keywords / module roots
        "self" | "Self" | "super" | "crate" | "std" | "core" | "alloc" |
        // Very common constructors / methods
        "new" | "default" | "len" | "iter" | "into_iter" | "as_ref" | "as_mut" |
        // Common enums / variants
        "Ok" | "Err" | "Some" | "None" |
        // Common prelude-ish types
        "Result" | "Option" | "Vec" | "String" |
        // Primitive-ish names
        "str" | "bool" | "char" |
        "u8" | "u16" | "u32" | "u64" | "usize" |
        "i8" | "i16" | "i32" | "i64" | "isize" |
        "f32" | "f64" |
        // Traits that show up everywhere
        "Clone" | "Copy" | "Debug" | "Default" | "Send" | "Sync" |
        "Into" | "From" | "TryFrom" | "Iterator" | "IntoIterator" |
        // Common collection names
        "HashMap" | "HashSet"
    )
}

fn is_good_ref(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.len() < 2 || s.len() > 80 {
        return false;
    }
    if is_stop_ref(s) {
        return false;
    }
    // numeric-only tokens are rarely useful
    if s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Skip obvious punctuation-y things (tree-sitter should avoid these, but be safe)
    if s.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    true
}

/// Sanitize arbitrary task/error text into an FTS5-friendly query.
///
/// SQLite FTS5 uses its own query syntax; raw compiler output can contain
/// punctuation that triggers parse errors. This function extracts simple
/// identifier-ish tokens and joins them with spaces.
fn sanitize_fts_query(text: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if cur.len() >= 2 {
                tokens.push(cur.clone());
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        tokens.push(cur);
    }
    tokens.truncate(64);
    tokens.join(" ")
}

#[derive(Debug, Clone)]
struct Accum {
    score: f32,
    reasons: Vec<String>,
}

impl Accum {
    fn new() -> Self {
        Self {
            score: 0.0,
            reasons: Vec::new(),
        }
    }

    fn add(&mut self, delta: f32, reason: impl Into<String>) {
        self.score += delta;
        // Avoid unbounded growth: keep the first few reasons.
        if self.reasons.len() < 6 {
            self.reasons.push(reason.into());
        }
    }

    /// Ensure the accumulated score is at least `target`.
    ///
    /// This is useful for injecting additional candidates (e.g. file-level ApiSummary
    /// fragments) with a minimum desirability score.
    fn ensure_at_least(&mut self, target: f32, reason: impl Into<String>) {
        if target > self.score {
            self.score = target;
            if self.reasons.len() < 6 {
                self.reasons.push(reason.into());
            }
        }
    }

    fn reason_string(&self) -> String {
        if self.reasons.is_empty() {
            "".to_string()
        } else {
            self.reasons.join("; ")
        }
    }
}

/// Rebuild an in-process HNSW index from all embeddings stored in SQLite.
///
/// MVP behavior:
/// - loads all embeddings into memory
/// - rebuilds HNSW on each invocation
///
/// Note: for very large repos, consider streaming or persisting a dump and
/// supporting incremental updates.
pub fn build_hnsw_from_db(db: &Db) -> Result<(VecIndex, HashMap<i64, Vec<f32>>)> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT rowid, vec, dim FROM embeddings")?;
    let rows = stmt.query_map([], |r| {
        let rowid: i64 = r.get(0)?;
        let blob: Vec<u8> = r.get(1)?;
        let dim: i64 = r.get(2)?;
        Ok((rowid, blob, dim))
    })?;

    let mut all: Vec<(i64, Vec<f32>)> = Vec::new();
    let mut dim_usize: usize = 0;

    for rr in rows {
        let (rowid, blob, dim) = rr?;
        dim_usize = dim as usize;
        let vec: &[f32] = bytemuck::cast_slice(&blob);
        all.push((rowid, vec.to_vec()));
    }

    let mut index = VecIndex::new(dim_usize.max(1), all.len().max(1));
    let mut map: HashMap<i64, Vec<f32>> = HashMap::new();

    for (rid, v) in all {
        index.insert(rid as usize, &v);
        map.insert(rid, v);
    }

    Ok((index, map))
}

/// Try to load an HNSW dump from disk, falling back to rebuilding from embeddings.
///
/// This is designed for CLIs and servers that want fast startup and don't want to
/// rebuild the ANN index on every run.
///
/// Staleness check (best-effort):
/// - compares dump `nb_point` to `SELECT COUNT(*) FROM embeddings`.
/// - if they mismatch, the dump is considered stale and we rebuild.
pub fn load_or_build_hnsw(db: &Db, hnsw_dir: &Path, base: &str, mmap: bool) -> Result<VecIndex> {
    // Ensure repo + embedding meta is populated; this makes staleness checks
    // cheap and durable across process restarts.
    let repo_hash = match db.get_meta("repo.state_hash")? {
        Some(h) => h,
        None => db.update_repo_meta()?,
    };
    let emb_meta = db.update_embeddings_meta()?;

    // Fast path: load a compatible dump.
    if VecIndex::dump_exists(hnsw_dir, base) {
        if let Ok(desc) = VecIndex::dump_description(hnsw_dir, base) {
            let mut ok = true;

            // Base compatibility checks.
            if desc.dimension == 0 || emb_meta.dim == 0 {
                ok = false;
            }
            if desc.dimension != emb_meta.dim {
                ok = false;
            }
            if desc.nb_point != emb_meta.total_count {
                ok = false;
            }

            // Meta-based staleness checks (more robust than count alone).
            if let Some(h) = db.get_meta("hnsw.repo_state_hash")? {
                if h != repo_hash {
                    ok = false;
                }
            }
            if let Some(h) = db.get_meta("hnsw.embeddings_state_hash")? {
                if h != emb_meta.state_hash {
                    ok = false;
                }
            }
            if let Some(m) = db.get_meta("hnsw.model")? {
                if !m.is_empty() && m != emb_meta.model {
                    ok = false;
                }
            }
            if let Some(d) = db.get_meta("hnsw.dim")? {
                if let Ok(dim) = d.parse::<usize>() {
                    if dim > 0 && dim != emb_meta.dim {
                        ok = false;
                    }
                }
            }

            if ok {
                if let Ok(ix) = VecIndex::try_load(hnsw_dir, base, mmap) {
                    return Ok(ix);
                }
            }
        }
    }

    // Slow path: rebuild from SQLite embeddings and dump for next time.
    let (ix, _map) = build_hnsw_from_db(db)?;
    let _ = std::fs::create_dir_all(hnsw_dir);
    let _ = ix.dump(hnsw_dir, base);

    // Persist dump metadata for future compatibility/staleness checks.
    db.set_meta("hnsw.base", base)?;
    db.set_meta_usize("hnsw.nb_points", emb_meta.total_count)?;
    db.set_meta_usize("hnsw.dim", emb_meta.dim)?;
    db.set_meta("hnsw.model", &emb_meta.model)?;
    db.set_meta("hnsw.repo_state_hash", &repo_hash)?;
    db.set_meta("hnsw.embeddings_state_hash", &emb_meta.state_hash)?;
    db.set_meta_i64("hnsw.dump_created_at_ms", Db::now_ms())?;

    Ok(ix)
}

/// Like `hybrid_scores_map` but allows the caller to provide a prebuilt semantic index.
///
/// This is useful for long-running processes (e.g. the MCP server) so we don't rebuild
/// HNSW for every request.
fn hybrid_scores_map_with_index(
    db: &Db,
    embedder: &Embedder,
    vec_index: Option<&VecIndex>,
    query: &str,
    lexical_k: usize,
    semantic_k: usize,
    alpha: f32,
) -> Result<HashMap<i64, Accum>> {
    let mut scores: HashMap<i64, Accum> = HashMap::new();

    // Lexical candidates
    let lex = match db.search_fts(query, lexical_k) {
        Ok(v) => v,
        Err(_e) => {
            let q2 = sanitize_fts_query(query);
            if q2.trim().is_empty() {
                vec![]
            } else {
                db.search_fts(&q2, lexical_k).unwrap_or_default()
            }
        }
    };
    for (rid, sc) in lex {
        scores
            .entry(rid)
            .or_insert_with(Accum::new)
            .add((1.0 - alpha) * sc, "lexical");
    }

    // Semantic candidates
    if semantic_k > 0 {
        // Use caller-provided index if present; otherwise rebuild from DB (MVP).
        let owned;
        let vi: &VecIndex = if let Some(v) = vec_index {
            v
        } else {
            owned = build_hnsw_from_db(db)?;
            &owned.0
        };

        if vi.dim > 0 {
            let qv = embedder.embed_query(query)?;
            let nn = vi.search(&qv, semantic_k, 64);
            for n in nn {
                // DistCosine: smaller is closer
                let sim = 1.0 - (n.distance as f32);
                let sim = sim.max(0.0);
                scores
                    .entry(n.d_id as i64)
                    .or_insert_with(Accum::new)
                    .add(alpha * sim, "semantic");
            }
        }
    }

    Ok(scores)
}

/// Hybrid search returning fully hydrated SearchHit records.
pub fn hybrid_search(
    db: &Db,
    embedder: &Embedder,
    query: &str,
    lexical_k: usize,
    semantic_k: usize,
    k: usize,
    alpha: f32,
) -> Result<Vec<SearchHit>> {
    hybrid_search_with_index(db, embedder, None, query, lexical_k, semantic_k, k, alpha)
}

/// Hybrid search with an optional prebuilt semantic index.
pub fn hybrid_search_with_index(
    db: &Db,
    embedder: &Embedder,
    vec_index: Option<&VecIndex>,
    query: &str,
    lexical_k: usize,
    semantic_k: usize,
    k: usize,
    alpha: f32,
) -> Result<Vec<SearchHit>> {
    let scores =
        hybrid_scores_map_with_index(db, embedder, vec_index, query, lexical_k, semantic_k, alpha)?;

    let mut ranked: Vec<(i64, f32)> = scores.into_iter().map(|(rid, a)| (rid, a.score)).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(k);

    let rowids: Vec<i64> = ranked.iter().map(|(rid, _)| *rid).collect();
    let mut hits = db.search_hits_by_rowids(&rowids)?;

    let map: HashMap<i64, f32> = ranked.into_iter().collect();
    for h in hits.iter_mut() {
        h.score = *map.get(&h.rowid).unwrap_or(&0.0);
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(hits)
}

/// Build a candidate pool for packing:
///
/// 1) hybrid retrieval (lexical + semantic)
/// 2) optional graph-ish expansion using:
///    - same-file neighbors
///    - symbol definition lookup for referenced identifiers
///
/// Returns a ranked list of (rowid, score, reason_string).
pub fn candidate_rowids_for_pack(
    db: &Db,
    embedder: &Embedder,
    task: &str,
    cfg: &StrategyConfig,
) -> Result<Vec<(i64, f32, String)>> {
    candidate_rowids_for_pack_with_index(db, embedder, None, task, cfg, None)
}

/// Candidate retrieval for packing with an optional prebuilt semantic index.
pub fn candidate_rowids_for_pack_with_index(
    db: &Db,
    embedder: &Embedder,
    vec_index: Option<&VecIndex>,
    task: &str,
    cfg: &StrategyConfig,
    signals: Option<&SignalBundle>,
) -> Result<Vec<(i64, f32, String)>> {
    let mut scores = hybrid_scores_map_with_index(
        db,
        embedder,
        vec_index,
        task,
        cfg.lexical_k,
        cfg.semantic_k,
        cfg.hybrid_alpha,
    )?;

    // Signals first: direct file:line hints are high-precision and should seed expansion.
    if cfg.signals_enabled {
        let span_cap = cfg.signal_max_spans.max(cfg.signal_file_line_max);
        let path_cap = cfg.signal_max_paths.max(1);
        let owned;
        let bundle = if let Some(b) = signals {
            b
        } else {
            owned = ce_core::signals::extract_signals(task, span_cap, path_cap);
            &owned
        };
        add_signal_boosts(db, bundle, &mut scores, cfg)?;
    }

    if cfg.graph_expand {
        expand_graph(db, task, &mut scores, cfg)?;
    }

    // Optional: inject file-level ApiSummary fragments for the most relevant files.
    // This helps produce a cheap overview in large repos.
    if cfg.include_api_summaries {
        inject_api_summaries(db, &mut scores, cfg)?;
    }

    let mut ranked: Vec<(i64, f32, String)> = scores
        .into_iter()
        .map(|(rid, a)| (rid, a.score, a.reason_string()))
        .collect();

    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(cfg.candidate_pool_limit.max(1));
    Ok(ranked)
}

fn inject_api_summaries(
    db: &Db,
    scores: &mut HashMap<i64, Accum>,
    cfg: &StrategyConfig,
) -> Result<()> {
    use std::collections::HashMap as StdHashMap;

    if cfg.api_summary_max == 0 {
        return Ok(());
    }

    // Rank existing candidates by score.
    let mut ranked: Vec<(i64, f32)> = scores.iter().map(|(rid, a)| (*rid, a.score)).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let scan_n = cfg.api_summary_scan_top_n.max(1).min(ranked.len());
    ranked.truncate(scan_n);

    let rowids: Vec<i64> = ranked.iter().map(|(rid, _)| *rid).collect();
    let path_map = db.paths_for_rowids(&rowids)?;

    // Best score per path among the scanned candidates.
    let mut best_by_path: StdHashMap<String, f32> = StdHashMap::new();
    for (rid, sc) in ranked {
        if let Some(path) = path_map.get(&rid) {
            let e = best_by_path.entry(path.clone()).or_insert(sc);
            if sc > *e {
                *e = sc;
            }
        }
    }

    let mut paths: Vec<(String, f32)> = best_by_path.into_iter().collect();
    paths.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    paths.truncate(cfg.api_summary_max);

    for (path, base_score) in paths {
        if let Some(sum_rowid) = db.api_summary_rowid_for_path(&path)? {
            let target = base_score * cfg.api_summary_score_mul + cfg.api_summary_score_bonus;
            scores
                .entry(sum_rowid)
                .or_insert_with(Accum::new)
                .ensure_at_least(target, format!("api-summary:{path}"));
        }
    }
    Ok(())
}

fn add_signal_boosts(
    db: &Db,
    signals: &SignalBundle,
    scores: &mut HashMap<i64, Accum>,
    cfg: &StrategyConfig,
) -> Result<()> {
    let span_boost = if cfg.signal_span_boost > 0.0 {
        cfg.signal_span_boost
    } else {
        cfg.signal_file_line_boost
    };

    for span in &signals.spans {
        let rowids = db.fragment_rowids_covering_line(&span.path, span.line, 2)?;
        for rid in rowids {
            scores.entry(rid).or_insert_with(Accum::new).add(
                span_boost,
                format!("signal:span:{}:{}", span.path, span.line),
            );
        }
    }

    let mut path_hints: HashSet<String> = HashSet::new();
    for diff in &signals.diffs {
        for p in &diff.changed_paths {
            path_hints.insert(p.clone());
        }
    }
    for span in &signals.spans {
        path_hints.insert(span.path.clone());
    }
    for module in &signals.modules {
        if looks_like_path(&module.specifier) {
            path_hints.insert(module.specifier.clone());
        }
    }

    if !path_hints.is_empty() {
        let path_boost = span_boost * 0.6;
        for path in path_hints {
            if let Some(sum_rowid) = db.api_summary_rowid_for_path(&path)? {
                scores
                    .entry(sum_rowid)
                    .or_insert_with(Accum::new)
                    .add(path_boost, format!("signal:path:{}", path));
            }
            for rid in db.fragment_rowids_for_path(&path, 2)? {
                scores
                    .entry(rid)
                    .or_insert_with(Accum::new)
                    .add(path_boost, format!("signal:path:{}", path));
            }
        }
    }

    let sym_boost = span_boost * 0.8;
    for sym in &signals.symbols {
        let defs = db.resolve_symbol_defs(&sym.name, 3)?;
        for rid in defs {
            scores
                .entry(rid)
                .or_insert_with(Accum::new)
                .add(sym_boost, format!("signal:symbol:{}", sym.name));
        }
    }

    let test_boost = span_boost * 0.5;
    for test in &signals.tests {
        let defs = db.resolve_symbol_defs(&test.name, 2)?;
        for rid in defs {
            scores
                .entry(rid)
                .or_insert_with(Accum::new)
                .add(test_boost, format!("signal:test:{}", test.name));
        }
    }

    Ok(())
}

fn looks_like_path(path: &str) -> bool {
    let p = path.trim();
    if p.is_empty() {
        return false;
    }
    if p.contains("http://") || p.contains("https://") {
        return false;
    }
    p.contains('/') || (p.contains('.') && p.len() > 2)
}

fn expand_graph(
    db: &Db,
    task: &str,
    scores: &mut HashMap<i64, Accum>,
    cfg: &StrategyConfig,
) -> Result<()> {
    // pick seed rowids by current score
    let mut seed: Vec<(i64, f32)> = scores.iter().map(|(rid, a)| (*rid, a.score)).collect();
    seed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    seed.truncate(cfg.graph_seed_k.max(1));

    // Optional: prefetch seed paths for file-summary hub expansion.
    let seed_paths = if cfg.file_summary_hub {
        let seed_rids: Vec<i64> = seed.iter().map(|(rid, _)| *rid).collect();
        db.paths_for_rowids(&seed_rids)?
    } else {
        std::collections::HashMap::new()
    };

    let mut expanded_file_summaries: std::collections::HashSet<i64> =
        std::collections::HashSet::new();

    for (seed_rid, seed_score) in seed {
        if seed_score < 0.05 {
            continue;
        }

        // A) same-file neighbors
        for nb in db.neighbors_in_file(seed_rid, cfg.neighbors_k)? {
            let delta = seed_score * cfg.neighbor_weight;
            scores
                .entry(nb)
                .or_insert_with(Accum::new)
                .add(delta, format!("neighbor-of:{}", seed_rid));
        }

        // B) definition lookup for referenced identifiers
        // Pull more than needed so filtering doesn't starve us.
        let raw_refs =
            db.refs_for_fragment(seed_rid, (cfg.refs_per_seed * 6).max(cfg.refs_per_seed))?;
        let mut refs: Vec<String> = raw_refs.into_iter().filter(|r| is_good_ref(r)).collect();

        // Prefer refs that literally appear in the task/error text.
        refs.sort_by_key(|r| if task.contains(r) { 0 } else { 1 });
        refs.dedup();
        refs.truncate(cfg.refs_per_seed.max(1));

        for r in refs {
            let defs = db.resolve_symbol_defs_near(&r, seed_rid, cfg.defs_per_ref)?;
            for def_rid in defs {
                if def_rid == seed_rid {
                    continue;
                }
                let delta = seed_score * cfg.def_weight;
                scores
                    .entry(def_rid)
                    .or_insert_with(Accum::new)
                    .add(delta, format!("def-of:{} (from {})", r, seed_rid));
            }
        }

        // C) explicit edge-based subgraph expansion (multi-hop)
        let max_r = cfg
            .edge_refers_radius
            .max(cfg.edge_module_radius)
            .max(cfg.edge_reverse_radius)
            .max(cfg.edge_radius);
        if max_r > 0 {
            expand_edges_bfs(db, scores, seed_rid, seed_score, cfg)?;

            // Additionally seed from the file-level ApiSummary node, which is where
            // file-level edges (e.g. Rust `mod`/`use`) attach.
            if cfg.file_summary_hub {
                if let Some(path) = seed_paths.get(&seed_rid) {
                    if let Some(sum_rid) = db.api_summary_rowid_for_path(path)? {
                        if sum_rid != seed_rid {
                            let hub_score = seed_score * cfg.file_summary_hub_weight;
                            scores
                                .entry(sum_rid)
                                .or_insert_with(Accum::new)
                                .add(hub_score, format!("file-summary-of:{}", seed_rid));

                            if expanded_file_summaries.insert(sum_rid) {
                                expand_edges_bfs(db, scores, sum_rid, hub_score, cfg)?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn expand_edges_bfs(
    db: &Db,
    scores: &mut HashMap<i64, Accum>,
    seed_rid: i64,
    seed_score: f32,
    cfg: &StrategyConfig,
) -> Result<()> {
    use std::collections::{HashSet, VecDeque};

    // We keep the original `edge_radius` knob for backwards compatibility,
    // but allow per-edge-type radii and multipliers so strategies can
    // constrain module graphs without disabling definition edges.
    let max_radius = cfg
        .edge_refers_radius
        .max(cfg.edge_module_radius)
        .max(cfg.edge_reverse_radius)
        .max(cfg.edge_radius);
    let max_nodes = cfg.edge_max_nodes_per_seed.max(1);
    let max_edges = cfg.edge_max_edges_per_node;

    let mut visited: HashSet<i64> = HashSet::new();
    visited.insert(seed_rid);

    // (node, depth)
    let mut q: VecDeque<(i64, usize)> = VecDeque::new();
    q.push_back((seed_rid, 0));

    fn edge_radius_for(cfg: &StrategyConfig, ty: &str) -> usize {
        match ty {
            "refers" | "jsx_uses" => cfg.edge_refers_radius,
            "mod" | "use" | "ts_import" => cfg.edge_module_radius,
            "imported_by" | "modded_by" | "ts_imported_by" | "jsx_used_by" => {
                cfg.edge_reverse_radius
            }
            _ => cfg.edge_radius,
        }
    }

    fn edge_mul_for(cfg: &StrategyConfig, ty: &str) -> f32 {
        match ty {
            "refers" | "jsx_uses" => cfg.edge_mul_refers,
            "mod" => cfg.edge_mul_mod,
            "use" | "ts_import" => cfg.edge_mul_use,
            "imported_by" | "ts_imported_by" | "jsx_used_by" => cfg.edge_mul_imported_by,
            "modded_by" => cfg.edge_mul_modded_by,
            _ => cfg.edge_mul_other,
        }
    }

    fn edge_priority(ty: &str) -> u8 {
        match ty {
            "refers" | "jsx_uses" => 0,
            "mod" => 1,
            "use" | "ts_import" => 2,
            "imported_by" | "modded_by" | "ts_imported_by" | "jsx_used_by" => 3,
            _ => 4,
        }
    }

    while let Some((node, depth)) = q.pop_front() {
        if depth >= max_radius {
            continue;
        }
        let nd = depth + 1;
        let decay = cfg.edge_hop_decay.powi(nd as i32);

        if cfg.edge_include_outgoing {
            let mut outs = db.edges_outgoing(node, max_edges)?;
            if cfg.edge_prioritize_by_type {
                outs.sort_by(|a, b| {
                    let pa = edge_priority(&a.1);
                    let pb = edge_priority(&b.1);
                    if pa != pb {
                        return pa.cmp(&pb);
                    }
                    b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                });
            }

            for (to, ty, w) in outs {
                let r = edge_radius_for(cfg, &ty);
                if r == 0 || nd > r {
                    continue;
                }
                if visited.len() >= max_nodes {
                    break;
                }
                if visited.insert(to) {
                    let mul = edge_mul_for(cfg, &ty);
                    let delta = seed_score * cfg.edge_out_weight * decay * w * mul;
                    scores
                        .entry(to)
                        .or_insert_with(Accum::new)
                        .add(delta, format!("edge-out:{} d{} from {}", ty, nd, seed_rid));
                    q.push_back((to, nd));
                }
            }
        }

        if cfg.edge_include_incoming {
            let mut ins = db.edges_incoming(node, max_edges)?;
            if cfg.edge_prioritize_by_type {
                ins.sort_by(|a, b| {
                    let pa = edge_priority(&a.1);
                    let pb = edge_priority(&b.1);
                    if pa != pb {
                        return pa.cmp(&pb);
                    }
                    b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                });
            }

            for (from, ty, w) in ins {
                let r = edge_radius_for(cfg, &ty);
                if r == 0 || nd > r {
                    continue;
                }
                if visited.len() >= max_nodes {
                    break;
                }
                if visited.insert(from) {
                    let mul = edge_mul_for(cfg, &ty);
                    let delta = seed_score * cfg.edge_in_weight * decay * w * mul;
                    scores
                        .entry(from)
                        .or_insert_with(Accum::new)
                        .add(delta, format!("edge-in:{} d{} from {}", ty, nd, seed_rid));
                    q.push_back((from, nd));
                }
            }
        }
    }

    Ok(())
}
