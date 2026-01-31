use crate::model::{
    ContextPack, DeferredItem, FragKind, FragmentView, PackItem, PackMetrics, SignalBundle, Span,
    StrategyConfig, UnresolvedSymbol,
};
use crate::tokenizer::TokenCounter;
use crate::util::{extract_ident_tokens, hash_text_hex, jaccard_sorted};
use std::collections::{HashMap, HashSet, VecDeque};

/// Signature-first packer with:
/// - optional token budget enforcement (tiktoken tokenizer w/ heuristic fallback)
/// - MMR-style diversification (cheap lexical proxy)
/// - per-file caps
/// - body "upgrades" (replace signature with body to avoid duplication)
pub fn pack_with_strategy(
    strategy: &StrategyConfig,
    mut candidates: Vec<Candidate>,
) -> ContextPack {
    // Sort descending relevance score.
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let budget_chars = strategy.budget_chars;
    let budget_tokens = strategy.budget_tokens;
    let max_bodies = strategy.max_bodies;
    let per_file_cap_sigs = strategy.per_file_cap_signatures.max(1);
    let per_file_cap_bodies = strategy.per_file_cap_bodies.max(1);

    // A small, fixed overhead per item to account for separators/markdown/etc.
    // (We already include a header in decorate_signature/body, but this keeps us conservative.)
    let overhead_chars: usize = 80;

    let pack_id = hash_text_hex(&format!(
        "pack:{}:{:?}:{}:{}:{}:{}:{}",
        budget_chars,
        budget_tokens,
        max_bodies,
        candidates.len(),
        strategy.mmr_lambda,
        strategy.mmr_top_n,
        per_file_cap_sigs
    ));

    let mut used_chars = 0usize;
    let mut used_tokens = 0usize;

    // Token counter used for budgeting/reporting.
    // Defaults to a tiktoken encoding (strategy.tokenizer) and falls back to a
    // conservative heuristic if the tokenizer can't be resolved.
    let token_counter = TokenCounter::new(&strategy.tokenizer);

    // Estimate per-item token overhead for headings/separators (not included in
    // candidate.signature/body strings).
    let overhead_tokens: usize = {
        let sample = "### 00000000 (Signature, score=0.000, reason=mmr-selected; ... )\n\n";
        token_counter.count(sample).max(1)
    };

    let mut items: Vec<PackItem> = Vec::new();
    let mut deferred: Vec<DeferredItem> = Vec::new();

    // Track how many signature slots we used per file.
    let mut file_sig_counts: HashMap<String, usize> = HashMap::new();
    // Track how many body upgrades we used per file.
    let mut file_body_counts: HashMap<String, usize> = HashMap::new();

    // id -> index in items (so we can upgrade signature -> body without duplicating).
    let mut item_index_by_id: HashMap<String, usize> = HashMap::new();

    // Helper: check budget with either chars or tokens.
    let fits_budget =
        |used_chars: usize, used_tokens: usize, add_chars: isize, add_tokens: isize| -> bool {
            let next_chars = if add_chars.is_negative() {
                used_chars.saturating_sub(add_chars.unsigned_abs() as usize)
            } else {
                used_chars.saturating_add(add_chars as usize)
            };

            let next_tokens = if add_tokens.is_negative() {
                used_tokens.saturating_sub(add_tokens.unsigned_abs() as usize)
            } else {
                used_tokens.saturating_add(add_tokens as usize)
            };

            if let Some(bt) = budget_tokens {
                next_tokens <= bt
            } else {
                next_chars <= budget_chars
            }
        };

    // Helper: compute (chars, tokens) "cost" for a piece of text.
    let cost = |text: &str| -> (usize, usize) {
        let c = text.len() + overhead_chars;
        let t = token_counter.count(text) + overhead_tokens;
        (c, t)
    };

    // ----------------------------
    // Phase 1: signature selection
    // ----------------------------
    let use_subgraph = strategy.subgraph_enabled && !candidates.is_empty();

    if use_subgraph {
        let mut id_to_idx: HashMap<String, usize> = HashMap::new();
        for (i, cand) in candidates.iter().enumerate() {
            id_to_idx.insert(cand.id.clone(), i);
        }

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];
        for (i, cand) in candidates.iter().enumerate() {
            for nb in &cand.neighbors {
                if let Some(&j) = id_to_idx.get(&nb.id) {
                    adj[i].push(j);
                    adj[j].push(i);
                }
            }
        }

        let mut seed_idxs: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.reason.contains("signal:"))
            .map(|(i, _)| i)
            .collect();
        seed_idxs.sort_by(|a, b| {
            candidates[*b]
                .score
                .partial_cmp(&candidates[*a].score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if seed_idxs.is_empty() {
            seed_idxs = (0..candidates.len()).collect();
            seed_idxs.sort_by(|a, b| {
                candidates[*b]
                    .score
                    .partial_cmp(&candidates[*a].score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let mut selected: HashSet<usize> = HashSet::new();
        let mut add_signature = |idx: usize, tag: &str| -> bool {
            let cand = &candidates[idx];
            if item_index_by_id.contains_key(&cand.id) {
                return false;
            }
            let count = *file_sig_counts.get(&cand.path).unwrap_or(&0);
            if count >= per_file_cap_sigs {
                return false;
            }
            let (c_chars, c_tokens) = cost(&cand.signature);
            if !fits_budget(used_chars, used_tokens, c_chars as isize, c_tokens as isize) {
                return false;
            }
            used_chars += c_chars;
            used_tokens += c_tokens;
            *file_sig_counts.entry(cand.path.clone()).or_insert(0) += 1;

            let idx_item = items.len();
            items.push(PackItem {
                id: cand.id.clone(),
                view: FragmentView::Signature,
                path: cand.path.clone(),
                kind: cand.kind,
                symbol: cand.symbol.clone(),
                span: cand.span,
                score: cand.score,
                reason: format!("{}; {}", tag, cand.reason),
                content: cand.signature.clone(),
            });
            item_index_by_id.insert(cand.id.clone(), idx_item);
            true
        };

        for &seed in &seed_idxs {
            if add_signature(seed, "subgraph-seed") {
                selected.insert(seed);
                break;
            }
        }

        if !selected.is_empty() {
            let hop_cap = strategy.max_hops;
            for &seed in seed_idxs.iter().skip(1) {
                if selected.contains(&seed) {
                    continue;
                }
                if let Some(path) = shortest_path(&adj, &selected, seed, hop_cap) {
                    for idx in path {
                        if selected.contains(&idx) {
                            continue;
                        }
                        if add_signature(idx, "subgraph-path") {
                            selected.insert(idx);
                        } else {
                            break;
                        }
                    }
                }
            }

            loop {
                let dist = bfs_distances(&adj, &selected, hop_cap);
                let mut ranked: Vec<(usize, f32)> = Vec::new();
                for (i, cand) in candidates.iter().enumerate() {
                    if selected.contains(&i) {
                        continue;
                    }
                    let Some(d) = dist[i] else {
                        continue;
                    };
                    if d == 0 {
                        continue;
                    }
                    let (c_chars, c_tokens) = cost(&cand.signature);
                    let denom = if budget_tokens.is_some() {
                        c_tokens.max(1) as f32
                    } else {
                        c_chars.max(1) as f32
                    };
                    let benefit = cand.score / (1.0 + strategy.connectivity_penalty * d as f32);
                    ranked.push((i, benefit / denom));
                }

                ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let mut added_any = false;
                let beam = strategy.beam_width.max(1).min(ranked.len());
                for (idx, _) in ranked.into_iter().take(beam) {
                    if add_signature(idx, "subgraph-expand") {
                        selected.insert(idx);
                        added_any = true;
                        break;
                    }
                }
                if !added_any {
                    break;
                }
            }
        }
    }

    if !use_subgraph {
        let mmr_lambda = strategy.mmr_lambda.clamp(0.0, 1.0);
        let use_mmr = mmr_lambda < 0.999;
        let mmr_top_n = strategy.mmr_top_n.max(1).min(candidates.len());

        // MMR only operates over a limited pool for speed.
        // Precompute token sets for those candidates.
        let mut token_sets: Vec<Vec<String>> = Vec::new();
        token_sets.reserve(mmr_top_n);
        for i in 0..mmr_top_n {
            token_sets.push(extract_ident_tokens(&candidates[i].signature));
        }

        let mut selected_in_mmr: HashSet<usize> = HashSet::new();
        let mut selected_token_sets: Vec<Vec<String>> = Vec::new();

        if use_mmr && mmr_top_n > 0 {
            let mut remaining: Vec<usize> = (0..mmr_top_n).collect();

            loop {
                let mut best: Option<(usize, f32)> = None;

                for &idx in &remaining {
                    let cand = &candidates[idx];

                    // Per-file cap
                    let count = *file_sig_counts.get(&cand.path).unwrap_or(&0);
                    if count >= per_file_cap_sigs {
                        continue;
                    }

                    // Budget check (signature)
                    let (c_chars, c_tokens) = cost(&cand.signature);
                    if !fits_budget(used_chars, used_tokens, c_chars as isize, c_tokens as isize) {
                        continue;
                    }

                    // Diversity penalty: max similarity to already selected
                    let sim = if selected_token_sets.is_empty() {
                        0.0
                    } else {
                        let ts = &token_sets[idx];
                        selected_token_sets
                            .iter()
                            .map(|s| jaccard_sorted(ts, s))
                            .fold(0.0, f32::max)
                    };

                    let mmr_score = mmr_lambda * cand.score - (1.0 - mmr_lambda) * sim;
                    if best.map(|(_, s)| mmr_score > s).unwrap_or(true) {
                        best = Some((idx, mmr_score));
                    }
                }

                let Some((best_idx, _)) = best else {
                    break;
                };

                // Select best_idx
                selected_in_mmr.insert(best_idx);
                selected_token_sets.push(token_sets[best_idx].clone());

                // Remove from remaining
                remaining.retain(|&i| i != best_idx);

                // Add signature item
                let cand = &candidates[best_idx];
                let (c_chars, c_tokens) = cost(&cand.signature);
                used_chars += c_chars;
                used_tokens += c_tokens;

                *file_sig_counts.entry(cand.path.clone()).or_insert(0) += 1;

                let idx_item = items.len();
                items.push(PackItem {
                    id: cand.id.clone(),
                    view: FragmentView::Signature,
                    path: cand.path.clone(),
                    kind: cand.kind,
                    symbol: cand.symbol.clone(),
                    span: cand.span,
                    score: cand.score,
                    reason: format!("mmr-selected; {}", cand.reason),
                    content: cand.signature.clone(),
                });
                item_index_by_id.insert(cand.id.clone(), idx_item);

                // Early stop if we can't fit any more signatures
                // (We don't know the next minimal cost; the loop condition handles it.)
                if remaining.is_empty() {
                    break;
                }
            }
        }

        // Fill remaining signatures by relevance order (still enforcing per-file caps & budget)
        for cand in &candidates {
            if item_index_by_id.contains_key(&cand.id) {
                continue;
            }

            let count = *file_sig_counts.get(&cand.path).unwrap_or(&0);
            if count >= per_file_cap_sigs {
                // We'll defer later.
                continue;
            }

            let (c_chars, c_tokens) = cost(&cand.signature);
            if !fits_budget(used_chars, used_tokens, c_chars as isize, c_tokens as isize) {
                continue;
            }

            used_chars += c_chars;
            used_tokens += c_tokens;
            *file_sig_counts.entry(cand.path.clone()).or_insert(0) += 1;

            let idx_item = items.len();
            let reason = if use_mmr {
                // If MMR was enabled and this wasn't picked in the MMR pool, label as fill.
                format!("fill; {}", cand.reason)
            } else {
                cand.reason.clone()
            };

            items.push(PackItem {
                id: cand.id.clone(),
                view: FragmentView::Signature,
                path: cand.path.clone(),
                kind: cand.kind,
                symbol: cand.symbol.clone(),
                span: cand.span,
                score: cand.score,
                reason,
                content: cand.signature.clone(),
            });
            item_index_by_id.insert(cand.id.clone(), idx_item);
        }
    }

    // ---------------------------------
    // Phase 2: body upgrades (top-ranked)
    // ---------------------------------
    if max_bodies > 0 {
        let mut bodies_added = 0usize;
        for cand in &candidates {
            if bodies_added >= max_bodies {
                break;
            }

            // ApiSummary fragments already contain a compact, file-level overview.
            // "Upgrading" them to a body is usually pointless (body == signature)
            // and wastes a limited body slot.
            if cand.kind == FragKind::ApiSummary {
                continue;
            }
            let Some(&idx_item) = item_index_by_id.get(&cand.id) else {
                continue;
            };

            let body_count = *file_body_counts.get(&cand.path).unwrap_or(&0);
            if body_count >= per_file_cap_bodies {
                continue;
            }

            // Compute delta cost for signature -> body replacement.
            let (sig_chars, sig_tokens) = cost(&cand.signature);
            let (body_chars, body_tokens) = cost(&cand.body);
            let delta_chars = body_chars as isize - sig_chars as isize;
            let delta_tokens = body_tokens as isize - sig_tokens as isize;

            if !fits_budget(used_chars, used_tokens, delta_chars, delta_tokens) {
                continue;
            }

            // Apply upgrade
            used_chars = if delta_chars.is_negative() {
                used_chars.saturating_sub(delta_chars.unsigned_abs() as usize)
            } else {
                used_chars.saturating_add(delta_chars as usize)
            };
            used_tokens = if delta_tokens.is_negative() {
                used_tokens.saturating_sub(delta_tokens.unsigned_abs() as usize)
            } else {
                used_tokens.saturating_add(delta_tokens as usize)
            };

            bodies_added += 1;
            *file_body_counts.entry(cand.path.clone()).or_insert(0) += 1;

            let item = &mut items[idx_item];
            item.view = cand.body_view.clone();
            item.content = cand.body.clone();
            let tag = match cand.body_view {
                FragmentView::Body => "body-upgrade",
                FragmentView::Slice => "slice-upgrade",
                _ => "body-upgrade",
            };
            item.reason = format!("{tag}; {}", item.reason);
        }
    }

    // ---------------------------------
    // Phase 2b: support closure (optional)
    // ---------------------------------
    let mut covers_symbols: Vec<String> = Vec::new();
    let mut unresolved_symbols: Vec<UnresolvedSymbol> = Vec::new();
    let mut support_defs_added = 0usize;

    if strategy.support_enabled {
        let mut covered_tokens: HashSet<String> = HashSet::new();
        for it in &items {
            if let Some(sym) = &it.symbol {
                covered_tokens.insert(sym.clone());
                let tail = symbol_tail(sym);
                if tail != sym {
                    covered_tokens.insert(tail.to_string());
                }
            }
        }

        let mut token_counts: HashMap<String, usize> = HashMap::new();
        for it in &items {
            for t in extract_ident_tokens(&it.content) {
                if is_stop_token(&t) {
                    continue;
                }
                *token_counts.entry(t).or_insert(0) += 1;
            }
        }

        let mut tokens: Vec<(String, usize)> = token_counts.into_iter().collect();
        tokens.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));
        let token_limit = strategy.support_max_defs.max(1).saturating_mul(3);

        for (tok, _) in tokens.into_iter().take(token_limit) {
            if covered_tokens.contains(&tok) {
                continue;
            }

            let mut best: Option<&Candidate> = None;
            let mut suggestions: Vec<DeferredItem> = Vec::new();

            for cand in &candidates {
                let Some(sym) = cand.symbol.as_ref() else {
                    continue;
                };
                if !symbol_matches_token(sym, &tok) {
                    continue;
                }
                if cand.score < strategy.support_min_confidence {
                    continue;
                }
                if suggestions.len() < 3 {
                    suggestions.push(DeferredItem {
                        id: cand.id.clone(),
                        path: cand.path.clone(),
                        kind: cand.kind,
                        symbol: cand.symbol.clone(),
                        span: cand.span,
                        reason: "support-candidate".to_string(),
                    });
                }
                if item_index_by_id.contains_key(&cand.id) {
                    continue;
                }
                if best.map(|b| cand.score > b.score).unwrap_or(true) {
                    best = Some(cand);
                }
            }

            if support_defs_added >= strategy.support_max_defs {
                unresolved_symbols.push(UnresolvedSymbol {
                    symbol: tok,
                    candidates: suggestions,
                    reason: Some("support-cap".to_string()),
                });
                continue;
            }

            let Some(cand) = best else {
                unresolved_symbols.push(UnresolvedSymbol {
                    symbol: tok,
                    candidates: suggestions,
                    reason: Some("no-candidate".to_string()),
                });
                continue;
            };

            let (content, view, add_sig, add_body): (String, FragmentView, bool, bool) =
                if strategy.support_signature_only {
                    (cand.signature.clone(), FragmentView::Signature, true, false)
                } else {
                    let v = cand.body_view.clone();
                    let is_sig = matches!(v, FragmentView::Signature);
                    (cand.body.clone(), v, is_sig, !is_sig)
                };

            if add_sig {
                let count = *file_sig_counts.get(&cand.path).unwrap_or(&0);
                if count >= per_file_cap_sigs {
                    unresolved_symbols.push(UnresolvedSymbol {
                        symbol: tok,
                        candidates: suggestions,
                        reason: Some("per-file-cap".to_string()),
                    });
                    continue;
                }
            } else {
                let count = *file_body_counts.get(&cand.path).unwrap_or(&0);
                if count >= per_file_cap_bodies {
                    unresolved_symbols.push(UnresolvedSymbol {
                        symbol: tok,
                        candidates: suggestions,
                        reason: Some("per-file-cap".to_string()),
                    });
                    continue;
                }
            }

            let (c_chars, c_tokens) = cost(&content);
            if !fits_budget(used_chars, used_tokens, c_chars as isize, c_tokens as isize) {
                unresolved_symbols.push(UnresolvedSymbol {
                    symbol: tok,
                    candidates: suggestions,
                    reason: Some("budget".to_string()),
                });
                continue;
            }

            used_chars += c_chars;
            used_tokens += c_tokens;
            if add_sig {
                *file_sig_counts.entry(cand.path.clone()).or_insert(0) += 1;
            }
            if add_body {
                *file_body_counts.entry(cand.path.clone()).or_insert(0) += 1;
            }

            let idx_item = items.len();
            items.push(PackItem {
                id: cand.id.clone(),
                view,
                path: cand.path.clone(),
                kind: cand.kind,
                symbol: cand.symbol.clone(),
                span: cand.span,
                score: cand.score,
                reason: format!("support:{}; {}", tok, cand.reason),
                content,
            });
            item_index_by_id.insert(cand.id.clone(), idx_item);
            support_defs_added += 1;
            covered_tokens.insert(tok);
        }
    }

    let mut cover_set: HashSet<String> = HashSet::new();
    for it in &items {
        if let Some(sym) = &it.symbol {
            cover_set.insert(sym.clone());
            let tail = symbol_tail(sym);
            if tail != sym {
                cover_set.insert(tail.to_string());
            }
        }
    }
    covers_symbols.extend(cover_set.into_iter());
    covers_symbols.sort();

    // -------------------------
    // Phase 3: deferred listing
    // -------------------------
    for cand in &candidates {
        if item_index_by_id.contains_key(&cand.id) {
            continue;
        }
        let count = *file_sig_counts.get(&cand.path).unwrap_or(&0);
        let reason = if count >= per_file_cap_sigs {
            format!("deferred (per-file cap); {}", cand.reason)
        } else {
            format!("deferred (budget); {}", cand.reason)
        };
        deferred.push(DeferredItem {
            id: cand.id.clone(),
            path: cand.path.clone(),
            kind: cand.kind,
            symbol: cand.symbol.clone(),
            span: cand.span,
            reason,
        });
    }

    let mut notes: Vec<String> = Vec::new();
    if token_counter.is_fallback() {
        notes.push(format!(
            "tokenizer fallback: could not resolve '{}' (token counts are approximate)",
            token_counter.spec()
        ));
    }

    let mut metrics = PackMetrics::default();
    metrics.pack_tokens_total = used_tokens;
    metrics.unbound_symbol_count = unresolved_symbols.len();
    metrics.support_defs_added = support_defs_added;
    metrics.connectivity_score = connectivity_score(&items, &candidates);

    ContextPack {
        pack_id,
        budget_chars,
        used_chars,
        budget_tokens,
        used_tokens,
        items,
        deferred,
        notes,
        signals: SignalBundle::default(),
        signals_used: Vec::new(),
        covers_symbols,
        unresolved_symbols,
        metrics,
        recipe_excerpt: None,
        external_docs: Vec::new(),
    }
}

// (We no longer expose approx token helpers here; token counting is centralized
// in `tokenizer::TokenCounter` with heuristic fallback.)

/// Neighbor edge used for connected-subgraph selection.
#[derive(Debug, Clone)]
pub struct CandidateNeighbor {
    pub id: String,
    pub weight: f32,
}

/// Candidate fragment used by the packer.
/// Retrieval layer should fill these fields (signature/body already loaded).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub rowid: i64,
    pub path: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    pub span: Span,
    pub score: f32,
    pub reason: String,
    pub signature: String,
    /// Content used when this candidate is selected for an “upgrade”.
    ///
    /// Most commonly this is the full body, but for context compaction it may
    /// be a slice/excerpt.
    pub body: String,

    /// Neighbor edges for connected-subgraph selection.
    pub neighbors: Vec<CandidateNeighbor>,

    /// The view corresponding to `body`.
    ///
    /// - `Body`  => full body
    /// - `Slice` => compact excerpt
    pub body_view: FragmentView,
}

fn bfs_distances(
    adj: &[Vec<usize>],
    sources: &HashSet<usize>,
    max_hops: usize,
) -> Vec<Option<usize>> {
    let mut dist: Vec<Option<usize>> = vec![None; adj.len()];
    let mut q: VecDeque<usize> = VecDeque::new();

    for &s in sources {
        if s < adj.len() {
            dist[s] = Some(0);
            q.push_back(s);
        }
    }

    while let Some(node) = q.pop_front() {
        let d = dist[node].unwrap_or(0);
        if d >= max_hops {
            continue;
        }
        let nd = d + 1;
        for &nb in &adj[node] {
            if dist[nb].is_none() {
                dist[nb] = Some(nd);
                q.push_back(nb);
            }
        }
    }

    dist
}

fn shortest_path(
    adj: &[Vec<usize>],
    sources: &HashSet<usize>,
    target: usize,
    max_hops: usize,
) -> Option<Vec<usize>> {
    if sources.is_empty() || target >= adj.len() {
        return None;
    }

    let mut parent: Vec<Option<usize>> = vec![None; adj.len()];
    let mut dist: Vec<Option<usize>> = vec![None; adj.len()];
    let mut q: VecDeque<usize> = VecDeque::new();

    for &s in sources {
        if s < adj.len() {
            dist[s] = Some(0);
            q.push_back(s);
        }
    }

    while let Some(node) = q.pop_front() {
        let d = dist[node].unwrap_or(0);
        if d >= max_hops {
            continue;
        }
        let nd = d + 1;
        for &nb in &adj[node] {
            if dist[nb].is_none() {
                dist[nb] = Some(nd);
                parent[nb] = Some(node);
                if nb == target {
                    let mut path = vec![nb];
                    let mut cur = nb;
                    while let Some(p) = parent[cur] {
                        path.push(p);
                        cur = p;
                        if sources.contains(&cur) {
                            break;
                        }
                    }
                    path.reverse();
                    return Some(path);
                }
                q.push_back(nb);
            }
        }
    }

    None
}

fn connectivity_score(items: &[PackItem], candidates: &[Candidate]) -> Option<f32> {
    if items.is_empty() {
        return None;
    }
    if items.len() == 1 {
        return Some(1.0);
    }

    let mut cand_map: HashMap<&str, &Candidate> = HashMap::new();
    for cand in candidates {
        cand_map.insert(cand.id.as_str(), cand);
    }

    let item_ids: Vec<&str> = items.iter().map(|it| it.id.as_str()).collect();
    let item_set: HashSet<&str> = item_ids.iter().copied().collect();

    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for &id in &item_ids {
        if let Some(cand) = cand_map.get(id) {
            let mut neighbors: Vec<&str> = Vec::new();
            for nb in &cand.neighbors {
                let nb_id = nb.id.as_str();
                if item_set.contains(nb_id) {
                    neighbors.push(nb_id);
                }
            }
            adj.insert(id, neighbors);
        }
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut q: VecDeque<&str> = VecDeque::new();
    let start = item_ids[0];
    visited.insert(start);
    q.push_back(start);

    while let Some(cur) = q.pop_front() {
        if let Some(neis) = adj.get(cur) {
            for &nb in neis {
                if visited.insert(nb) {
                    q.push_back(nb);
                }
            }
        }
    }

    Some(visited.len() as f32 / items.len() as f32)
}

fn symbol_tail(sym: &str) -> &str {
    if let Some((_, tail)) = sym.rsplit_once("::") {
        return tail;
    }
    if let Some((_, tail)) = sym.rsplit_once('.') {
        return tail;
    }
    sym
}

fn symbol_matches_token(sym: &str, tok: &str) -> bool {
    let tail = symbol_tail(sym).to_ascii_lowercase();
    let full = sym.to_ascii_lowercase();
    tail == tok || full == tok
}

fn is_stop_token(tok: &str) -> bool {
    matches!(
        tok,
        "fn" | "let"
            | "pub"
            | "use"
            | "mod"
            | "struct"
            | "enum"
            | "trait"
            | "impl"
            | "type"
            | "where"
            | "self"
            | "super"
            | "crate"
            | "return"
            | "if"
            | "else"
            | "match"
            | "for"
            | "while"
            | "loop"
            | "async"
            | "await"
            | "class"
            | "func"
            | "var"
            | "val"
            | "const"
            | "interface"
            | "protocol"
            | "extension"
            | "import"
            | "from"
            | "in"
            | "new"
            | "switch"
            | "case"
            | "break"
            | "continue"
            | "default"
            | "true"
            | "false"
            | "nil"
            | "null"
            | "this"
    )
}
