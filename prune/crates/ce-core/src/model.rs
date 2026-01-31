use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type FragId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FragKind {
    Function,
    /// A method extracted from within an `impl` block.
    Method,
    /// A test function (best-effort detection via attributes like `#[test]`).
    Test,
    Struct,
    Enum,
    Trait,
    Impl,
    Mod,
    Const,
    Static,
    TypeAlias,
    Macro,
    /// A synthetic, file-level public API summary (signatures only).
    ApiSummary,
    /// External reference documentation snippet (not stored in the repo index).
    RefDoc,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fragment {
    pub id: FragId,       // content-addressed hash
    pub ast_hash: String, // structural hash (optional)
    pub file: PathBuf,
    pub kind: FragKind,
    pub symbol: Option<String>,
    pub span: Span,

    pub signature: String,
    pub body: String,
    pub doc: String,

    /// Text used for retrieval (FTS + embedding). Keep compact.
    pub retrieval_text: String,

    /// Best-effort referenced identifiers (used for graph-ish expansion)
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FragmentView {
    Signature,
    Body,
    /// A compact excerpt of the body (line slice / grep slice).
    Slice,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanHint {
    pub path: String,
    pub line: u32,
    pub col: Option<u32>,
    pub message: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolHint {
    pub name: String,
    pub kind: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleHint {
    pub specifier: String,
    pub importer_path: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestHint {
    pub name: String,
    pub suite: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHint {
    pub code: Option<String>,
    pub category: Option<String>,
    pub first_line: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHint {
    pub changed_paths: Vec<String>,
    pub hunk_spans: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignalBundle {
    pub spans: Vec<SpanHint>,
    pub symbols: Vec<SymbolHint>,
    pub modules: Vec<ModuleHint>,
    pub tests: Vec<TestHint>,
    pub errors: Vec<ErrorHint>,
    pub diffs: Vec<DiffHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignalStats {
    pub spans: usize,
    pub symbols: usize,
    pub modules: usize,
    pub tests: usize,
    pub errors: usize,
    pub diffs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackItem {
    pub id: FragId,
    pub view: FragmentView,
    pub path: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    /// Source span in the file (0-based line/col; bytes are UTF-8 offsets).
    pub span: Span,
    pub score: f32,
    pub reason: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDoc {
    pub provider: String,
    pub library: String,
    pub title: String,
    pub text: String,
    pub approx_tokens: usize,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalDocsSection {
    pub title: String,
    pub provider: String,
    pub max_tokens: usize,
    pub used_tokens: usize,
    pub snippets: Vec<ExternalDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredItem {
    pub id: FragId,
    pub path: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    /// Source span in the file (0-based line/col; bytes are UTF-8 offsets).
    pub span: Span,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedSymbol {
    pub symbol: String,
    #[serde(default)]
    pub candidates: Vec<DeferredItem>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackMetrics {
    pub pack_tokens_total: usize,
    pub baseline_tokens_total: Option<usize>,
    pub saved_pct: Option<f32>,
    pub hit_rate_paths: Option<f32>,
    pub unbound_symbol_count: usize,
    pub avg_iterations_per_fix: Option<f32>,
    pub redundancy_pct: Option<f32>,
    pub connectivity_score: Option<f32>,
    pub support_defs_added: usize,
    #[serde(default)]
    pub signals_extracted: SignalStats,
    #[serde(default)]
    pub signals_used: SignalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub pack_id: String,
    pub budget_chars: usize,
    pub used_chars: usize,
    /// Optional token budget. If present, packing enforces this budget.
    pub budget_tokens: Option<usize>,
    /// Token count for the produced pack.
    ///
    /// Note: This depends on the configured tokenizer/encoding.
    pub used_tokens: usize,
    pub items: Vec<PackItem>,
    pub deferred: Vec<DeferredItem>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub signals: SignalBundle,
    #[serde(default)]
    pub signals_used: Vec<String>,
    #[serde(default)]
    pub covers_symbols: Vec<String>,
    #[serde(default)]
    pub unresolved_symbols: Vec<UnresolvedSymbol>,
    #[serde(default)]
    pub metrics: PackMetrics,
    #[serde(default)]
    pub recipe_excerpt: Option<String>,
    #[serde(default)]
    pub external_docs: Vec<ExternalDocsSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StrategyConfig {
    /// How many lexical hits to retrieve (FTS)
    pub lexical_k: usize,
    /// How many semantic hits to retrieve (HNSW)
    pub semantic_k: usize,
    /// Blend factor: 0 = only lexical; 1 = only semantic
    pub hybrid_alpha: f32,

    /// Enable simple signal extraction from the task text.
    ///
    /// Currently implemented signals:
    /// - file:line hints (e.g. `src/lib.rs:123:45`, rustc `--> ...`)
    pub signals_enabled: bool,

    /// Maximum number of file:line hints to use from the task text.
    pub signal_file_line_max: usize,

    /// Score boost applied to fragments directly referenced by file:line signals.
    pub signal_file_line_boost: f32,

    /// Max number of span signals (path:line[:col]) to use.
    pub signal_max_spans: usize,

    /// Max number of path-only signals to use.
    pub signal_max_paths: usize,

    /// Score boost applied to fragments matched by span signals.
    pub signal_span_boost: f32,

    /// Enable lightweight graph-ish expansion.
    ///
    /// When true, after the initial hybrid retrieval step we expand the
    /// candidate set using cheap structural hints from the index:
    /// - same-file neighbors (by byte-offset proximity)
    /// - symbol definition lookup for referenced identifiers
    pub graph_expand: bool,

    /// How many top-ranked fragments are used as seeds for graph expansion.
    pub graph_seed_k: usize,

    /// Maximum hop distance for edge-based expansion.
    ///
    /// - 0: no edge traversal (neighbors/defs only)
    /// - 1: direct edges only
    /// - 2+: expand the subgraph outward
    pub edge_radius: usize,

    /// Maximum number of nodes we will add (via edges) per seed.
    /// This prevents explosion on ambiguous identifiers.
    pub edge_max_nodes_per_seed: usize,

    /// Maximum number of edges we will traverse per node (per direction).
    pub edge_max_edges_per_node: usize,

    /// Include outgoing edges (fragment -> definitions it refers to).
    pub edge_include_outgoing: bool,

    /// Include incoming edges (callers/users of a definition).
    pub edge_include_incoming: bool,

    /// Weight applied when adding nodes via an outgoing edge.
    pub edge_out_weight: f32,

    /// Weight applied when adding nodes via an incoming edge.
    pub edge_in_weight: f32,

    /// Per-hop multiplicative decay for edge expansion.
    /// e.g. 0.6 means depth-2 nodes receive ~36% of the depth-1 boost.
    pub edge_hop_decay: f32,

    /// Edge-type-specific traversal radii.
    ///
    /// The high-level `edge_radius` is kept for backward compatibility, but
    /// the traversal can be constrained per edge type to avoid exploding
    /// through file/module graphs.
    ///
    /// Defaults are conservative:
    /// - `refers` edges: follow up to `edge_radius`
    /// - module edges (`mod`/`use`): 1 hop
    /// - reverse edges (`imported_by`/`modded_by`): 1 hop
    pub edge_refers_radius: usize,
    pub edge_module_radius: usize,
    pub edge_reverse_radius: usize,

    /// Edge-type multipliers applied on top of the stored edge weight.
    ///
    /// This is a cheap “dial” for strategy search (DGM): you can bias the
    /// expander toward definition edges vs module graph edges.
    pub edge_mul_refers: f32,
    pub edge_mul_mod: f32,
    pub edge_mul_use: f32,
    pub edge_mul_imported_by: f32,
    pub edge_mul_modded_by: f32,
    pub edge_mul_other: f32,

    /// If true, edge expansion prioritizes edges by their type (definitions first).
    pub edge_prioritize_by_type: bool,

    /// Also use file-level `ApiSummary` fragments as additional graph seeds.
    ///
    /// This is especially helpful when the index contains file-level edges
    /// such as Rust `mod` / `use` module graph edges.
    pub file_summary_hub: bool,

    /// Score multiplier applied when seeding graph expansion from the file summary.
    pub file_summary_hub_weight: f32,

    /// How many neighbors (total) to pull from the same file for each seed.
    pub neighbors_k: usize,

    /// How many referenced identifiers to consider per seed fragment.
    pub refs_per_seed: usize,

    /// How many definition fragments to pull per referenced identifier.
    pub defs_per_ref: usize,

    /// Score multiplier applied to a seed score when adding a same-file neighbor.
    pub neighbor_weight: f32,

    /// Score multiplier applied to a seed score when adding a symbol definition.
    pub def_weight: f32,

    /// Enable constraint-based support closure ("no unbound names").
    pub support_enabled: bool,

    /// Max number of support definitions to add.
    pub support_max_defs: usize,

    /// Prefer signature-only support (cheaper).
    pub support_signature_only: bool,

    /// Minimum candidate score for a support definition.
    pub support_min_confidence: f32,

    /// Penalty weight used by strategies that score unbound symbols.
    pub unbound_penalty_weight: f32,

    /// Enable recipe memory lookup.
    pub recipes_enabled: bool,

    /// Max tokens allocated to recipe excerpt text.
    pub recipes_max_tokens: usize,

    /// Minimum Jaccard similarity required to include recipes.
    pub recipes_min_similarity: f32,

    /// Enable external docs injection (provider-controlled).
    pub docs_enabled: bool,

    /// Cap the candidate pool before loading fragment content and packing.
    pub candidate_pool_limit: usize,

    /// If true, the retrieval layer will try to include file-level `ApiSummary`
    /// fragments for the most relevant file paths.
    ///
    /// This is especially helpful for very large repos where you want a
    /// summary-first context pack (cheap overview before deep bodies).
    pub include_api_summaries: bool,

    /// Maximum number of ApiSummary fragments to inject.
    pub api_summary_max: usize,

    /// How many top ranked candidates to scan when selecting which file paths
    /// should get an ApiSummary injected.
    pub api_summary_scan_top_n: usize,

    /// Score multiplier applied to the best score found for a given file path
    /// when scoring its ApiSummary.
    pub api_summary_score_mul: f32,

    /// Score bonus added when scoring an injected ApiSummary.
    pub api_summary_score_bonus: f32,

    /// Max number of full bodies to include in pack
    pub max_bodies: usize,

    /// Prefer signatures first (true for most cases)
    pub signatures_first: bool,

    /// Char budget for packing (fallback). If `budget_tokens` is set, token budget is enforced.
    pub budget_chars: usize,

    /// Optional token budget for packing.
    ///
    /// If set, the packer enforces `budget_tokens` using the configured tokenizer.
    pub budget_tokens: Option<usize>,

    /// Tokenizer spec used for token counting.
    ///
    /// Accepted values (tiktoken encodings):
    /// - `o200k_base` (recommended for GPT-5 / o-series models)
    /// - `cl100k_base`
    /// - `p50k_base`, `p50k_edit`, `r50k_base`
    /// - `o200k_harmony`
    ///
    /// You may also pass a model name (e.g. `gpt-4o`) and we will try to resolve
    /// the tokenizer via tiktoken's model mapping.
    pub tokenizer: String,

    /// How to compact full bodies when doing “body upgrades”.
    ///
    /// Values:
    /// - `full` => always include full bodies
    ///
    /// Otherwise this is intentionally *stringly-typed* and feature-gated by
    /// substrings so it can be evolved via config search.
    ///
    /// If the string contains:
    /// - `signals`    => slice around file:line signals when available
    /// - `symbols`    => slice around lines matching “focus tokens” (task tokens ∩ fragment refs)
    /// - `ast`        => AST-based pruning (Rust only for now)
    /// - `skeleton`   => AST skeletonization (Rust only for now)
    /// - `query_grep` => slice around lines matching task tokens
    ///
    /// When multiple are enabled, the system tries: signals → symbols → ast → skeleton → query_grep,
    /// selecting the first slice that saves enough tokens.
    pub body_snippet_mode: String,

    /// Context lines to include around a matched/signal line when producing a slice.
    pub body_snippet_context_lines: usize,

    /// Max number of lines in a slice (soft cap; we may emit fewer).
    pub body_snippet_max_lines: usize,

    /// Only use a slice if it saves at least this many tokens vs the full body.
    ///
    /// This prevents “slicing” when it doesn't meaningfully reduce context.
    pub body_snippet_min_savings_tokens: usize,

    /// AST skeletonization / pruning knobs (Rust).
    ///
    /// These are used when `body_snippet_mode` contains `skeleton`.
    /// Skeletonization preserves control-flow headers and structural lines while
    /// omitting deep subtrees, match arms, and large literals.
    pub ast_skeleton_max_nodes: usize,
    /// Maximum AST depth to traverse while collecting skeleton lines.
    pub ast_skeleton_max_depth: usize,
    /// Maximum number of match arms to show for a `match` expression.
    pub ast_skeleton_match_arm_limit: usize,
    /// Maximum number of method signatures to show for an `impl` block skeleton.
    pub ast_skeleton_impl_method_limit: usize,
    /// If a literal spans at least this many lines, treat it as "large" and only show a head/tail skeleton.
    pub ast_skeleton_large_literal_line_threshold: usize,
    /// If a literal has at least this many named children, treat it as "large".
    pub ast_skeleton_large_literal_elem_threshold: usize,
    /// Number of head lines to show for large literals (after the opening line).
    pub ast_skeleton_large_literal_head_lines: usize,
    /// Max characters per emitted line in a skeleton. Lines longer than this are truncated.
    pub ast_skeleton_max_line_chars: usize,

    /// Rendering controls for compact slices.
    ///
    /// When slices omit large regions, we insert placeholder lines (`...`).
    /// These knobs control how those placeholders are rendered.
    pub placeholder_indent_spaces: usize,

    /// Enable block-aware collapsing in slice rendering.
    ///
    /// When enabled and the renderer sees a gap between a line ending in `{`
    /// and a line starting with `}` (at the same indentation), it may collapse
    /// the block into a single line like `{ ... }`.
    pub block_collapse_enabled: bool,

    /// Minimum number of omitted lines required before collapsing a `{ ... }` block.
    pub block_collapse_min_gap_lines: usize,

    /// If a session provides a set of "seen" fragment ids, apply a score multiplier
    /// to reduce repetition.
    pub avoid_seen: bool,

    /// Score multiplier applied to candidates that were already seen.
    ///
    /// 1.0 = no penalty; 0.0 = completely suppress.
    pub seen_score_mul: f32,

    /// MMR (max marginal relevance) diversification factor.
    ///
    /// - 1.0 => pure relevance (no diversity)
    /// - 0.0 => pure diversity
    pub mmr_lambda: f32,

    /// Apply MMR selection only to the top-N candidates by relevance.
    pub mmr_top_n: usize,

    /// Max number of signature items per file path.
    pub per_file_cap_signatures: usize,

    /// Max number of body upgrades per file path.
    pub per_file_cap_bodies: usize,

    /// Enable connected-subgraph selection for signature packing.
    pub subgraph_enabled: bool,

    /// Beam width for subgraph selection (if enabled).
    pub beam_width: usize,

    /// Max hop distance when connecting nodes.
    pub max_hops: usize,

    /// Connectivity penalty applied for distant nodes.
    pub connectivity_penalty: f32,

    /// TSX skeletonization: max JSX depth to render before collapsing.
    pub tsx_skeleton_max_depth: usize,

    /// TSX skeletonization: max props per element to show.
    pub tsx_skeleton_max_props: usize,

    /// SwiftUI skeletonization: max view builder depth to render.
    pub swiftui_skeleton_max_depth: usize,

    /// SwiftUI skeletonization: max modifier lines to keep per view.
    pub swiftui_skeleton_max_modifiers: usize,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            lexical_k: 40,
            semantic_k: 40,
            hybrid_alpha: 0.50,
            signals_enabled: true,
            signal_file_line_max: 8,
            signal_file_line_boost: 2.0,
            signal_max_spans: 8,
            signal_max_paths: 12,
            signal_span_boost: 2.0,
            graph_expand: true,
            graph_seed_k: 20,
            edge_radius: 2,
            edge_max_nodes_per_seed: 48,
            edge_max_edges_per_node: 12,
            edge_include_outgoing: true,
            edge_include_incoming: true,
            edge_out_weight: 0.55,
            edge_in_weight: 0.30,
            edge_hop_decay: 0.60,
            edge_refers_radius: 2,
            edge_module_radius: 1,
            edge_reverse_radius: 1,
            edge_mul_refers: 1.0,
            edge_mul_mod: 0.90,
            edge_mul_use: 0.80,
            edge_mul_imported_by: 0.30,
            edge_mul_modded_by: 0.30,
            edge_mul_other: 0.70,
            edge_prioritize_by_type: true,
            file_summary_hub: true,
            file_summary_hub_weight: 0.55,
            neighbors_k: 4,
            refs_per_seed: 8,
            defs_per_ref: 2,
            neighbor_weight: 0.35,
            def_weight: 0.55,
            support_enabled: false,
            support_max_defs: 12,
            support_signature_only: true,
            support_min_confidence: 0.05,
            unbound_penalty_weight: 0.0,
            recipes_enabled: false,
            recipes_max_tokens: 256,
            recipes_min_similarity: 0.35,
            docs_enabled: false,
            candidate_pool_limit: 250,
            include_api_summaries: false,
            api_summary_max: 24,
            api_summary_scan_top_n: 120,
            api_summary_score_mul: 0.60,
            api_summary_score_bonus: 0.12,
            max_bodies: 2,
            signatures_first: true,
            budget_chars: 12000,
            budget_tokens: None,
            tokenizer: "o200k_base".to_string(),
            body_snippet_mode: "signals_or_query_grep".to_string(),
            body_snippet_context_lines: 4,
            body_snippet_max_lines: 160,
            body_snippet_min_savings_tokens: 64,
            ast_skeleton_max_nodes: 64,
            ast_skeleton_max_depth: 10,
            ast_skeleton_match_arm_limit: 8,
            ast_skeleton_impl_method_limit: 24,
            ast_skeleton_large_literal_line_threshold: 12,
            ast_skeleton_large_literal_elem_threshold: 64,
            ast_skeleton_large_literal_head_lines: 4,
            ast_skeleton_max_line_chars: 240,
            placeholder_indent_spaces: 4,
            block_collapse_enabled: true,
            block_collapse_min_gap_lines: 8,
            avoid_seen: true,
            seen_score_mul: 0.25,
            mmr_lambda: 0.92,
            mmr_top_n: 80,
            per_file_cap_signatures: 8,
            per_file_cap_bodies: 1,
            subgraph_enabled: false,
            beam_width: 6,
            max_hops: 3,
            connectivity_penalty: 0.25,
            tsx_skeleton_max_depth: 3,
            tsx_skeleton_max_props: 6,
            swiftui_skeleton_max_depth: 3,
            swiftui_skeleton_max_modifiers: 4,
        }
    }
}
