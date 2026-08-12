# Context Engine (Rust, embedded)

A **language-aware Context Engine** for LLM coding agents.

This repository implements the *external* “context engineering” layer described in our design:
- **AST-aware fragmentation** (Rust first)
- **content-addressed fragment IDs** (Unison-inspired)
- **hybrid retrieval** (SQLite FTS5 + in-process HNSW vector search)
- **signals-first seeding** (file:line hints from compiler output)
- **explicit edges + subgraph expansion**
  - resolved **ref→def** edges (multi-hop BFS)
  - **Rust module graph** edges between file-level summaries (workspace-aware via Cargo metadata when available): `mod` / `use` (+ reverse `imported_by` / `modded_by` for explainability)
  - same-file neighbors
- **budgeted context packing** (signatures-first; *MMR-diversified*; bodies as *upgrades* to avoid duplication)
- **MCP server** so tools like **Codex CLI** can call `context.pack()` at runtime.

**New in v20**

- **Definition-time symbol qualification (Rust, no rust-analyzer required)**: when indexing Rust sources under `src/`, definitions are stored not only as `Bar` but also as module-qualified aliases like `foo::Bar` and `crate::foo::Bar` (best-effort path-based module prefixing). This is also *backfilled* during edge rebuilds so older indexes can be upgraded without a full reindex.
- **Cross-crate qualification (workspace-aware)**: for lib/proc-macro targets discovered via Cargo metadata, we also store `crate_name::foo::Bar` aliases, improving resolution for refs like `other_crate::foo::Bar`.

*(v19 improvements are still included: scoped ref tail-aliasing + module-path-biased resolution.)*


> Status: MVP skeleton focused on Rust repositories. The code is intentionally modular so you can add other languages later.

---

## Why this exists

Large repos don’t fit in an LLM context window. Blindly pasting code:
- wastes tokens,
- adds noise,
- increases latency and cost.

Instead, we treat a repo like a searchable, structured database:
1) Index it into **fragments** (functions/types/impls/modules)
2) Compute a **stable fragment ID** from content (Unison-ish content addressing)
3) Build **lexical** + **semantic** search indexes
4) At query time, produce a minimal **Context Pack** within a budget.

---

## Repo layout

```
context-engine/
  crates/
    ce-core/        Core types (Fragment, Pack) + hashing helpers
    ce-lang-rust/   Tree-sitter based Rust parser + fragment extractor
    ce-store/       SQLite schema + DB access + HNSW wrapper + embedding helpers
    ce-cli/         CLI: index/search/pack
    ce-mcp/         MCP stdio server: context.pack / fragment.get / context.search
```

---

## Requirements

- Rust (stable) + Cargo
- SQLite (bundled via rusqlite)
- Tree-sitter Rust grammar (Rust crate dependency)
- `fastembed` downloads embedding model weights on first run (local inference).

> No external services required. Everything runs locally/in-process.

---

## Quick start

### 1) Build

```bash
cargo build --release
```

### 2) Index a Rust repo

```bash
# Example: index the current repo
./target/release/ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw
```

By default indexing is incremental:
- unchanged files (by whole-file content hash) are skipped
- changed files have their old fragments deleted before inserting new ones

Useful flags:

```bash
# Reindex everything
./target/release/ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw --full

# Remove entries for files deleted from disk
./target/release/ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw --prune

# Skip rebuilding resolved edges (faster indexing; weaker multi-hop subgraph expansion)
./target/release/ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw --skip-edges
```

This will:
- scan `*.rs` files (gitignore-aware),
- parse top-level items with tree-sitter,
- store fragments in SQLite,
- embed fragments locally (fastembed),
- build an in-process HNSW index and dump it to `.ce/hnsw`.

Index keys are stored as **repo-relative paths** (normalized with `/`) so your
SQLite index is stable across clones and absolute filesystem locations.

Edge rebuild behavior:
- By default, resolved ref→def edges are rebuilt **incrementally** when only a small number of files changed.
- If you pass `--full` (or touch many files), the CLI falls back to a full edge rebuild for correctness.

### 3) Search

```bash
./target/release/ce search --db .ce/index.sqlite --hnsw-dir .ce/hnsw --query "tokio spawn blocking deadlock" --k 8
```

### 4) Explain the repo graph

```bash
./target/release/ce graph-report --db .ce/index.sqlite --out .ce/GRAPH_REPORT.md
```

This writes a compact Markdown report with indexed fragment/edge counts, edge
type distribution, top connected fragments, strongest cross-file relationships,
and suggested follow-up questions.

### 5) Create a context pack (signatures-first + diversified)

```bash
./target/release/ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw \
  --task "Fix failing test in user auth flow. Error: mismatch in token expiry." \
  --budget-chars 12000 --max-bodies 2

# Token-based budgeting (recommended for real LLM context windows)
./target/release/ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw \
  --task "Fix failing test..." \
  --budget-tokens 3000 --max-bodies 2
```

The output is a single text “pack” suitable to paste into an LLM prompt:
- many signatures (wide coverage)
- a few full bodies (deep detail) as **in-place upgrades** (so you don't pay for the signature twice)
- a deferred list (fragment IDs to fetch on demand)

You can also emit structured output:

```bash
# JSON output (for programmatic consumers)
./target/release/ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw \
  --task "Fix failing test..." --budget-tokens 3000 --max-bodies 2 --format json

# Both (text pack + JSON)
./target/release/ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw \
  --task "Fix failing test..." --budget-tokens 3000 --max-bodies 2 --format both
```

Notes:
- Fragment headers include a **source span** (`span: Lstart-Lend`) to make it easy for an agent to
  fetch adjacent context precisely.
- When the Context Engine emits compact body slices, it uses **indent-aware placeholders**
  and **block-aware collapsing** (`{ ... }`) for very large blocks to reduce tokens while
  preserving structure.
- File:line “signals” are extracted from compiler-style logs and (when present) **unified diffs**
  (e.g. `@@ -12,7 +98,12 @@`).

---

## Strategy configs (DGM “genomes”)

Strategies let you store and reuse different **Context Engine behaviors** (retrieval/expansion/packing parameters)
without recompiling. This is the primary knob you will evolve/optimize in a DGM-style loop.

### Add a strategy

```bash
# Store a partial TOML config (missing fields fall back to defaults)
./target/release/ce strategy add \
  --db .ce/index.sqlite \
  --name "cheap" \
  --config strategies/cheap.toml
```

### List strategies

```bash
./target/release/ce strategy list --db .ce/index.sqlite
```

### Use a stored strategy for packing

```bash
./target/release/ce pack \
  --db .ce/index.sqlite \
  --hnsw-dir .ce/hnsw \
  --strategy-id <strategy_id> \
  --task "Fix failing test..."
```

### Use a strategy file directly (no DB)

```bash
./target/release/ce pack \
  --db .ce/index.sqlite \
  --hnsw-dir .ce/hnsw \
  --strategy-file strategies/high_recall.toml \
  --task "..."
```

---

## Using with Codex CLI via MCP

Codex CLI supports MCP servers configured in `~/.codex/config.toml`.

Add a server entry:

```toml
[mcp_servers.context_engine]
command = "/ABS/PATH/context-engine/target/release/ce-mcp"
args = ["--db", "/ABS/PATH/.ce/index.sqlite", "--hnsw-dir", "/ABS/PATH/.ce/hnsw"]
```

Recommended: use the repo helper script so Codex can start `ce-mcp` even when `.ce` doesn't exist yet.

```toml
[mcp_servers.context_engine]
command = "/ABS/PATH/TO/THIS-REPO/scripts/codex/ce-mcp.sh"
```

If Codex reports `MCP startup failed` or the handshake closes, run the helper directly once to surface the error:

```bash
cd /ABS/PATH/TO/THIS-REPO
./scripts/codex/ce-mcp.sh
```

Then create a Codex **skill** (example) that always calls `context.pack` before editing.
(See `docs/codex-skill-example.md` in this repo.)

The MCP server (`ce-mcp`) currently exposes tools:
- `context.pack`
- `context.search`
- `fragment.get`
- `strategy.list`
- `strategy.get`
- `memory.add`
- `memory.list`

---

## Design notes

### Content-addressed FragmentId

We compute:
- `content_hash = blake3(normalized_fragment_body)`
- `ast_hash = blake3(tree_sitter_node.to_sexp())` (optional structural signature)

`frag_id = content_hash` by default.
This makes fragment IDs stable across file moves (and helps deduplicate identical code).

### Embedded indexes

- **SQLite** stores metadata, text, and embeddings.
- **FTS5** provides fast lexical search.
- **HNSW (hnsw_rs)** provides fast approximate nearest neighbor search over embeddings.

HNSW is dumped to disk after indexing.

Current behavior:
- `ce` CLI **loads** the persisted HNSW dump (fast) and falls back to rebuilding from SQLite embeddings if the dump is missing or stale.
- `ce-mcp` does the same at startup, then reuses the in-process index for all requests.

Staleness checks use both the dump description (dimension/point count) and DB-persisted metadata
stored in the `meta` table (embedding model/dim + cheap state hashes).

### Explicit edges (refs -> defs)

During indexing we also rebuild a lightweight `edges` table:
- an edge from fragment A to fragment B means "A references a symbol defined in B" (type `refers`).

Edges have a heuristic `weight` (not just 1.0) that biases toward likely resolutions
(same file / same directory / kind match) to reduce ambiguity for common names.

At pack time, the budget-aware prize-collecting subgraph solver uses weighted connections to retain the strongest connected evidence set. It is enabled by default. When every relevant component cannot fit, packs include a structured `missing_links` summary rather than silently returning disconnected fragments.

Cross-file support closure is also enabled by default. It follows identifiers extracted by the language adapters, includes matching definitions as signature-only evidence, and reports `unresolved_symbols` with candidate provenance when a definition is unavailable or cannot fit. Strategy evaluation penalizes that missing-definition risk through `unbound_penalty_weight`; set `support_enabled = false` only as an explicit low-budget opt-out.

### Token budgeting

`budget_tokens` enforces a token budget during packing.

Current behavior:
- Uses `tiktoken-rs` for token counting.
- Default tokenizer is `o200k_base` (good default for modern OpenAI "o-series" models).
- Configure via the strategy field `tokenizer`, or per-invocation via:
  - CLI: `ce pack --tokenizer o200k_base` (or `cl100k_base`, `model:gpt-4o`, ...)
  - MCP: `context.pack` argument `tokenizer`.

If the tokenizer spec can't be resolved, the packer falls back to a conservative heuristic and
adds a note in the produced pack.

### Uniform policy-driven AST slicing

Rust, TypeScript/TSX, and Swift/SwiftUI implement the same `AstSlicePolicy`: public declarations, docs and types are structural evidence; bodies follow `none`, `referenced_only`, or `top_k_relevant`; call sites and branches are selected from symbol/error-span relevance. The older string modes remain compatibility fallbacks.

When a candidate is selected for a “body upgrade”, the engine can swap the signature for:

- the full body (`Body`), or
- a compact excerpt (`Slice`) focused on signals / relevant tokens.

Key strategy fields:

- `body_snippet_mode`:
  - `full` => always full bodies
  - otherwise this is intentionally stringly-typed and feature-gated by substrings:
    - include `signals` to enable file:line signal slicing
    - include `symbols` to enable “focus token” slicing (task tokens ∩ fragment refs)
    - include `ast` to enable Rust AST-based pruning (statement-aware excerpts)
    - include `skeleton` to enable Rust AST skeletonization (control-flow + structure, bounded)
    - include `query_grep` to enable broad task-token grep slicing
  - when multiple are enabled, the engine tries: signals → symbols → ast → skeleton → query_grep
- `body_snippet_context_lines`: lines of context around a matched/signal line
- `body_snippet_max_lines`: upper bound on slice size
- `body_snippet_min_savings_tokens`: only use a slice when it saves at least N tokens vs the full body

Skeleton knobs (Rust, used when `body_snippet_mode` contains `skeleton`):

- `ast_skeleton_max_nodes`, `ast_skeleton_max_depth`
- `ast_skeleton_match_arm_limit`, `ast_skeleton_impl_method_limit`
- `ast_skeleton_large_literal_line_threshold`, `ast_skeleton_large_literal_elem_threshold`, `ast_skeleton_large_literal_head_lines`
- `ast_skeleton_max_line_chars`

### API summary fragments

At index time, the CLI generates a small synthetic fragment per file (`FragKind::ApiSummary`).
This gives the LLM an inexpensive overview of a file's surface area without ingesting the whole file.

The summary generator is **language-agnostic** (best-effort heuristics): it prefers
"public/exported" definitions when it can detect them (e.g. `pub`, `export`, `public`).

You can optionally have the retrieval layer **inject** ApiSummary fragments for the most relevant
file paths (even if the summary itself didn't match the query directly) by setting:

- `include_api_summaries = true`
- `api_summary_max`, `api_summary_scan_top_n`, `api_summary_score_mul`, `api_summary_score_bonus`

See `strategies/summary_first_large_repo.toml` for a ready-to-use "summary-first" profile.

### Session-aware repetition avoidance (MCP)

The MCP server can maintain a best-effort, in-memory set of “seen” fragment ids per `session_id`.
If `session_id` is provided to `context.pack`, candidates already seen can be downweighted via:

- `avoid_seen` (bool)
- `seen_score_mul` (float)

This reduces repeated context across multi-step agent loops.

---

## Evaluation (no LLM)

To evolve strategies (DGM-style), you need a fast fitness signal.

`ce eval` scores the Context Engine on whether it retrieves/pack the *expected* paths/symbols for a task.

Task set format: JSONL, one object per line:

```json
{"id":"t1","task":"Fix failing test...","expect_paths":["src/auth.rs"],"expect_symbols":["Token"]}
```

SWE-bench-ish input is supported too. If a task line contains `problem_statement` (and optionally `hints_text`) plus a unified diff `patch`, the CLI will derive `expect_paths` (and can optionally derive Rust symbol names) for evaluation.

You can also convert datasets into Context Engine eval JSONL:

```bash
./target/release/ce tasks import-swebench \
  --input ./swebench.json \
  --out ./eval/swebench_tasks.jsonl \
  --derive-symbols
```

Run:

```bash
./target/release/ce eval --db .ce/index.sqlite --hnsw-dir .ce/hnsw \
  --tasks ./eval/tasks.jsonl \
  --strategy-file strategies/high_recall.toml \
  --out ./eval/results.jsonl
```

### Population strategy evolution (no LLM)

`ce strategy evolve` uses mutation plus deterministic crossover to tune operating thresholds, budgets, and heuristic weights. It is not a learned relevance model. Each generation keeps and stores its Pareto front across resolved rate, tokens, latency, redundancy, and missing-definition risk.

### Evidence retrieval versus declaration rendering

`ce-core::evidence` separates fine-grained retrieval units from coherent rendering units. An `EvidenceGraph` contains region nodes (`compute`, `call`, `projection`, branches, expressions, and related kinds), typed dependency edges, and an explicit provenance stratum. Every evidence node names an owning declaration fragment. `pack_evidence_with_strategy` ranks and selects region nodes under the evidence token budget, then renders each selected owner at most once through the existing declaration packer. Its result exposes both `selected_evidence_node_ids` and `rendered_owner_fragment_ids`.

Canonical LPM JSONL ingestion marks graphs as `lpm_exact` and accepts trace-derived `trace_touched` and `causally_required` facts directly; these are structural oracle labels, not execution-accuracy deltas. Source adapters must use the separate `source_approximate` stratum so evaluation cannot silently confound exact IR dependencies with conservative source resolution.

```bash
./target/release/ce strategy evolve --db .ce/index.sqlite \
  --tasks ./eval/tasks.jsonl \
  --base-strategy-file strategies/balanced.toml \
  --generations 20 --population 25 \
  --name-prefix "evolved"
```

### SWE-bench Stage B

The Stage-B runner performs dataset loading, base-commit checkout, indexing, context packing, an optional patch-agent command, predictions JSONL generation, and official SWE-bench harness evaluation:

```bash
ce tasks run-swe-bench --input ./swebench.jsonl \
  --workspace .ce/stage-b-work --out .ce/stage-b-results \
  --agent-command 'my-agent --context "$PRUNE_CONTEXT_PATH" --patch "$PRUNE_PATCH_PATH"'
```

Use `--dry-run` to validate the complete command plan. The default harness command invokes `python -m swebench.harness.run_evaluation`; override `--harness-command` when the installed harness requires additional dataset/split arguments.

### Automatic profiles and repository memory

When no explicit strategy is supplied, CLI and MCP classify the task as bugfix, feature, refactor, or integration and combine it with the indexed repository archetype. Every pack records the selection, confidence, and fallback reason.

Store durable context with `ce memory add --kind decision ...` or `--kind golden_path ...`. Relevant records are retrieved and included in subsequent packs. MCP clients can use `memory.add` and `memory.list`.

---

## Next steps

- Improve edges (imports/calls/defs) via deeper AST traversal.
- Add LSP-powered edges (rust-analyzer) for higher precision.
- Add additional language packs beyond Rust, TypeScript/TSX, and Swift/SwiftUI.

---

## License

MIT (feel free to change).
