//! Body compaction helpers.
//!
//! The Context Engine often wants to include **something** from a fragment body,
//! but full bodies can be expensive in tokens. This module implements simple,
//! deterministic “slices” that preserve the most relevant lines.
//!
//! Implemented slice strategies:
//! - **file:line slice**: include a window of lines around a target file line
//! - **grep slice**: include windows around lines that match task tokens

use std::cmp::{max, min};

#[derive(Debug, Clone, Copy)]
struct Window {
    start: usize,
    end: usize, // inclusive
}

fn leading_ws(s: &str) -> &str {
    let mut n = 0usize;
    for (i, ch) in s.char_indices() {
        if ch == ' ' || ch == '\t' {
            n = i + ch.len_utf8();
        } else {
            break;
        }
    }
    &s[..n]
}

fn merge_windows(mut w: Vec<Window>) -> Vec<Window> {
    if w.is_empty() {
        return w;
    }
    w.sort_by_key(|x| x.start);
    let mut out: Vec<Window> = Vec::new();
    let mut cur = w[0];
    for win in w.into_iter().skip(1) {
        if win.start <= cur.end + 1 {
            cur.end = max(cur.end, win.end);
        } else {
            out.push(cur);
            cur = win;
        }
    }
    out.push(cur);
    out
}

fn windows_to_text(
    lines: &[&str],
    frag_start_line0: u32,
    wins: &[Window],
    max_lines: usize,
) -> String {
    let mut out = String::new();
    let mut emitted = 0usize;
    let mut first = true;

    // Indent-aware placeholders: when we skip a region between windows, emit
    // `...` with indentation that matches the surrounding code.
    let indent_size: usize = 4;
    let gap_indent = |prev: &str, next: &str| -> String {
        let p = prev.trim_end();
        let n = next.trim_start();
        let ip = leading_ws(prev).to_string();
        let inext = leading_ws(next).to_string();

        // If we appear to be collapsing a block between `{` and `}`, indent into the block.
        if p.ends_with('{') && n.starts_with('}') {
            let mut s = String::new();
            s.push_str(&ip);
            s.push_str(&" ".repeat(indent_size));
            return s;
        }

        // Otherwise pick the deeper indentation so the placeholder looks like it's inside.
        if inext.len() > ip.len() {
            inext
        } else {
            ip
        }
    };

    let mut prev_end_for_indent: Option<usize> = None;

    for win in wins {
        if emitted >= max_lines {
            break;
        }
        if !first {
            let pe = prev_end_for_indent.unwrap_or(win.start.saturating_sub(1));
            let prev_line = lines.get(pe).copied().unwrap_or("");
            let next_line = lines.get(win.start).copied().unwrap_or("");
            let ind = gap_indent(prev_line, next_line);
            out.push_str(&format!("{}...\n", ind));
            emitted += 1;
            if emitted >= max_lines {
                break;
            }
        }
        first = false;

        for i in win.start..=win.end {
            if i >= lines.len() {
                break;
            }
            if emitted >= max_lines {
                break;
            }
            let file_line_1based = frag_start_line0.saturating_add(i as u32).saturating_add(1);
            out.push_str(&format!("L{:>5}: {}\n", file_line_1based, lines[i]));
            emitted += 1;
        }

        prev_end_for_indent = Some(win.end.min(lines.len().saturating_sub(1)));
    }

    out
}

/// Slice a fragment body around one or more target file line numbers.
///
/// - `frag_start_line0` is the fragment's start line in the file (0-based).
/// - `targets_1based` are file line numbers (1-based).
///
/// Returns `None` if no target line falls within the body.
pub fn slice_by_file_lines(
    body: &str,
    frag_start_line0: u32,
    targets_1based: &[u32],
    context_lines: usize,
    max_lines: usize,
) -> Option<String> {
    if targets_1based.is_empty() || max_lines == 0 {
        return None;
    }

    let lines: Vec<&str> = body.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut wins: Vec<Window> = Vec::new();
    for &t1 in targets_1based {
        if t1 == 0 {
            continue;
        }
        let t0 = t1.saturating_sub(1);
        // relative index within fragment
        if t0 < frag_start_line0 {
            continue;
        }
        let rel = (t0 - frag_start_line0) as usize;
        if rel >= lines.len() {
            continue;
        }
        let start = rel.saturating_sub(context_lines);
        let end = min(rel + context_lines, lines.len().saturating_sub(1));
        wins.push(Window { start, end });
    }

    if wins.is_empty() {
        return None;
    }

    let wins = merge_windows(wins);
    Some(windows_to_text(&lines, frag_start_line0, &wins, max_lines))
}

/// Slice a fragment body by matching task tokens.
///
/// `query_tokens` should be lowercased identifier-ish tokens (e.g. from
/// `ce_core::util::extract_ident_tokens`).
///
/// Returns `None` if no matching lines were found.
pub fn slice_by_grep(
    body: &str,
    frag_start_line0: u32,
    query_tokens: &[String],
    context_lines: usize,
    max_lines: usize,
) -> Option<String> {
    if query_tokens.is_empty() || max_lines == 0 {
        return None;
    }

    let lines: Vec<&str> = body.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // Score each line by how many query tokens it matches.
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let ll = line.to_ascii_lowercase();
        let mut cnt = 0usize;
        for t in query_tokens {
            if t.len() >= 2 && ll.contains(t) {
                cnt += 1;
            }
        }
        if cnt > 0 {
            hits.push((i, cnt));
        }
    }

    if hits.is_empty() {
        return None;
    }

    // Keep the most informative lines, but preserve ordering for readability.
    hits.sort_by(|a, b| b.1.cmp(&a.1));
    hits.truncate(16);
    let mut idxs: Vec<usize> = hits.into_iter().map(|(i, _)| i).collect();
    idxs.sort();
    idxs.dedup();

    let mut wins: Vec<Window> = Vec::new();
    for idx in idxs {
        let start = idx.saturating_sub(context_lines);
        let end = min(idx + context_lines, lines.len().saturating_sub(1));
        wins.push(Window { start, end });
    }

    let wins = merge_windows(wins);
    Some(windows_to_text(&lines, frag_start_line0, &wins, max_lines))
}

fn trim_jsx_props(line: &str, max_props: usize) -> Option<String> {
    if max_props == 0 {
        return Some(line.to_string());
    }
    let trimmed = line.trim_start();
    if !trimmed.starts_with('<') || trimmed.starts_with("</") {
        return None;
    }

    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];

    let end = trimmed.find('>')?;
    let (head, _tail) = trimmed.split_at(end);
    let closing = &trimmed[end..];
    let inner = head.trim_start_matches('<').trim();

    let toks: Vec<&str> = inner.split_whitespace().collect();
    if toks.is_empty() {
        return None;
    }

    let tag = toks[0];
    let mut props: Vec<&str> = Vec::new();
    for t in toks.iter().skip(1) {
        if t.contains('=') || t.starts_with('{') {
            props.push(*t);
        }
        if props.len() >= max_props {
            break;
        }
    }

    let mut out = String::new();
    out.push_str(indent);
    out.push('<');
    out.push_str(tag);
    if !props.is_empty() {
        out.push(' ');
        out.push_str(&props.join(" "));
        if toks.len().saturating_sub(1) > props.len() {
            out.push_str(" ...");
        }
    }
    out.push_str(closing);
    Some(out)
}

pub fn skeletonize_tsx(body: &str, max_depth: usize, max_props: usize) -> Option<String> {
    if max_depth == 0 {
        return None;
    }

    let mut out = String::new();
    let mut depth = 0usize;
    let mut collapsed = false;
    let mut changed = false;

    for line in body.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len().saturating_sub(trimmed.len())];
        let is_open = trimmed.starts_with('<')
            && !trimmed.starts_with("</")
            && !trimmed.starts_with("<!")
            && !trimmed.starts_with("<?");
        let is_close = trimmed.starts_with("</");
        let self_close = trimmed.ends_with("/>");

        if depth >= max_depth && !is_close {
            if !collapsed {
                out.push_str(&format!("{}...<jsx/>\n", indent));
                changed = true;
                collapsed = true;
            }
        } else {
            if collapsed {
                collapsed = false;
            }
            if let Some(trimmed_line) = trim_jsx_props(line, max_props) {
                out.push_str(&trimmed_line);
                out.push('\n');
                if trimmed_line != line {
                    changed = true;
                }
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }

        if is_open && !self_close {
            depth = depth.saturating_add(1);
        }
        if is_close {
            depth = depth.saturating_sub(1);
        }
    }

    if changed {
        Some(out)
    } else {
        None
    }
}

fn is_swiftui_container(line: &str) -> bool {
    for name in [
        "VStack",
        "HStack",
        "ZStack",
        "List",
        "Form",
        "Section",
        "Group",
        "NavigationStack",
        "NavigationView",
        "ScrollView",
    ] {
        if line.contains(name) {
            return true;
        }
    }
    false
}

pub fn skeletonize_swiftui(body: &str, max_depth: usize, max_modifiers: usize) -> Option<String> {
    if max_depth == 0 {
        return None;
    }

    let mut out = String::new();
    let mut depth = 0usize;
    let mut collapsed = false;
    let mut changed = false;
    let mut modifiers_kept = 0usize;

    for line in body.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len().saturating_sub(trimmed.len())];
        let is_open = trimmed.ends_with('{') && is_swiftui_container(trimmed);
        let is_close = trimmed.starts_with('}');
        let is_modifier = trimmed.starts_with('.') && !trimmed.starts_with("..");

        if is_modifier {
            if modifiers_kept >= max_modifiers {
                if !collapsed {
                    out.push_str(&format!("{}...\n", indent));
                    changed = true;
                    collapsed = true;
                }
            } else {
                out.push_str(line);
                out.push('\n');
                modifiers_kept += 1;
            }
        } else if depth >= max_depth && !is_close {
            if !collapsed {
                out.push_str(&format!("{}...<view>\n", indent));
                changed = true;
                collapsed = true;
            }
        } else {
            collapsed = false;
            modifiers_kept = 0;
            out.push_str(line);
            out.push('\n');
        }

        if is_open {
            depth = depth.saturating_add(1);
        }
        if is_close {
            depth = depth.saturating_sub(1);
        }
    }

    if changed {
        Some(out)
    } else {
        None
    }
}
