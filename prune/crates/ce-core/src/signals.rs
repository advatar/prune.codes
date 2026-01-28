use crate::model::{
    DiffHint, DiffHunk, ErrorHint, ModuleHint, PackItem, SignalBundle, SignalStats, SpanHint,
    SymbolHint, TestHint,
};
use std::collections::HashSet;

pub fn extract_signals(text: &str, max_spans: usize, max_paths: usize) -> SignalBundle {
    let spans = extract_span_hints(text, max_spans);
    let symbols = extract_symbol_hints(text, 16);
    let modules = extract_module_hints(text, 12);
    let tests = extract_test_hints(text, 8);
    let errors = extract_error_hints(text);
    let diffs = extract_diff_hints(text, max_paths);

    SignalBundle {
        spans,
        symbols,
        modules,
        tests,
        errors,
        diffs,
    }
}

pub fn signal_stats(bundle: &SignalBundle) -> SignalStats {
    SignalStats {
        spans: bundle.spans.len(),
        symbols: bundle.symbols.len(),
        modules: bundle.modules.len(),
        tests: bundle.tests.len(),
        errors: bundle.errors.len(),
        diffs: bundle.diffs.len(),
    }
}

pub fn signals_used(bundle: &SignalBundle, items: &[PackItem]) -> (Vec<String>, SignalStats) {
    let mut used: Vec<String> = Vec::new();
    let mut stats = SignalStats::default();

    for span in &bundle.spans {
        if items.iter().any(|it| {
            path_matches(&it.path, &span.path)
                && span.line >= it.span.start_line.saturating_add(1)
                && span.line <= it.span.end_line.saturating_add(1)
        }) {
            used.push(format!("span:{}:{}", span.path, span.line));
            stats.spans += 1;
        }
    }

    for diff in &bundle.diffs {
        for path in &diff.changed_paths {
            if items.iter().any(|it| path_matches(&it.path, path)) {
                used.push(format!("diff:{}", path));
                stats.diffs += 1;
            }
        }
    }

    for sym in &bundle.symbols {
        if items.iter().any(|it| symbol_matches_item(it, &sym.name)) {
            used.push(format!("symbol:{}", sym.name));
            stats.symbols += 1;
        }
    }

    for module in &bundle.modules {
        if items.iter().any(|it| {
            it.path.ends_with(&module.specifier) || it.content.contains(&module.specifier)
        }) {
            used.push(format!("module:{}", module.specifier));
            stats.modules += 1;
        }
    }

    for test in &bundle.tests {
        if items.iter().any(|it| symbol_matches_item(it, &test.name)) {
            used.push(format!("test:{}", test.name));
            stats.tests += 1;
        }
    }

    if !bundle.errors.is_empty() && !items.is_empty() {
        stats.errors = bundle.errors.len();
        for err in &bundle.errors {
            if let Some(code) = &err.code {
                used.push(format!("error:{}", code));
            } else if let Some(cat) = &err.category {
                used.push(format!("error:{}", cat));
            }
        }
    }

    (used, stats)
}

fn path_matches(item_path: &str, hint_path: &str) -> bool {
    item_path == hint_path || item_path.ends_with(hint_path)
}

fn symbol_matches_item(item: &PackItem, name: &str) -> bool {
    if let Some(sym) = &item.symbol {
        if sym == name || sym.ends_with(&format!("::{name}")) {
            return true;
        }
    }
    item.content.contains(name)
}

pub fn extract_span_hints(text: &str, max: usize) -> Vec<SpanHint> {
    if max == 0 {
        return vec![];
    }
    let mut out: Vec<SpanHint> = Vec::new();
    let mut seen: HashSet<(String, u32, Option<u32>)> = HashSet::new();

    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut skip_until = 0usize;
        for (i, b) in bytes.iter().enumerate() {
            if i < skip_until {
                continue;
            }
            if *b != b':' {
                continue;
            }

            let (line_num, col, end_idx) = match parse_line_col(bytes, i + 1) {
                Some(v) => v,
                None => continue,
            };

            let start = find_path_start(bytes, i);
            if start >= i {
                continue;
            }
            let mut path = line[start..i].to_string();
            path = path
                .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
                .to_string();
            path = path.trim_start_matches("-->").trim().to_string();
            if !looks_like_path(&path) {
                continue;
            }

            let key = (path.clone(), line_num, col);
            if seen.insert(key) {
                out.push(SpanHint {
                    path,
                    line: line_num,
                    col,
                    message: None,
                    confidence: 0.9,
                });
                if out.len() >= max {
                    return out;
                }
            }

            skip_until = end_idx;
        }
    }

    out
}

pub fn extract_symbol_hints(text: &str, max: usize) -> Vec<SymbolHint> {
    if max == 0 {
        return vec![];
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for line in text.lines() {
        let ll = line.to_ascii_lowercase();
        if !(ll.contains("cannot find")
            || ll.contains("not found")
            || ll.contains("undefined")
            || ll.contains("unresolved"))
        {
            continue;
        }

        let mut candidates = extract_quoted_tokens(line);
        if candidates.is_empty() {
            if let Some(tok) = last_ident_token(line) {
                candidates.push(tok);
            }
        }

        for name in candidates {
            if name.len() < 2 {
                continue;
            }
            if seen.insert(name.clone()) {
                out.push(SymbolHint {
                    name,
                    kind: None,
                    confidence: 0.75,
                });
                if out.len() >= max {
                    return out;
                }
            }
        }
    }

    out
}

pub fn extract_module_hints(text: &str, max: usize) -> Vec<ModuleHint> {
    if max == 0 {
        return vec![];
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for line in text.lines() {
        let ll = line.to_ascii_lowercase();
        if !(ll.contains("cannot resolve module")
            || ll.contains("cannot find module")
            || ll.contains("module not found")
            || ll.contains("can't resolve"))
        {
            continue;
        }

        let mut candidates = extract_quoted_tokens(line);
        if candidates.is_empty() {
            if let Some(pos) = ll.find("module") {
                let tail = line[pos + "module".len()..].trim();
                if let Some(tok) = tail.split_whitespace().next() {
                    candidates.push(
                        tok.trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
                            .to_string(),
                    );
                }
            }
        }

        for spec in candidates {
            if spec.is_empty() {
                continue;
            }
            if seen.insert(spec.clone()) {
                out.push(ModuleHint {
                    specifier: spec,
                    importer_path: None,
                    confidence: 0.7,
                });
                if out.len() >= max {
                    return out;
                }
            }
        }
    }

    out
}

pub fn extract_test_hints(text: &str, max: usize) -> Vec<TestHint> {
    if max == 0 {
        return vec![];
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for line in text.lines() {
        let ll = line.to_ascii_lowercase();
        if !(ll.contains("test") && (ll.contains("fail") || ll.contains("error"))) {
            continue;
        }

        let mut candidates = extract_quoted_tokens(line);
        if candidates.is_empty() {
            if let Some(tok) = last_ident_token(line) {
                candidates.push(tok);
            }
        }

        for name in candidates {
            if seen.insert(name.clone()) {
                out.push(TestHint {
                    name,
                    suite: None,
                    confidence: 0.6,
                });
                if out.len() >= max {
                    return out;
                }
            }
        }
    }

    out
}

pub fn extract_error_hints(text: &str) -> Vec<ErrorHint> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let code = find_error_code(trimmed);
        let category = find_error_category(trimmed);
        return vec![ErrorHint {
            code,
            category,
            first_line: Some(trimmed.to_string()),
            confidence: 0.5,
        }];
    }

    Vec::new()
}

pub fn extract_diff_hints(text: &str, max_paths: usize) -> Vec<DiffHint> {
    let mut changed_paths = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut hunk_spans = Vec::new();

    let mut cur_path: Option<String> = None;
    for line in text.lines() {
        let l = line.trim_end();
        if let Some(p) = parse_unified_diff_path(l) {
            if seen_paths.insert(p.clone()) {
                if max_paths == 0 || changed_paths.len() < max_paths {
                    changed_paths.push(p.clone());
                }
            }
            cur_path = Some(p);
            continue;
        }

        if l.starts_with("@@") {
            let Some(path) = cur_path.as_ref() else {
                continue;
            };
            if let Some((start, end)) = parse_unified_diff_new_span(l) {
                hunk_spans.push(DiffHunk {
                    path: path.clone(),
                    start_line: start,
                    end_line: end,
                });
            }
        }
    }

    if changed_paths.is_empty() && hunk_spans.is_empty() {
        return Vec::new();
    }

    vec![DiffHint {
        changed_paths,
        hunk_spans,
    }]
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

fn find_path_start(bytes: &[u8], colon_idx: usize) -> usize {
    let mut start = colon_idx;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ','
            )
        {
            break;
        }
        start -= 1;
    }
    start
}

fn parse_line_col(bytes: &[u8], mut idx: usize) -> Option<(u32, Option<u32>, usize)> {
    let (line, next) = parse_number(bytes, idx)?;
    idx = next;
    let mut col = None;
    if idx < bytes.len() && bytes[idx] == b':' {
        if let Some((c, next2)) = parse_number(bytes, idx + 1) {
            col = Some(c);
            idx = next2;
        }
    }
    Some((line, col, idx))
}

fn parse_number(bytes: &[u8], mut idx: usize) -> Option<(u32, usize)> {
    let mut n: u32 = 0;
    let mut any = false;
    while idx < bytes.len() {
        let c = bytes[idx] as char;
        if !c.is_ascii_digit() {
            break;
        }
        any = true;
        n = n.saturating_mul(10).saturating_add((c as u8 - b'0') as u32);
        idx += 1;
    }
    if any && n > 0 {
        Some((n, idx))
    } else {
        None
    }
}

fn extract_quoted_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' || ch == '\'' || ch == '"' {
            let mut buf = String::new();
            while let Some(&nc) = chars.peek() {
                chars.next();
                if nc == ch {
                    break;
                }
                buf.push(nc);
            }
            let trimmed = buf.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

fn last_ident_token(line: &str) -> Option<String> {
    let mut cur = String::new();
    let mut last = None;
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if cur.len() >= 2 {
                last = Some(cur.clone());
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        last = Some(cur);
    }
    last
}

fn find_error_code(line: &str) -> Option<String> {
    let mut buf = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
            if buf.len() > 8 {
                buf.remove(0);
            }
            if buf.len() >= 2
                && buf.starts_with('E')
                && buf[1..].chars().all(|c| c.is_ascii_digit())
            {
                return Some(buf.clone());
            }
        } else {
            buf.clear();
        }
    }
    None
}

fn find_error_category(line: &str) -> Option<String> {
    for cat in [
        "TypeError",
        "ReferenceError",
        "AssertionError",
        "Fatal",
        "Panic",
    ] {
        if line.contains(cat) {
            return Some(cat.to_string());
        }
    }
    None
}

fn parse_unified_diff_path(line: &str) -> Option<String> {
    if line.starts_with("diff --git ") {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let b = parts[3];
            let p = b.strip_prefix("b/").unwrap_or(b);
            if p != "/dev/null" {
                return Some(p.to_string());
            }
        }
    }
    if line.starts_with("+++ ") {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let p0 = parts[1];
            if p0 == "/dev/null" {
                return None;
            }
            let p = p0.strip_prefix("b/").unwrap_or(p0);
            return Some(p.to_string());
        }
    }
    None
}

fn parse_unified_diff_new_span(line: &str) -> Option<(u32, u32)> {
    let plus = line.find('+')?;
    let bytes = line.as_bytes();
    let (start, idx) = parse_number(bytes, plus + 1)?;
    let mut len: u32 = 1;
    if idx < bytes.len() && bytes[idx] == b',' {
        if let Some((n, _next)) = parse_number(bytes, idx + 1) {
            len = n.max(1);
        }
    }
    let end = start.saturating_add(len.saturating_sub(1));
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_span_hints_with_col() {
        let text = "error at src/lib.rs:42:7";
        let spans = extract_span_hints(text, 4);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].path, "src/lib.rs");
        assert_eq!(spans[0].line, 42);
        assert_eq!(spans[0].col, Some(7));
    }

    #[test]
    fn extracts_diff_hunks() {
        let text = "diff --git a/foo.rs b/foo.rs\n@@ -1,2 +10,3 @@\n";
        let diffs = extract_diff_hints(text, 4);
        assert_eq!(diffs.len(), 1);
        let hint = &diffs[0];
        assert_eq!(hint.changed_paths, vec!["foo.rs"]);
        assert_eq!(hint.hunk_spans.len(), 1);
        assert_eq!(hint.hunk_spans[0].start_line, 10);
        assert_eq!(hint.hunk_spans[0].end_line, 12);
    }
}
