use crate::{graph, SurrealStore};
use anyhow::{anyhow, Result};
use ce_core::model::{FragKind, FragmentView, SignalBundle, StrategyConfig};
use ce_core::pack::{pack_with_strategy, Candidate, CandidateNeighbor};
use ce_core::signals;
use ce_core::snippet;
use ce_core::tokenizer::TokenCounter;
use ce_store_core::{CeStore, PackRequest, PackResult};
use std::collections::{HashMap, HashSet};
use surrealdb::sql::Thing;

pub async fn build_pack(store: &SurrealStore, req: PackRequest) -> Result<PackResult> {
    let query_vec = req
        .query_vec
        .as_ref()
        .ok_or_else(|| anyhow!("pack requires query_vec"))?;

    let span_cap = req
        .strategy
        .signal_max_spans
        .max(req.strategy.signal_file_line_max);
    let path_cap = req.strategy.signal_max_paths.max(1);
    let signal_bundle = signals::extract_signals(&req.query, span_cap, path_cap);

    let base_k = req.strategy.candidate_pool_limit.max(1);
    let mut scores: HashMap<String, Accum> = HashMap::new();

    let hits = store
        .hybrid_search_rrf(&req.repo_id, &req.query, query_vec, base_k)
        .await?;
    for h in hits {
        scores
            .entry(h.frag_id)
            .or_insert_with(Accum::new)
            .add(h.score, h.reason);
    }

    if req.strategy.signals_enabled {
        add_signal_boosts(
            store,
            &req.repo_id,
            &signal_bundle,
            &mut scores,
            &req.strategy,
        )
        .await?;
    }

    if req.strategy.graph_expand {
        expand_edges(store, &req.repo_id, &mut scores, &req.strategy).await?;
    }

    let mut ranked: Vec<(String, f32, String)> = scores
        .into_iter()
        .map(|(id, acc)| (id, acc.score, acc.reason_string()))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(req.strategy.candidate_pool_limit.max(1));

    let ids: Vec<String> = ranked.iter().map(|(id, _, _)| id.clone()).collect();
    let frags = store.fetch_fragments(&req.repo_id, &ids).await?;

    let mut frag_by_id: HashMap<String, ce_store_core::FragmentRecord> = HashMap::new();
    for f in frags {
        frag_by_id.insert(f.frag_id.clone(), f);
    }

    let file_line_hints = ce_core::util::extract_file_line_hints(&req.query, span_cap);
    let task_tokens = ce_core::util::extract_ident_tokens(&req.query);
    let token_counter = TokenCounter::new(&req.strategy.tokenizer);

    let mut cands: Vec<Candidate> = Vec::new();
    for (fid, score, reason) in ranked {
        let Some(frag_rec) = frag_by_id.get(&fid) else {
            continue;
        };
        let frag = frag_rec.to_fragment();
        let mut score = score;
        let mut reason = reason;
        if req.strategy.avoid_seen {
            if let Some(seen) = req.seen.as_ref() {
                if seen.contains(&frag.id) {
                    score *= req.strategy.seen_score_mul;
                    reason = format!("{reason};seen");
                }
            }
        }

        let signature = decorate_signature(&frag);

        let mut focus_tokens: Vec<String> = Vec::new();
        for r in &frag.refs {
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

        let full_body = decorate_body(&frag);
        let mut body_view = FragmentView::Body;
        let mut body_text = full_body.clone();

        if req.strategy.body_snippet_mode != "full" {
            if let Some((slice_reason, slice_text)) = compute_best_slice(
                &frag,
                &file_line_hints,
                &task_tokens,
                &focus_tokens,
                &req.strategy,
            ) {
                let decorated = decorate_slice(&frag, &slice_reason, &slice_text);
                let full_toks = token_counter.count(&full_body);
                let slice_toks = token_counter.count(&decorated);
                if full_toks.saturating_sub(slice_toks)
                    >= req.strategy.body_snippet_min_savings_tokens
                {
                    body_view = FragmentView::Slice;
                    body_text = decorated;
                }
            }
        }

        cands.push(Candidate {
            id: frag.id.clone(),
            rowid: 0,
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

    attach_candidate_neighbors(store, &req.repo_id, &req.strategy, &mut cands).await?;

    let mut pack = pack_with_strategy(&req.strategy, cands);
    let (signals_used, signals_used_stats) = signals::signals_used(&signal_bundle, &pack.items);
    pack.signals = signal_bundle.clone();
    pack.signals_used = signals_used;
    pack.metrics.signals_extracted = signals::signal_stats(&pack.signals);
    pack.metrics.signals_used = signals_used_stats;

    let redundancy_pct = if let Some(seen) = req.seen.as_ref() {
        let repeated = pack.items.iter().filter(|it| seen.contains(&it.id)).count();
        if pack.items.is_empty() {
            0.0
        } else {
            (repeated as f32 / pack.items.len() as f32) * 100.0
        }
    } else {
        0.0
    };
    pack.metrics.redundancy_pct = Some(redundancy_pct);

    Ok(PackResult {
        pack,
        debug: serde_json::json!({}),
    })
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
        if self.reasons.len() < 6 {
            self.reasons.push(reason.into());
        }
    }

    fn reason_string(&self) -> String {
        if self.reasons.is_empty() {
            String::new()
        } else {
            self.reasons.join("; ")
        }
    }
}

async fn add_signal_boosts(
    store: &SurrealStore,
    repo_id: &str,
    signals: &SignalBundle,
    scores: &mut HashMap<String, Accum>,
    cfg: &StrategyConfig,
) -> Result<()> {
    let span_boost = if cfg.signal_span_boost > 0.0 {
        cfg.signal_span_boost
    } else {
        cfg.signal_file_line_boost
    };

    for span in &signals.spans {
        let ids = fragments_covering_line(store, repo_id, &span.path, span.line, 2).await?;
        for fid in ids {
            scores.entry(fid).or_insert_with(Accum::new).add(
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

    for path in path_hints.into_iter().take(cfg.signal_max_paths.max(1)) {
        let ids = fragments_for_path(store, repo_id, &path, 4).await?;
        for fid in ids {
            scores
                .entry(fid)
                .or_insert_with(Accum::new)
                .add(cfg.signal_file_line_boost, format!("signal:path:{path}"));
        }
    }

    Ok(())
}

async fn expand_edges(
    store: &SurrealStore,
    repo_id: &str,
    scores: &mut HashMap<String, Accum>,
    cfg: &StrategyConfig,
) -> Result<()> {
    if cfg.edge_radius == 0 {
        return Ok(());
    }
    let mut seed: Vec<(String, f32)> = scores.iter().map(|(id, a)| (id.clone(), a.score)).collect();
    seed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    seed.truncate(cfg.graph_seed_k.max(1));

    seed.retain(|(_, score)| *score >= 0.05);
    if seed.is_empty() {
        return Ok(());
    }

    let seed_ids: Vec<String> = seed.iter().map(|(id, _)| id.clone()).collect();
    let seed_scores: HashMap<String, f32> = seed
        .iter()
        .map(|(id, score)| (id.clone(), *score))
        .collect();
    let seed_files = fetch_seed_files(store, &seed_ids).await?;

    let max_hops = cfg.edge_radius.max(1) as u32;
    let max_nodes = cfg.edge_max_nodes_per_seed.max(1);
    let etypes: Vec<String> = crate::DEFAULT_REL_ETYPES
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut file_scores: HashMap<String, f32> = HashMap::new();
    for (seed_id, seed_score) in &seed {
        let Some(seed_file) = seed_files.get(seed_id) else {
            continue;
        };
        let mut prev: HashSet<String> = HashSet::new();
        let mut added = 0usize;

        if prev.insert(seed_file.clone()) {
            let score = seed_score * cfg.edge_out_weight;
            file_scores
                .entry(seed_file.clone())
                .and_modify(|s| {
                    if score > *s {
                        *s = score;
                    }
                })
                .or_insert(score);
        }

        for hop in 1..=max_hops {
            if added >= max_nodes {
                break;
            }
            let ids = graph::collect_file_neighborhood(&store.db, repo_id, seed_file, hop).await?;
            let set: HashSet<String> = ids.into_iter().collect();
            let decay = cfg.edge_hop_decay.powi(hop as i32);
            for id in set.difference(&prev) {
                if added >= max_nodes {
                    break;
                }
                let score = seed_score * cfg.edge_out_weight * decay;
                file_scores
                    .entry(id.clone())
                    .and_modify(|s| {
                        if score > *s {
                            *s = score;
                        }
                    })
                    .or_insert(score);
                added += 1;
            }
            prev = set;
        }
    }

    if !file_scores.is_empty() {
        let file_ids: Vec<String> = file_scores.keys().cloned().collect();
        let key_frags = fetch_key_frags(store, repo_id, &file_ids, max_nodes).await?;
        for (frag_id, file_id) in key_frags {
            if let Some(score) = file_scores.get(&file_id) {
                scores
                    .entry(frag_id)
                    .or_insert_with(Accum::new)
                    .add(*score, format!("file:{file_id}"));
            }
        }
    }

    for (seed_id, seed_score) in &seed {
        let mut prev: HashSet<String> = HashSet::new();
        let mut added = 0usize;
        for hop in 1..=max_hops {
            if added >= max_nodes {
                break;
            }
            let ids =
                graph::collect_frag_neighborhood(&store.db, repo_id, seed_id, hop, &etypes).await?;
            let set: HashSet<String> = ids.into_iter().collect();
            let decay = cfg.edge_hop_decay.powi(hop as i32);
            for id in set.difference(&prev) {
                if id == seed_id || added >= max_nodes {
                    continue;
                }
                let delta = seed_score * cfg.edge_out_weight * decay;
                scores
                    .entry(id.clone())
                    .or_insert_with(Accum::new)
                    .add(delta, format!("rel:{seed_id}"));
                added += 1;
            }
            prev = set;
        }
    }

    if seed_ids.len() > 1 {
        let hub = seed_ids[0].clone();
        for seed_id in seed_ids.iter().skip(1) {
            let Some(seed_score) = seed_scores.get(seed_id) else {
                continue;
            };
            let path =
                graph::shortest_path_frags(&store.db, repo_id, seed_id, &hub, &etypes, max_hops)
                    .await?;
            for (idx, id) in path.into_iter().enumerate() {
                if id == *seed_id {
                    continue;
                }
                let decay = cfg.edge_hop_decay.powi(idx as i32);
                let delta = seed_score * cfg.edge_out_weight * decay;
                scores
                    .entry(id)
                    .or_insert_with(Accum::new)
                    .add(delta, format!("path:{seed_id}"));
            }
        }
    }
    Ok(())
}

async fn fetch_seed_files(
    store: &SurrealStore,
    seed_ids: &[String],
) -> Result<HashMap<String, String>> {
    if seed_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<Thing> = seed_ids
        .iter()
        .map(|id| Thing::from(("frag", id.as_str())))
        .collect();
    let sql = "SELECT id, file_id FROM $ids";
    let mut res = store.db.query(sql).bind(("ids", ids)).await?;
    #[derive(serde::Deserialize)]
    struct Row {
        id: Thing,
        file_id: String,
    }
    let rows: Vec<Row> = res.take(0)?;
    let mut out = HashMap::new();
    for row in rows {
        let frag_id = row
            .id
            .to_string()
            .trim_start_matches("frag:")
            .trim_matches('"')
            .to_string();
        out.insert(frag_id, row.file_id);
    }
    Ok(out)
}

async fn fetch_key_frags(
    store: &SurrealStore,
    repo_id: &str,
    file_ids: &[String],
    max_per_file: usize,
) -> Result<Vec<(String, String)>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = "SELECT id, file_id, kind, signature FROM frag WHERE repo_id = $repo_id AND file_id IN $file_ids";
    let mut res = store
        .db
        .query(sql)
        .bind(("repo_id", repo_id.to_string()))
        .bind(("file_ids", file_ids.to_vec()))
        .await?;
    #[derive(serde::Deserialize)]
    struct Row {
        id: Thing,
        file_id: String,
        kind: FragKind,
        signature: String,
    }
    let rows: Vec<Row> = res.take(0)?;
    let mut out: Vec<(String, String)> = Vec::new();
    let mut per_file: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let is_key =
            matches!(row.kind, FragKind::ApiSummary) || is_public_signature(&row.signature);
        if !is_key {
            continue;
        }
        let count = per_file.entry(row.file_id.clone()).or_insert(0);
        if *count >= max_per_file {
            continue;
        }
        *count += 1;
        let frag_id = row
            .id
            .to_string()
            .trim_start_matches("frag:")
            .trim_matches('"')
            .to_string();
        out.push((frag_id, row.file_id));
    }
    Ok(out)
}

fn is_public_signature(signature: &str) -> bool {
    let sig = signature.to_ascii_lowercase();
    sig.contains("export ")
        || sig.contains("public ")
        || sig.contains("open ")
        || sig.contains("pub ")
        || sig.contains("pub(")
        || sig.contains("pub(crate")
        || sig.contains("pub(super")
        || sig.contains("pub(self")
}

async fn fragments_covering_line(
    store: &SurrealStore,
    repo_id: &str,
    path: &str,
    line: u32,
    k: usize,
) -> Result<Vec<String>> {
    let sql = "SELECT id FROM frag WHERE repo_id = $repo_id AND path = $path AND start_line <= $line AND end_line >= $line LIMIT $k";
    let mut res = store
        .db
        .query(sql)
        .bind(("repo_id", repo_id.to_string()))
        .bind(("path", path.to_string()))
        .bind(("line", line as i64))
        .bind(("k", k as i64))
        .await?;
    #[derive(serde::Deserialize)]
    struct Row {
        id: Thing,
    }
    let rows: Vec<Row> = res.take(0)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(
            row.id
                .to_string()
                .trim_start_matches("frag:")
                .trim_matches('"')
                .to_string(),
        );
    }
    Ok(out)
}

async fn fragments_for_path(
    store: &SurrealStore,
    repo_id: &str,
    path: &str,
    k: usize,
) -> Result<Vec<String>> {
    let sql = "SELECT id FROM frag WHERE repo_id = $repo_id AND path = $path LIMIT $k";
    let mut res = store
        .db
        .query(sql)
        .bind(("repo_id", repo_id.to_string()))
        .bind(("path", path.to_string()))
        .bind(("k", k as i64))
        .await?;
    #[derive(serde::Deserialize)]
    struct Row {
        id: Thing,
    }
    let rows: Vec<Row> = res.take(0)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(
            row.id
                .to_string()
                .trim_start_matches("frag:")
                .trim_matches('"')
                .to_string(),
        );
    }
    Ok(out)
}

async fn attach_candidate_neighbors(
    store: &SurrealStore,
    repo_id: &str,
    strategy: &StrategyConfig,
    cands: &mut [Candidate],
) -> Result<()> {
    if cands.is_empty() {
        return Ok(());
    }

    let mut idx_by_id: HashMap<String, usize> = HashMap::new();
    for (idx, cand) in cands.iter().enumerate() {
        idx_by_id.insert(cand.id.clone(), idx);
    }

    let max_edges = strategy.edge_max_edges_per_node.max(1) as i64;
    for idx in 0..cands.len() {
        let node_thing = Thing::from(("frag", cands[idx].id.as_str()));
        let sql = "SELECT in, out, weight FROM rel WHERE repo_id = $repo_id AND (in = $node OR out = $node) LIMIT $k";
        let mut res = store
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("node", node_thing.clone()))
            .bind(("k", max_edges))
            .await?;
        #[derive(serde::Deserialize)]
        struct EdgeRow {
            #[serde(rename = "in")]
            in_id: Thing,
            #[serde(rename = "out")]
            out_id: Thing,
            weight: Option<f64>,
        }
        let rows: Vec<EdgeRow> = res.take(0)?;
        let mut neighbor_map: HashMap<String, f32> = HashMap::new();
        for row in rows {
            let weight = row.weight.unwrap_or(1.0) as f32;
            let neighbor = if row.in_id == node_thing {
                row.out_id
            } else {
                row.in_id
            };
            let neighbor = neighbor.to_string();
            let neighbor = neighbor.trim_start_matches("frag:").trim_matches('"');
            if let Some(_) = idx_by_id.get(neighbor) {
                let entry = neighbor_map.entry(neighbor.to_string()).or_insert(weight);
                if weight > *entry {
                    *entry = weight;
                }
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
        cands[idx].neighbors = neighbors;
    }

    Ok(())
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

    let allow_signals = mode.contains("signals");
    let allow_symbols = mode.contains("symbols");
    let allow_query = mode.contains("query_grep");

    let mut targets: Vec<u32> = Vec::new();
    if allow_signals {
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

    if allow_query {
        if let Some(s) = snippet::slice_by_grep(
            &frag.body,
            frag.span.start_line,
            task_tokens,
            ctx,
            max_lines,
        ) {
            let mut show: Vec<String> = task_tokens.iter().take(8).cloned().collect();
            show.retain(|t| !t.is_empty());
            let reason = if show.is_empty() {
                "query".to_string()
            } else {
                format!("query:{}", show.join(","))
            };
            return Some((reason, s));
        }
    }

    None
}
