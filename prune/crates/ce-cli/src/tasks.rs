use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashSet;

/// A lightweight evaluation task specification.
///
/// The CLI supports two broad input styles:
///
/// 1) Native Context Engine eval JSONL:
///    {"id":"t1","task":"...","expect_paths":[...],"expect_symbols":[...]}
///
/// 2) SWE-bench-ish instances:
///    {"instance_id":"...","problem_statement":"...","hints_text":"...","patch":"...", ...}
///
/// For style (2), we derive expectations from `patch` if `expect_*` are missing.
#[derive(Debug, Clone)]
pub struct EvalTask {
    pub id: Option<String>,
    pub task: String,
    pub expect_paths: Vec<String>,
    pub expect_symbols: Vec<String>,
    pub iterations: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    pub derive_symbols: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            derive_symbols: true,
        }
    }
}

pub fn parse_eval_task(v: &Value) -> Result<EvalTask> {
    parse_eval_task_with(v, ParseOptions::default())
}

pub fn parse_eval_task_with(v: &Value, opts: ParseOptions) -> Result<EvalTask> {
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("instance_id").and_then(|x| x.as_str()))
        .map(|s| s.to_string());

    // --- Task text ---
    let mut task = if let Some(t) = v.get("task").and_then(|x| x.as_str()) {
        t.to_string()
    } else if let Some(t) = v.get("problem_statement").and_then(|x| x.as_str()) {
        t.to_string()
    } else if let Some(t) = v.get("prompt").and_then(|x| x.as_str()) {
        t.to_string()
    } else {
        return Err(anyhow!(
            "task missing (expected one of: task, problem_statement, prompt)"
        ));
    };

    // Add optional hint fields commonly present in SWE-bench-like datasets.
    for k in ["hints_text", "context", "failure", "error"] {
        if let Some(h) = v.get(k).and_then(|x| x.as_str()) {
            let h = h.trim();
            if !h.is_empty() {
                task.push_str("\n\n");
                task.push_str(h);
            }
        }
    }

    // --- Expectations ---
    let mut expect_paths = parse_string_array(v.get("expect_paths"));
    let mut expect_symbols = parse_string_array(v.get("expect_symbols"));

    let patch = find_patch(v);
    if expect_paths.is_empty() {
        if let Some(p) = patch {
            expect_paths = extract_paths_from_patch(p);
        }
    }

    if opts.derive_symbols && expect_symbols.is_empty() {
        if let Some(p) = patch {
            // Heuristic: only attempt symbol extraction if there are Rust-ish paths.
            let looks_rust = expect_paths.iter().any(|p| p.ends_with(".rs"));
            if looks_rust {
                expect_symbols = extract_rust_symbols_from_patch(p);
            }
        }
    }

    let iterations = parse_optional_f32(v.get("iterations"));

    Ok(EvalTask {
        id,
        task,
        expect_paths,
        expect_symbols,
        iterations,
    })
}

fn parse_string_array(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|y| y.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_optional_f32(v: Option<&Value>) -> Option<f32> {
    let v = v?;
    if let Some(n) = v.as_f64() {
        return Some(n as f32);
    }
    if let Some(s) = v.as_str() {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Ok(n) = s.parse::<f32>() {
            return Some(n);
        }
    }
    None
}

fn find_patch<'a>(v: &'a Value) -> Option<&'a str> {
    for key in [
        "patch",
        "gold_patch",
        "golden_patch",
        "solution_patch",
        "answer_patch",
    ] {
        if let Some(p) = v.get(key).and_then(|x| x.as_str()) {
            if !p.trim().is_empty() {
                return Some(p);
            }
        }
    }
    None
}

/// Extract touched file paths from a unified diff.
///
/// Supports common formats:
/// - `diff --git a/... b/...`
/// - `+++ b/...` / `--- a/...`
pub fn extract_paths_from_patch(patch: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                // Prefer the b/ path.
                let p = parts[3];
                let p = p.strip_prefix("b/").unwrap_or(p);
                if p != "/dev/null" && !p.is_empty() {
                    if seen.insert(p.to_string()) {
                        out.push(p.to_string());
                    }
                }
            }
            continue;
        }

        if line.starts_with("+++ ") {
            let p = line.trim_start_matches("+++ ").trim();
            if p == "/dev/null" {
                continue;
            }
            let p = p.strip_prefix("b/").unwrap_or(p);
            if !p.is_empty() {
                if seen.insert(p.to_string()) {
                    out.push(p.to_string());
                }
            }
        }
    }

    out.sort();
    out
}

/// Heuristic extraction of Rust symbol names from a unified diff.
///
/// This is intentionally lightweight: it looks for `fn/struct/enum/trait/type/mod` declarations
/// on added/removed lines.
pub fn extract_rust_symbols_from_patch(patch: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for raw in patch.lines() {
        if raw.starts_with("+++") || raw.starts_with("---") || raw.starts_with("@@") {
            continue;
        }
        let (sign, line) = match raw.chars().next() {
            Some('+') => ('+', &raw[1..]),
            Some('-') => ('-', &raw[1..]),
            _ => continue,
        };
        // Ignore purely whitespace lines.
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // We don't currently distinguish + vs -; both can indicate relevant symbols.
        let _ = sign;

        // Tokenize by whitespace for quick scanning.
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }

        // Find the first keyword occurrence.
        for (i, &tok) in toks.iter().enumerate() {
            let kw = match tok {
                "fn" | "struct" | "enum" | "trait" | "type" | "mod" => Some(tok),
                _ => None,
            };
            let Some(_kw) = kw else {
                continue;
            };
            if i + 1 >= toks.len() {
                continue;
            }
            let name_tok = toks[i + 1];
            if let Some(name) = clean_ident(name_tok) {
                if seen.insert(name.to_string()) {
                    out.push(name.to_string());
                }
            }
        }
    }

    out.sort();
    if out.len() > 32 {
        out.truncate(32);
    }
    out
}

fn clean_ident(tok: &str) -> Option<&str> {
    let t = tok.trim();
    if t.is_empty() {
        return None;
    }
    // Strip common punctuation following the identifier.
    let end = t
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(t.len());
    let ident = &t[..end];
    if ident.len() < 2 {
        return None;
    }
    if !ident.chars().next().unwrap_or('_').is_ascii_alphabetic()
        && ident.chars().next().unwrap_or('_') != '_'
    {
        return None;
    }
    Some(ident)
}
