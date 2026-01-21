use anyhow::Result;
use ce_core::model::{FragKind, Fragment, Span};
use ce_core::util::{hash_text_hex, normalize_whitespace};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};

/// TypeScript / TSX (React) language adapter.
///
/// Design goals (v1):
/// - Deterministic, AST-first fragmentation using `tree-sitter-typescript`.
/// - Cover the majority of real-world frontend repos:
///   - exported functions
///   - component functions (named + `const Foo = () => ...`)
///   - classes + class methods
///   - interfaces, enums, type aliases
/// - Conservative identifier refs to power ref→def edges without exploding noise.
///
/// Notes:
/// - We model `class` as `FragKind::Struct` (close enough for retrieval).
/// - We model `interface` as `FragKind::Trait`.
/// - We treat arrow-function component declarations as `FragKind::Function`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsMode {
    TypeScript,
    Tsx,
}

pub struct TsReactAdapter {
    parser: Parser,
    mode: TsMode,
}

/// Collect file-level refs (imports/exports) from TS/TSX source.
///
/// These refs are attached to the file-level ApiSummary fragment at index time.
/// They improve retrieval and explainability (e.g. `react`, `supabase`, `createClient`).
///
/// Strategy:
/// - line-based scan for `import` / `export ... from` / `require()`
/// - extract imported identifiers + module specifier basename
///
/// This is intentionally lightweight (no regex; best-effort).
pub fn collect_file_level_refs(source: &str) -> Vec<String> {
    let mut out: HashSet<String> = HashSet::new();

    // We expect imports/exports near the top; cap scanning for speed.
    let mut meaningful_seen = 0usize;
    for raw in source.lines().take(800) {
        let mut line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // Very light comment stripping.
        if line.starts_with("//") {
            continue;
        }
        if let Some((before, _)) = line.split_once("//") {
            line = before.trim();
        }
        if line.is_empty() {
            continue;
        }

        let is_import = line.starts_with("import ") || line.starts_with("import\t") || line.starts_with("import{");
        let is_export_from = (line.starts_with("export ") || line.starts_with("export\t")) && line.contains(" from ");
        let is_require = line.contains("require(");

        if is_import || is_export_from {
            meaningful_seen += 1;
            // module specifier
            if let Some(spec) = extract_first_string_literal(line) {
                add_module_spec_refs(&spec, &mut out);
            }
            // imported identifiers
            extract_imported_identifiers(line, &mut out);
            continue;
        }

        if is_require {
            meaningful_seen += 1;
            if let Some(spec) = extract_first_string_literal(line) {
                add_module_spec_refs(&spec, &mut out);
            }
            continue;
        }

        // Stop once we've passed the likely import header section.
        // A few non-import lines are ok (e.g. 'use client', directives).
        if meaningful_seen > 0 {
            break;
        }
    }

    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    v.dedup();
    if v.len() > 128 {
        v.truncate(128);
    }
    v
}

fn add_module_spec_refs(spec: &str, out: &mut HashSet<String>) {
    let s = spec.trim();
    if s.is_empty() {
        return;
    }

    // Always include the full specifier (bounded).
    if s.len() <= 96 {
        out.insert(s.to_string());
    }

    // Also include the basename after the last '/' (helps for npm packages).
    let tail = s.rsplit('/').next().unwrap_or(s);
    let tail = tail.trim();
    if !tail.is_empty() && tail.len() <= 64 {
        out.insert(tail.to_string());
    }

    // For scoped pkgs `@scope/name`, include `name`.
    if let Some((_, name)) = s.rsplit_once('/') {
        let name = name.trim();
        if !name.is_empty() && name.len() <= 64 {
            out.insert(name.to_string());
        }
    }
}

fn extract_imported_identifiers(line: &str, out: &mut HashSet<String>) {
    // Examples:
    //   import React from 'react'
    //   import * as React from 'react'
    //   import { useState, useEffect as useEF } from 'react'
    //   import React, { useMemo } from 'react'
    //   export { Foo, Bar as Baz } from './x'
    //   export * from './x'

    let mut s = line.trim();

    // Remove leading `export` in `export { ... } from`.
    if s.starts_with("export ") {
        s = s.trim_start_matches("export").trim();
    }

    if !s.starts_with("import ") && !s.starts_with("{") {
        // e.g. `export * from` doesn't have named imports.
        // But it may still contain `* as`.
    }

    // Remove `import` keyword.
    if s.starts_with("import") {
        s = s.trim_start_matches("import").trim();
    }

    // Remove `type` keyword (`import type {Foo} from ...`).
    if s.starts_with("type ") {
        s = s.trim_start_matches("type").trim();
    }

    // Everything before `from` is the import clause.
    let clause = if let Some((before, _)) = s.split_once(" from ") {
        before.trim()
    } else {
        // Side-effect import: `import 'x'`.
        s.trim()
    };

    if clause.is_empty() {
        return;
    }

    // `* as React`
    if clause.starts_with("*") {
        if let Some((_, after_as)) = clause.split_once(" as ") {
            let name = after_as
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("")
                .trim();
            if !name.is_empty() {
                out.insert(name.to_string());
            }
        }
        return;
    }

    // Named imports: `{ ... }`
    if let Some(start) = clause.find('{') {
        if let Some(end) = clause.rfind('}') {
            if end > start {
                let inner = &clause[start + 1..end];
                for part in inner.split(',') {
                    let p = part.trim();
                    if p.is_empty() {
                        continue;
                    }
                    // `foo as bar`
                    if let Some((a, b)) = p.split_once(" as ") {
                        let a = a.trim();
                        let b = b.trim();
                        let a = a.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
                        let b = b.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
                        if !a.is_empty() {
                            out.insert(a.to_string());
                        }
                        if !b.is_empty() {
                            out.insert(b.to_string());
                        }
                    } else {
                        let a = p.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
                        if !a.is_empty() {
                            out.insert(a.to_string());
                        }
                    }
                }
            }
        }
    }

    // Default import: `React, { ... }` or `React`.
    // Take the token before a comma or whitespace.
    if !clause.starts_with('{') {
        let tok = clause
            .split(|c: char| c == ',' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim();
        if !tok.is_empty() && (tok.chars().next().unwrap_or('_').is_ascii_alphabetic() || tok.starts_with('_')) {
            // avoid capturing `type` etc
            if tok != "type" {
                out.insert(tok.to_string());
            }
        }
    }
}

fn extract_first_string_literal(s: &str) -> Option<String> {
    // Return the first '...' or "..." literal content on the line.
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\'' || c == '"' {
            let quote = c;
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == quote {
                    return Some(s[start..j].to_string());
                }
                // naive escape handling
                if cj == '\\' {
                    j += 1;
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

impl TsReactAdapter {
    pub fn new_ts() -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
        Ok(Self {
            parser,
            mode: TsMode::TypeScript,
        })
    }

    pub fn new_tsx() -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())?;
        Ok(Self {
            parser,
            mode: TsMode::Tsx,
        })
    }

    pub fn parse(&mut self, source: &str) -> Result<Tree> {
        self.parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter parse returned None"))
    }

    pub fn extract_fragments(&self, path: &Path, source: &str, tree: &Tree) -> Vec<Fragment> {
        let root = tree.root_node();
        let bytes = source.as_bytes();

        let mut out: Vec<Fragment> = Vec::new();
        let mut cursor = root.walk();

        for item in root.named_children(&mut cursor) {
            self.extract_from_top_level_node(path, bytes, item, &mut out);
        }

        out
    }

    fn extract_from_top_level_node(&self, path: &Path, bytes: &[u8], node: Node, out: &mut Vec<Fragment>) {
        let k = node.kind();

        // Handle `export ...` wrappers.
        if k == "export_statement" {
            let mut c = node.walk();
            if let Some(ch) = node.named_children(&mut c).next() {
                self.extract_from_top_level_node(path, bytes, ch, out);
            }
            return;
        }

        // Default exports can be declarations (easy) or expressions (e.g. `export default () => ...`).
        if k == "export_default_declaration" {
            let mut c = node.walk();
            if let Some(ch) = node.named_children(&mut c).next() {
                let inner_k = ch.kind();
                if classify_ts_decl(bytes, ch, path).is_some()
                    || matches!(inner_k, "lexical_declaration" | "variable_statement")
                {
                    self.extract_from_top_level_node(path, bytes, ch, out);
                    return;
                }
            }

            // Otherwise keep the whole `export default ...` as its own fragment.
            let sym = file_stem_symbol(path);
            if let Some(f) = build_fragment(path, bytes, node, FragKind::Function, sym) {
                out.push(f);
            }
            return;
        }

        // `export_clause` is part of re-exports like `export { Foo } from "./x"`.
        // Those are handled via file-level refs and the TS module graph.
        if k == "export_clause" {
            return;
        }

        // Ignore top-level imports.
        if matches!(k, "import_statement" | "import_declaration") {
            return;
        }

        // Direct declarations.
        if let Some((kind, sym)) = classify_ts_decl(bytes, node, path) {
            if let Some(f) = build_fragment(path, bytes, node, kind, sym.clone()) {
                out.push(f.clone());

                // If it's a class, collect methods.
                if kind == FragKind::Struct {
                    let class_name = sym.as_deref().unwrap_or("Class");
                    self.collect_class_methods(path, bytes, node, class_name, out);
                }
            }
            return;
        }

        // Variable declarations (including arrow-function components).
        if matches!(k, "lexical_declaration" | "variable_statement") {
            self.extract_from_variable_statement(path, bytes, node, out);
            return;
        }

        // Some files wrap top-level declarations in statements (rare). Recurse a bit.
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            // Avoid diving into function bodies (huge).
            if is_large_body_container(ch.kind()) {
                continue;
            }
            self.extract_from_top_level_node(path, bytes, ch, out);
        }
    }

    fn extract_from_variable_statement(&self, path: &Path, bytes: &[u8], node: Node, out: &mut Vec<Fragment>) {
        // Find all variable_declarator nodes.
        let mut cursor = node.walk();
        for ch in node.named_children(&mut cursor) {
            self.collect_var_declarators(path, bytes, ch, out);
        }
    }

    fn collect_var_declarators(&self, path: &Path, bytes: &[u8], node: Node, out: &mut Vec<Fragment>) {
        let k = node.kind();
        if k == "variable_declarator" {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(bytes).ok())
                .map(|s| s.to_string());

            let init_kind = node
                .child_by_field_name("value")
                .map(|n| n.kind().to_string())
                .unwrap_or_default();

            let mut frag_kind = FragKind::Const;
            // Arrow function / function expression treated as Function.
            if init_kind.contains("arrow_function") || init_kind.contains("function") {
                frag_kind = FragKind::Function;
            }

            if let Some(f) = build_fragment(path, bytes, node, frag_kind, name) {
                out.push(f);
            }
            return;
        }

        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            // Avoid diving into function bodies.
            if is_large_body_container(ch.kind()) {
                continue;
            }
            self.collect_var_declarators(path, bytes, ch, out);
        }
    }

    fn collect_class_methods(&self, path: &Path, bytes: &[u8], class_node: Node, class_name: &str, out: &mut Vec<Fragment>) {
        // class body can be found via field `body`.
        let Some(body) = class_node.child_by_field_name("body") else { return; };
        let mut cursor = body.walk();
        for ch in body.named_children(&mut cursor) {
            self.collect_class_methods_rec(path, bytes, ch, class_name, /*inside_fn*/ false, out);
        }
    }

    fn collect_class_methods_rec(
        &self,
        path: &Path,
        bytes: &[u8],
        node: Node,
        class_name: &str,
        inside_fn: bool,
        out: &mut Vec<Fragment>,
    ) {
        let k = node.kind();

        let entering_fn = matches!(k, "function" | "arrow_function" | "function_declaration" | "method_definition");
        let now_inside = inside_fn || entering_fn;

        if !inside_fn {
            if k == "method_definition" {
                let mname = node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(bytes).ok())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "method".to_string());
                let sym = Some(format!("{}::{}", class_name, mname));
                if let Some(f) = build_fragment(path, bytes, node, FragKind::Method, sym) {
                    out.push(f);
                }
                return;
            }
        }

        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            if is_large_body_container(ch.kind()) && now_inside {
                continue;
            }
            self.collect_class_methods_rec(path, bytes, ch, class_name, now_inside, out);
        }
    }
}

fn is_large_body_container(kind: &str) -> bool {
    // Avoid diving into heavy body containers when doing top-level extraction.
    matches!(
        kind,
        "statement_block" | "statement_block" | "object" | "object_pattern" | "array" | "array_pattern" | "template_string" | "jsx_element" | "jsx_fragment"
    )
}

// -----------------------------------------------------------------------------
// Fragment construction
// -----------------------------------------------------------------------------

fn build_fragment(path: &Path, bytes: &[u8], node: Node, kind: FragKind, symbol: Option<String>) -> Option<Fragment> {
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

    // Extract doc-ish lines from the preamble.
    let doc = preamble
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("/**") || t.starts_with('*') || t.starts_with("///") || t.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content_hash = hash_text_hex(&normalize_whitespace(&body));
    let ast_hash = hash_text_hex(&node.to_sexp());

    // Collect referenced identifiers.
    let mut refs = collect_ts_identifiers(bytes, node);

    // Filter obvious junk/self references.
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
    let mut acc = String::new();
    for line in body.lines().take(14) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(t);
        if acc.contains('{') || acc.contains("=>") || acc.ends_with(';') {
            break;
        }
        // stop if the signature is already long
        if acc.len() > 320 {
            break;
        }
    }

    if acc.is_empty() {
        acc = body.lines().next().unwrap_or("").trim().to_string();
    }

    if acc.len() > 360 {
        format!("{}…", acc.chars().take(360).collect::<String>())
    } else {
        acc
    }
}

fn extract_preamble_from_source(bytes: &[u8], start_byte: usize) -> String {
    // Pull up to N lines before start_byte.
    let prefix = &bytes[..start_byte.min(bytes.len())];
    let text = String::from_utf8_lossy(prefix);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut out: Vec<&str> = Vec::new();
    let take = 12usize;
    for raw in lines.iter().rev().take(take) {
        let l = raw.trim_end();
        let t = l.trim_start();
        if t.is_empty() {
            continue;
        }
        let keep = t.starts_with("/**")
            || t.starts_with("/*")
            || t.starts_with("*")
            || t.starts_with("//")
            || t.starts_with('@');
        if keep {
            out.push(l);
        } else {
            break;
        }
    }

    out.reverse();
    out.join("\n")
}

// -----------------------------------------------------------------------------
// TS declaration classification
// -----------------------------------------------------------------------------

fn classify_ts_decl(bytes: &[u8], node: Node, path: &Path) -> Option<(FragKind, Option<String>)> {
    let k = node.kind();
    match k {
        "function_declaration" => Some((FragKind::Function, ts_decl_name(bytes, node))),
        "class_declaration" => Some((FragKind::Struct, ts_decl_name(bytes, node))),
        "interface_declaration" => Some((FragKind::Trait, ts_decl_name(bytes, node))),
        "type_alias_declaration" => Some((FragKind::TypeAlias, ts_decl_name(bytes, node))),
        "enum_declaration" => Some((FragKind::Enum, ts_decl_name(bytes, node))),
        "namespace_declaration" | "internal_module" | "module_declaration" => Some((FragKind::Mod, ts_decl_name(bytes, node))),
        "export_default_declaration" => {
            // Default exports often omit a name (`export default () => ...`).
            // Use the file stem as a fallback symbol.
            let sym = ts_decl_name(bytes, node).or_else(|| file_stem_symbol(path));
            Some((FragKind::Other, sym))
        }
        _ => None,
    }
}

fn file_stem_symbol(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
}

fn ts_decl_name(bytes: &[u8], node: Node) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(bytes).ok())
        .map(|s| s.to_string())
}

// -----------------------------------------------------------------------------
// Identifier collection
// -----------------------------------------------------------------------------

fn collect_ts_identifiers(bytes: &[u8], node: Node) -> Vec<String> {
    let mut out: HashSet<String> = HashSet::new();
    let mut stack: Vec<Node> = vec![node];

    while let Some(n) = stack.pop() {
        if n.is_named() {
            let k = n.kind();
            if k.contains("identifier") {
                if let Ok(t) = n.utf8_text(bytes) {
                    let s = t.trim();
                    if is_reasonable_ident(s) {
                        out.insert(s.to_string());
                    }
                }
            }
        }

        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            stack.push(ch);
        }
    }

    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    if v.len() > 128 {
        v.truncate(128);
    }
    v
}

fn is_reasonable_ident(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.len() < 2 || s.len() > 80 {
        return false;
    }
    // avoid numeric-only
    if s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // avoid weird whitespace
    if s.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    true
}
