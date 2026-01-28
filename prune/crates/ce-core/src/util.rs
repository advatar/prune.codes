use crate::signals;
use blake3::Hasher;

pub fn normalize_whitespace(s: &str) -> String {
    // Very simple normalization:
    // - convert CRLF -> LF
    // - trim trailing whitespace per line
    // - collapse repeated blank lines
    let s = s.replace("\r\n", "\n");
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;

    for line in s.lines() {
        let trimmed = line.trim_end();
        let blank = trimmed.is_empty();
        if blank {
            if prev_blank {
                continue;
            }
            prev_blank = true;
            out.push('\n');
            continue;
        }
        prev_blank = false;
        out.push_str(trimmed);
        out.push('\n');
    }

    out
}

pub fn fingerprint_failure(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            out.push('#');
        } else if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' {
            out.push('_');
        } else {
            out.push(' ');
        }
    }
    normalize_whitespace(&out)
}

pub fn failure_tokens(text: &str) -> Vec<String> {
    extract_ident_tokens(&fingerprint_failure(text))
}

pub fn hash_text_hex(s: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(s.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// A conservative token estimate for GPT-family tokenizers.
///
/// This is intentionally an approximation (no external tokenizer dependency).
///
/// Practical notes:
/// - For English prose, ~4 chars/token is a common rule of thumb.
/// - For code, identifiers and punctuation tend to reduce chars/token a bit.
///
/// We use bytes (UTF-8) rather than Unicode scalar count.
pub fn approx_tokens(text: &str) -> usize {
    let bytes = text.as_bytes().len();
    // Heuristic: assume ~3.8 bytes per token (slightly more conservative than 4).
    // Clamp to at least 1 for any non-empty string.
    if bytes == 0 {
        0
    } else {
        ((bytes as f32 / 3.8).ceil() as usize).max(1)
    }
}

pub fn approx_tokens_from_chars(chars: usize) -> usize {
    // Back-compat wrapper.
    if chars == 0 {
        0
    } else {
        ((chars as f32 / 3.8).ceil() as usize).max(1)
    }
}

/// Extract identifier-ish tokens from arbitrary text.
///
/// Used for MMR-style diversification without needing embeddings at pack time.
/// Returns a sorted, de-duplicated list (lowercased) for cheap set ops.
pub fn extract_ident_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch.to_ascii_lowercase());
        } else {
            if cur.len() >= 2 {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.sort();
    out.dedup();
    // Keep the set reasonably small to avoid O(n^2) blowups.
    if out.len() > 256 {
        out.truncate(256);
    }
    out
}

/// Extract up to `max` (path, line) hints from free-form text.
///
/// Examples:
/// - `src/lib.rs:123:45`
/// - rustc `--> src/main.rs:10:5`
///
/// This is used both for signal-based retrieval (boosting) and for
/// body compaction (slicing around the relevant line).
pub fn extract_file_line_hints(text: &str, max: usize) -> Vec<(String, u32)> {
    if max == 0 {
        return vec![];
    }

    let mut out: Vec<(String, u32)> = Vec::new();
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();

    for hint in signals::extract_span_hints(text, max) {
        let key = (hint.path.clone(), hint.line);
        if seen.insert(key) {
            out.push((hint.path, hint.line));
            if out.len() >= max {
                return out;
            }
        }
    }

    if out.len() < max {
        for diff in signals::extract_diff_hints(text, max) {
            for hunk in diff.hunk_spans {
                let key = (hunk.path.clone(), hunk.start_line);
                if seen.insert(key) {
                    out.push((hunk.path, hunk.start_line));
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
    }

    out
}

/// Jaccard similarity for two sorted, de-duplicated token lists.
pub fn jaccard_sorted(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut i = 0usize;
    let mut j = 0usize;
    let mut inter = 0usize;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                inter += 1;
                i += 1;
                j += 1;
            }
        }
    }
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}
