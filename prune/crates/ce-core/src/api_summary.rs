//! Language-agnostic file-level API summaries.
//!
//! We generate a synthetic `FragKind::ApiSummary` fragment per file at **index time**.
//! The goal is to provide a cheap, compact overview that can be injected into
//! the context pack for large repos.
//!
//! Design goals:
//! - Works across languages (best-effort heuristics).
//! - Stays small and deterministic.
//! - Produces useful `refs` for graph-ish expansion without exploding.

use crate::model::{FragKind, Fragment, Span};
use crate::util::{hash_text_hex, normalize_whitespace};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ApiSummaryOptions {
    /// Maximum number of items (lines) in the summary body.
    pub max_items: usize,
    /// If true, prefer “public-ish” items first (language heuristic).
    pub prefer_public: bool,
    /// Minimum number of items to include even if nothing looks public.
    pub min_items_if_no_public: usize,
    /// Maximum number of refs to attach (used for graph expansion).
    pub max_refs: usize,
}

impl Default for ApiSummaryOptions {
    fn default() -> Self {
        Self {
            max_items: 220,
            prefer_public: true,
            min_items_if_no_public: 24,
            max_refs: 128,
        }
    }
}

/// Build a synthetic file-level `ApiSummary` fragment.
///
/// - `path` should be the *indexed* path (ideally repo-relative).
/// - `language` is a short identifier (e.g. "rust", "python", "ts").
/// - `source` is the full file contents (used for span end-line only).
/// - `frags` are the extracted fragments for this file (excluding any ApiSummary).
/// - `extra_refs` can be supplied by a language adapter (e.g. import/use names).
pub fn build_api_summary(
    path: &Path,
    language: &str,
    source: &str,
    frags: &[Fragment],
    extra_refs: &[String],
    opts: &ApiSummaryOptions,
) -> Option<Fragment> {
    // Filter out any pre-existing ApiSummary fragments (defensive).
    let mut items: Vec<&Fragment> = frags
        .iter()
        .filter(|f| f.kind != FragKind::ApiSummary)
        .collect();

    if items.is_empty() {
        return None;
    }

    // Stable file order: sort by start byte.
    items.sort_by_key(|f| f.span.start_byte);

    let mut public_lines: Vec<String> = Vec::new();
    let mut all_lines: Vec<String> = Vec::new();

    for f in items.iter() {
        let sig1 = first_signature_line(&f.signature);
        if sig1.is_empty() {
            continue;
        }

        // Keep it compact.
        let compact = sig1.split_whitespace().collect::<Vec<_>>().join(" ");
        let label = label_for_kind(f.kind);
        let line = if compact.len() > 220 {
            format!("- ({label}) {}…", compact.chars().take(220).collect::<String>())
        } else {
            format!("- ({label}) {compact}")
        };

        let is_pub = if opts.prefer_public {
            looks_public(language, f.symbol.as_deref().unwrap_or(""), &sig1)
        } else {
            false
        };

        if is_pub {
            public_lines.push(line.clone());
        }
        all_lines.push(line);
    }

    if all_lines.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();

    if opts.prefer_public && !public_lines.is_empty() {
        lines.extend(public_lines);
    } else {
        // If nothing looks public, include a small but useful sample.
        lines.extend(all_lines.iter().take(opts.min_items_if_no_public.max(1)).cloned());
    }

    // If we have room, top up with remaining lines for more coverage.
    if lines.len() < opts.max_items {
        for l in all_lines {
            if lines.len() >= opts.max_items {
                break;
            }
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
    }

    if lines.len() > opts.max_items {
        lines.truncate(opts.max_items);
        lines.push("- …".to_string());
    }

    let summary = format!(
        "[api-summary]\npath: {}\nlanguage: {}\n\n{}",
        path.display(),
        language,
        lines.join("\n")
    );

    // Attach conservative refs.
    // - adapter-provided file refs (e.g. imports)
    // - tail segments of public-ish symbols we listed
    let mut refs: HashSet<String> = HashSet::new();
    for r in extra_refs {
        let t = r.trim();
        if !t.is_empty() && t.len() <= 128 {
            refs.insert(t.to_string());
        }
    }
    // Pull symbol tails from lines we included.
    for f in items {
        if let Some(sym) = &f.symbol {
            // Only add if it looks public-ish (keeps refs bounded).
            let sig1 = first_signature_line(&f.signature);
            let is_pub = if opts.prefer_public {
                looks_public(language, sym, &sig1)
            } else {
                false
            };
            if is_pub {
                for part in sym.split("::") {
                    let p = part.trim();
                    if !p.is_empty() && p.len() <= 128 {
                        refs.insert(p.to_string());
                    }
                }
            }
        }
    }

    let mut refs: Vec<String> = refs.into_iter().collect();
    refs.sort();
    if refs.len() > opts.max_refs {
        refs.truncate(opts.max_refs);
    }

    let content_hash = hash_text_hex(&normalize_whitespace(&summary));
    let ast_hash = hash_text_hex(&format!("api-summary:{}:{}", language, path.display()));

    let span = Span {
        start_byte: 0,
        end_byte: 0,
        start_line: 0,
        start_col: 0,
        end_line: source.lines().count().saturating_sub(1) as u32,
        end_col: 0,
    };

    Some(Fragment {
        id: content_hash,
        ast_hash,
        file: PathBuf::from(path),
        kind: FragKind::ApiSummary,
        symbol: None,
        span,
        signature: summary.clone(),
        body: summary.clone(),
        doc: String::new(),
        retrieval_text: summary,
        refs,
    })
}

fn label_for_kind(k: FragKind) -> &'static str {
    match k {
        FragKind::Method => "method",
        FragKind::Function => "fn",
        FragKind::Test => "test",
        FragKind::Struct => "struct",
        FragKind::Enum => "enum",
        FragKind::Trait => "trait",
        FragKind::Impl => "impl",
        FragKind::Mod => "mod",
        FragKind::Const => "const",
        FragKind::Static => "static",
        FragKind::TypeAlias => "type",
        FragKind::Macro => "macro",
        FragKind::ApiSummary => "api",
        FragKind::Other => "item",
    }
}

fn first_signature_line(signature: &str) -> String {
    signature
        .lines()
        .map(|l| l.trim())
        .find(|l| {
            !l.is_empty()
                && !l.starts_with("///")
                && !l.starts_with("//!")
                && !l.starts_with("#[")
                && !l.starts_with("//")
        })
        .unwrap_or("")
        .to_string()
}

fn looks_public(language: &str, symbol: &str, sig1: &str) -> bool {
    let l = sig1.trim();
    let lower = l.to_ascii_lowercase();

    match language {
        "rust" => l.contains("pub ") || l.contains("pub(") || l.starts_with("pub") || l.starts_with("extern "),
        "ts" | "tsx" | "js" | "jsx" => lower.starts_with("export ") || lower.contains(" export ") || lower.starts_with("declare export"),
        "java" | "kotlin" | "kt" | "cs" | "csharp" => lower.contains("public ") || lower.contains("protected "),
        "cpp" | "c" | "h" | "hpp" => lower.contains("extern ") || lower.contains("__declspec") || lower.contains("public:"),
        "python" | "py" => {
            // Convention: leading underscore is private.
            if symbol.is_empty() {
                false
            } else {
                !symbol.trim().starts_with('_')
            }
        }
        _ => {
            // Generic fallback.
            lower.contains("public ")
                || lower.contains("export ")
                || lower.contains("extern ")
                || (!symbol.is_empty() && !symbol.trim().starts_with('_'))
        }
    }
}
