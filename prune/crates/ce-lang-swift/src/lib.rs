use anyhow::Result;
use ce_core::model::{FragKind, Fragment, Span};
use ce_core::util::{hash_text_hex, normalize_whitespace};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};

/// Swift / SwiftUI language adapter.
///
/// Design goals (v1):
/// - Deterministic, AST-first fragmentation using tree-sitter-swift.
/// - Pragmatic coverage for app repos: structs/classes/enums/protocols/extensions + top-level funcs.
/// - SwiftUI-aware member extraction: computed `var body: some View { ... }` becomes a Method fragment.
/// - Conservative identifier refs to power ref→def edges without exploding noise.
///
/// NOTE: SwiftUI is Swift; we do not need a separate grammar.
pub struct SwiftAdapter {
    parser: Parser,
}

/// Collect file-level refs from `import` lines.
///
/// These are attached to the file-level ApiSummary fragment at index time.
/// They help retrieval and explainability (e.g., `SwiftUI`, `Combine`, `Foundation`).
pub fn collect_file_level_refs(source: &str) -> Vec<String> {
    let mut out: HashSet<String> = HashSet::new();

    for raw in source.lines() {
        let mut line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // Strip single-line comments.
        if let Some((before, _)) = line.split_once("//") {
            line = before.trim();
        }
        if line.is_empty() {
            continue;
        }

        // Swift: `import Foo` or `@_exported import Foo`
        if line.starts_with("import ") || line.contains(" import ") {
            // normalize: remove attributes like @_exported
            let line = line.replace("@_exported", "");
            let line = line.trim();
            let Some(idx) = line.find("import") else { continue; };
            let rest = line[idx + "import".len()..].trim();
            let name = rest
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                .next()
                .unwrap_or("")
                .trim();
            if !name.is_empty() {
                // Commonly people write `import SwiftUI`.
                // If it is qualified (rare), keep both tail and full.
                out.insert(name.to_string());
                if let Some((_, tail)) = name.rsplit_once('.') {
                    if !tail.is_empty() {
                        out.insert(tail.to_string());
                    }
                }
            }
        }
    }

    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    v.truncate(128);
    v
}

impl SwiftAdapter {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_swift::LANGUAGE.into())?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str) -> Result<Tree> {
        self.parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter parse returned None"))
    }

    /// Extract fragments from Swift source.
    ///
    /// v1 extraction:
    /// - Top-level: func / struct / class / enum / protocol / extension / typealias / actor
    /// - Members: methods inside type declarations
    /// - SwiftUI special: computed property `body` becomes a Method fragment
    pub fn extract_fragments(&self, path: &Path, source: &str, tree: &Tree) -> Vec<Fragment> {
        let root = tree.root_node();
        let bytes = source.as_bytes();

        let mut out: Vec<Fragment> = Vec::new();
        let mut cursor = root.walk();

        for item in root.named_children(&mut cursor) {
            self.extract_from_top_level_node(path, bytes, item, &mut out);
        }

        // If the grammar changes / parsing fails, it is better to return *something*
        // than nothing. As a last resort, do a light lexical scan for declarations.
        if out.is_empty() {
            out.extend(lexical_fallback_fragments(path, source));
        }

        out
    }

    fn extract_from_top_level_node(&self, path: &Path, bytes: &[u8], node: Node, out: &mut Vec<Fragment>) {
        if let Some((kind, sym)) = classify_swift_decl(bytes, node) {
            // Create a fragment for the declaration itself.
            if let Some(f) = build_fragment(path, bytes, node, kind, sym.clone(), None) {
                out.push(f.clone());

                // If it's a type-like declaration, extract member methods (including SwiftUI `body`).
                if matches!(kind, FragKind::Struct | FragKind::Enum | FragKind::Trait | FragKind::Impl) {
                    let type_name = sym.as_deref().unwrap_or("Type");
                    self.collect_member_fragments(path, bytes, node, type_name, out);
                }
            }
            return;
        }

        // Not a top-level decl we care about; still recurse a little because
        // Swift source files may wrap declarations in conditional compilation blocks.
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            self.extract_from_top_level_node(path, bytes, ch, out);
        }
    }

    fn collect_member_fragments(&self, path: &Path, bytes: &[u8], type_node: Node, type_name: &str, out: &mut Vec<Fragment>) {
        // Walk descendants; extract methods/properties, but avoid nested function bodies.
        let mut cursor = type_node.walk();
        for ch in type_node.named_children(&mut cursor) {
            self.collect_member_fragments_rec(path, bytes, ch, type_name, /*inside_fn*/ false, out);
        }
    }

    fn collect_member_fragments_rec(
        &self,
        path: &Path,
        bytes: &[u8],
        node: Node,
        type_name: &str,
        inside_fn: bool,
        out: &mut Vec<Fragment>,
    ) {
        let k = node.kind();

        // Entering a function/closure should suppress member extraction below it.
        let entering_fn = matches!(k, "function_declaration" | "initializer_declaration" | "deinitializer_declaration" | "closure_expression" | "lambda_literal");
        let now_inside = inside_fn || entering_fn;

        if !inside_fn {
            // Method-like nodes
            if matches!(k, "function_declaration" | "initializer_declaration" | "deinitializer_declaration" | "subscript_declaration") {
                let name = swift_member_name(bytes, node)
                    .map(|n| format!("{}::{}", type_name, n))
                    .or_else(|| Some(format!("{}::method", type_name)));

                if let Some(f) = build_fragment(path, bytes, node, FragKind::Method, name, Some(type_name)) {
                    out.push(f);
                    // Do not recurse into body; we already captured this member.
                    return;
                }
            }

            // SwiftUI: computed property `body` (typically `var body: some View { ... }`)
            // We treat it as a Method fragment because it behaves like a render function.
            if looks_like_swiftui_body_property(bytes, node) {
                let name = Some(format!("{}::body", type_name));
                if let Some(f) = build_fragment(path, bytes, node, FragKind::Method, name, Some(type_name)) {
                    out.push(f);
                    return;
                }
            }

            // Nested type declarations inside a type are rare but can be relevant.
            if let Some((kind, sym)) = classify_swift_decl(bytes, node) {
                // Only capture nested type decls if they have a name.
                if sym.is_some() {
                    if let Some(f) = build_fragment(path, bytes, node, kind, sym.clone(), Some(type_name)) {
                        out.push(f);
                    }
                }
                // Continue recursion to collect members of nested types too.
            }
        }

        // Recurse
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            self.collect_member_fragments_rec(path, bytes, ch, type_name, now_inside, out);
        }
    }
}

// -----------------------------------------------------------------------------
// Fragment construction
// -----------------------------------------------------------------------------

fn build_fragment(
    path: &Path,
    bytes: &[u8],
    node: Node,
    kind: FragKind,
    symbol: Option<String>,
    _owner_type: Option<&str>,
) -> Option<Fragment> {
    let span = Span {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row as u32,
        start_col: node.start_position().column as u32,
        end_line: node.end_position().row as u32,
        end_col: node.end_position().column as u32,
    };

    let body = node.utf8_text(bytes).ok()?.to_string();

    let preamble = extract_preamble_from_source(bytes, node.start_byte());
    let signature_core = signature_from_body(&body);
    let signature = if preamble.is_empty() {
        signature_core
    } else {
        format!("{}\n{}", preamble, signature_core)
    };

    let doc = preamble
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("///") || t.starts_with("/**") || t.starts_with("* ")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content_hash = hash_text_hex(&normalize_whitespace(&body));
    let ast_hash = hash_text_hex(&node.to_sexp());

    let mut refs = collect_swift_identifiers(bytes, node);

    // Filter obvious junk.
    if let Some(sym) = &symbol {
        refs.retain(|r| r != sym);
        if let Some((_, tail)) = sym.rsplit_once("::") {
            refs.retain(|r| r != tail);
        }
    }

    // retrieval text should be compact and semantic
    let retrieval_text = format!(
        "path: {}\nkind: {:?}\nsymbol: {}\n{}\n{}",
        path.display(),
        kind,
        symbol.clone().unwrap_or_default(),
        if doc.is_empty() { preamble.clone() } else { doc.clone() },
        signature
    );

    Some(Fragment {
        id: content_hash,
        ast_hash,
        file: PathBuf::from(path),
        kind,
        symbol,
        span,
        signature,
        body,
        doc,
        retrieval_text,
        refs,
    })
}

fn signature_from_body(body: &str) -> String {
    // First meaningful line (skip doc/comment-only lines).
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with("///") || l.starts_with("//") {
            continue;
        }
        // Keep the first line; Swift declarations can be long, so cap.
        if l.len() > 260 {
            return format!("{}…", l.chars().take(260).collect::<String>());
        }
        return l.to_string();
    }
    body.lines().next().unwrap_or("").trim().to_string()
}

/// Extract a small preamble immediately above a node start.
///
/// We keep doc comments and attributes that often matter for behavior:
/// - `///` doc comments
/// - block comment starters `/**`
/// - attribute lines `@MainActor`, `@available`, `@State`, etc.
/// - conditional compilation lines `#if`, `#endif`, ...
fn extract_preamble_from_source(bytes: &[u8], start_byte: usize) -> String {
    // Pull up to N lines before start_byte.
    let prefix = &bytes[..start_byte.min(bytes.len())];
    let text = String::from_utf8_lossy(prefix);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut out: Vec<&str> = Vec::new();
    let take = 10usize;
    for raw in lines.iter().rev().take(take) {
        let l = raw.trim_end();
        let t = l.trim_start();
        if t.is_empty() {
            // keep scanning upward but do not include blank lines
            continue;
        }
        let keep = t.starts_with("///")
            || t.starts_with("/**")
            || t.starts_with("*")
            || t.starts_with("@")
            || t.starts_with("#if")
            || t.starts_with("#endif")
            || t.starts_with("#elseif")
            || t.starts_with("#else");
        if keep {
            out.push(l);
        } else {
            // Stop when we hit a non-preamble line.
            break;
        }
    }

    out.reverse();
    out.join("\n")
}

// -----------------------------------------------------------------------------
// Swift declaration classification
// -----------------------------------------------------------------------------

fn classify_swift_decl(bytes: &[u8], node: Node) -> Option<(FragKind, Option<String>)> {
    let k = node.kind();
    match k {
        // Top-level / nested declarations
        "function_declaration" => Some((FragKind::Function, swift_decl_name(bytes, node))),
        "struct_declaration" => Some((FragKind::Struct, swift_decl_name(bytes, node))),
        "class_declaration" => Some((FragKind::Struct, swift_decl_name(bytes, node))),
        "actor_declaration" => Some((FragKind::Struct, swift_decl_name(bytes, node))),
        "enum_declaration" => Some((FragKind::Enum, swift_decl_name(bytes, node))),
        "protocol_declaration" => Some((FragKind::Trait, swift_decl_name(bytes, node))),
        "extension_declaration" => {
            // Extensions act like impl blocks.
            Some((FragKind::Impl, swift_extension_target(bytes, node)))
        }
        "typealias_declaration" => Some((FragKind::TypeAlias, swift_decl_name(bytes, node))),
        _ => None,
    }
}

fn swift_decl_name(bytes: &[u8], node: Node) -> Option<String> {
    if let Some(n) = node.child_by_field_name("name") {
        return n.utf8_text(bytes).ok().map(|s| s.to_string());
    }
    // fallback: pick first identifier-like child
    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        if looks_like_identifier_kind(ch.kind()) {
            if let Ok(t) = ch.utf8_text(bytes) {
                let t = t.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn swift_extension_target(bytes: &[u8], node: Node) -> Option<String> {
    // Best-effort: first line of extension decl, trimmed.
    // Example: `extension Foo: Bar { ... }`
    let text = node.utf8_text(bytes).ok()?;
    let first = text.lines().next().unwrap_or("extension").trim();

    // Try to extract `extension <Target>`.
    if let Some(rest) = first.strip_prefix("extension") {
        let rest = rest.trim();
        let name = rest
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .next()
            .unwrap_or("")
            .trim();
        if !name.is_empty() {
            return Some(name.replace('.', "::"));
        }
    }
    Some("extension".to_string())
}

fn swift_member_name(bytes: &[u8], node: Node) -> Option<String> {
    if let Some(n) = node.child_by_field_name("name") {
        return n.utf8_text(bytes).ok().map(|s| s.to_string());
    }
    // initializers may not have a "name" field.
    let k = node.kind();
    if k == "initializer_declaration" {
        return Some("init".to_string());
    }
    if k == "deinitializer_declaration" {
        return Some("deinit".to_string());
    }

    // fallback: any identifier-like child
    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        if looks_like_identifier_kind(ch.kind()) {
            if let Ok(t) = ch.utf8_text(bytes) {
                let t = t.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn looks_like_swiftui_body_property(bytes: &[u8], node: Node) -> bool {
    // Heuristic:
    // - Node kind suggests variable/property declaration
    // - Contains identifier `body`
    // - Contains a code block `{ ... }` (computed property)
    let k = node.kind();
    let var_like = matches!(k, "variable_declaration" | "property_declaration" | "property_declaration_list" | "pattern_binding" | "pattern_binding_list");
    if !var_like {
        // Also handle grammars that use a generic `declaration` node.
        if !k.contains("variable") && !k.contains("property") {
            return false;
        }
    }

    let text = match node.utf8_text(bytes) {
        Ok(t) => t,
        Err(_) => return false,
    };

    // Fast checks.
    if !text.contains("body") {
        return false;
    }
    if !text.contains('{') {
        return false;
    }

    // Find an identifier child named "body".
    let mut cursor = node.walk();
    for ch in node.named_children(&mut cursor) {
        if looks_like_identifier_kind(ch.kind()) {
            if let Ok(t) = ch.utf8_text(bytes) {
                if t.trim() == "body" {
                    return true;
                }
            }
        }
    }

    // Fallback: lexical.
    text.lines().any(|l| l.trim_start().starts_with("var body") || l.trim_start().starts_with("let body"))
}

fn looks_like_identifier_kind(kind: &str) -> bool {
    // Swift grammar variants:
    // - identifier
    // - type_identifier
    // - simple_identifier
    // - self_identifier, etc.
    kind.contains("identifier")
}

// -----------------------------------------------------------------------------
// Identifier refs
// -----------------------------------------------------------------------------

fn collect_swift_identifiers(bytes: &[u8], node: Node) -> Vec<String> {
    let mut out: HashSet<String> = HashSet::new();
    let mut cursor = node.walk();
    collect_swift_identifiers_rec(bytes, node, &mut cursor, &mut out);

    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    v.truncate(160);
    v
}

fn collect_swift_identifiers_rec(
    bytes: &[u8],
    node: Node,
    _cursor: &mut tree_sitter::TreeCursor,
    out: &mut HashSet<String>,
) {
    let k = node.kind();

    // Leaf identifiers (best-effort).
    if looks_like_identifier_kind(k) {
        if let Ok(t) = node.utf8_text(bytes) {
            let s = t.trim();
            if !s.is_empty()
                && s.len() <= 80
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                && !is_swift_keyword(s)
            {
                // Normalize dotted names to internal `::` (helps edge tailing / aliasing).
                let norm = s.replace('.', "::");
                // Add both full and tail.
                out.insert(norm.clone());
                if let Some((_, tail)) = norm.rsplit_once("::") {
                    if !tail.is_empty() {
                        out.insert(tail.to_string());
                    }
                }
            }
        }
    }

    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        collect_swift_identifiers_rec(bytes, ch, _cursor, out);
        if out.len() >= 256 {
            return;
        }
    }
}

fn is_swift_keyword(s: &str) -> bool {
    // This is intentionally small and conservative.
    // We mostly want to avoid edges on ubiquitous syntax words.
    matches!(
        s,
        "import" | "func" | "var" | "let" | "struct" | "class" | "enum" | "protocol" | "extension" | "typealias" | "actor" |
        "public" | "open" | "internal" | "fileprivate" | "private" |
        "static" | "mutating" | "nonmutating" | "override" | "final" |
        "if" | "else" | "for" | "while" | "switch" | "case" | "default" |
        "break" | "continue" | "return" | "throw" | "throws" | "rethrows" |
        "do" | "catch" | "try" | "await" | "async" |
        "in" | "where" | "guard" |
        "self" | "Self" |
        "true" | "false" | "nil" |
        "some" | "any" |
        // SwiftUI is everywhere; keep module names via file-level refs rather than as general refs.
        "View" | "App" | "Scene"
    )
}

// -----------------------------------------------------------------------------
// Lexical fallback (last resort)
// -----------------------------------------------------------------------------

fn lexical_fallback_fragments(path: &Path, source: &str) -> Vec<Fragment> {
    // Very small, best-effort. We only capture top-level decl headers.
    // If this triggers, it means tree-sitter didn’t produce recognizable nodes.
    let mut out = Vec::new();
    let bytes = source.as_bytes();

    let mut offset = 0usize;
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let decl = ["struct ", "class ", "enum ", "protocol ", "extension ", "actor ", "func "];
        let mut found = None;
        for kw in decl {
            if trimmed.starts_with(kw)
                || trimmed.starts_with(&format!("public {kw}"))
                || trimmed.starts_with(&format!("open {kw}"))
                || trimmed.starts_with(&format!("internal {kw}"))
            {
                found = Some(kw.trim());
                break;
            }
        }

        if let Some(kw) = found {
            let name = trimmed
                .split_whitespace()
                .nth(if trimmed.starts_with(kw) { 1 } else { 2 })
                .unwrap_or("")
                .trim_matches('{')
                .trim();

            let kind = match kw {
                "struct" | "class" | "actor" => FragKind::Struct,
                "enum" => FragKind::Enum,
                "protocol" => FragKind::Trait,
                "extension" => FragKind::Impl,
                "func" => FragKind::Function,
                _ => FragKind::Other,
            };

            // Find span end: naive brace match.
            let start_byte = offset;
            let mut end_byte = offset + line.len();
            let mut brace = count_char(line, '{') as i64 - count_char(line, '}') as i64;
            if brace > 0 {
                let mut j = i + 1;
                let mut off2 = offset + line.len() + 1; // assume `\n`
                for l2 in source.lines().skip(i + 1) {
                    brace += count_char(l2, '{') as i64;
                    brace -= count_char(l2, '}') as i64;
                    end_byte = off2 + l2.len();
                    if brace <= 0 {
                        break;
                    }
                    off2 += l2.len() + 1;
                    j += 1;
                }
            }

            // Node is fake; build a Span using line index.
            let span = Span {
                start_byte,
                end_byte,
                start_line: i as u32,
                start_col: 0,
                end_line: i as u32,
                end_col: 0,
            };

            let body = source.get(start_byte..end_byte).unwrap_or("").to_string();
            let signature = trimmed.to_string();
            let doc = String::new();
            let content_hash = hash_text_hex(&normalize_whitespace(&body));
            let ast_hash = hash_text_hex(&format!("lex:{}:{}", path.display(), i));

            let retrieval_text = format!(
                "path: {}\nkind: {:?}\nsymbol: {}\n{}",
                path.display(),
                kind,
                name,
                signature
            );

            out.push(Fragment {
                id: content_hash,
                ast_hash,
                file: PathBuf::from(path),
                kind,
                symbol: if name.is_empty() { None } else { Some(name.to_string()) },
                span,
                signature,
                body,
                doc,
                retrieval_text,
                refs: Vec::new(),
            });
        }

        offset += line.len() + 1;
    }

    // If fallback found nothing, return empty.
    // (Index still functions; file-level ApiSummary will be skipped.)
    // Keep small.
    out.truncate(256);
    out
}

fn count_char(s: &str, c: char) -> usize {
    s.chars().filter(|&x| x == c).count()
}
