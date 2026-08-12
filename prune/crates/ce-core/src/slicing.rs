use crate::model::{AstSlicePolicy, BodyIncludePolicy};
use std::collections::BTreeSet;

/// Input shared by every language pack's AST slicer.
#[derive(Debug, Clone)]
pub struct AstSliceRequest<'a> {
    pub source: &'a str,
    pub fragment_start_line: u32,
    pub target_lines: &'a [u32],
    pub focus_symbols: &'a [String],
    pub policy: &'a AstSlicePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstSlice {
    pub text: String,
    pub included_nodes: usize,
    pub omitted_nodes: usize,
    pub reasons: Vec<String>,
}

/// Uniform contract implemented by each tree-sitter language pack.
pub trait LanguageAstSlicer {
    fn language_id(&self) -> &'static str;
    fn slice_ast(&mut self, request: AstSliceRequest<'_>) -> anyhow::Result<Option<AstSlice>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstNodeRole {
    Declaration,
    Documentation,
    TypeDeclaration,
    CallSite,
    Branch,
    Body,
}

#[derive(Debug, Clone)]
pub struct AstNodeSpan {
    pub start_line: usize,
    pub end_line: usize,
    pub depth: usize,
    pub role: AstNodeRole,
    pub relevance_text: String,
}

/// Policy engine shared by all language adapters after they classify their
/// grammar-specific syntax nodes into stable semantic roles.
pub fn select_ast_nodes(request: &AstSliceRequest<'_>, nodes: &[AstNodeSpan]) -> Option<AstSlice> {
    let source_lines: Vec<&str> = request.source.lines().collect();
    if source_lines.is_empty() {
        return None;
    }
    let focus: Vec<String> = request
        .focus_symbols
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let target_lines: BTreeSet<usize> = request
        .target_lines
        .iter()
        .filter_map(|line| line.checked_sub(request.fragment_start_line + 1))
        .map(|line| line as usize)
        .collect();
    let mut ranked: Vec<(usize, i32, &AstNodeSpan, String)> = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.depth > request.policy.max_depth {
            continue;
        }
        let structural = match node.role {
            AstNodeRole::Declaration => request.policy.include_public_api,
            AstNodeRole::Documentation => request.policy.include_doc_comments,
            AstNodeRole::TypeDeclaration => request.policy.include_type_declarations,
            _ => false,
        };
        let line_hit = target_lines
            .range(node.start_line..=node.end_line)
            .next()
            .is_some();
        let lowered = node.relevance_text.to_ascii_lowercase();
        let symbol_hit = focus
            .iter()
            .any(|symbol| symbol.len() >= 2 && lowered.contains(symbol));
        let relevant_role = matches!(node.role, AstNodeRole::CallSite)
            && request.policy.include_call_sites
            || matches!(node.role, AstNodeRole::Branch) && request.policy.include_relevant_branches;
        let relevant = line_hit || symbol_hit;
        let include = structural
            || match request.policy.body_policy {
                BodyIncludePolicy::None => false,
                BodyIncludePolicy::ReferencedOnly => {
                    (relevant && relevant_role)
                        || (line_hit && matches!(node.role, AstNodeRole::Body))
                }
                BodyIncludePolicy::TopKRelevant => relevant || relevant_role,
            };
        if include {
            let score = if line_hit {
                100
            } else if symbol_hit {
                50
            } else if structural {
                20
            } else {
                1
            };
            let reason = if line_hit {
                "error-span"
            } else if symbol_hit {
                "referenced-symbol"
            } else {
                "structural"
            };
            ranked.push((index, score, node, reason.to_string()));
        }
    }
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let limit = match request.policy.body_policy {
        BodyIncludePolicy::TopKRelevant => request.policy.top_k_blocks.max(1),
        _ => request.policy.max_nodes.max(1),
    };
    ranked.truncate(limit.min(request.policy.max_nodes.max(1)));
    if ranked.is_empty() {
        return None;
    }
    ranked.sort_by_key(|(_, _, node, _)| node.start_line);
    let mut selected_lines = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    for (_, _, node, reason) in &ranked {
        reasons.insert(reason.clone());
        for line in node.start_line..=node.end_line.min(source_lines.len().saturating_sub(1)) {
            selected_lines.insert(line);
        }
    }
    let mut text = String::new();
    let mut previous = None;
    for line in selected_lines {
        if previous.is_some_and(|last| line > last + 1) {
            text.push_str("...\n");
        }
        text.push_str(source_lines[line]);
        text.push('\n');
        previous = Some(line);
    }
    Some(AstSlice {
        text,
        included_nodes: ranked.len(),
        omitted_nodes: nodes.len().saturating_sub(ranked.len()),
        reasons: reasons.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referenced_policy_keeps_structure_and_matching_branch() {
        let policy = AstSlicePolicy::default();
        let focus = vec!["needle".to_string()];
        let request = AstSliceRequest {
            source: "fn run() {\n  if needle {\n    work();\n  }\n}\n",
            fragment_start_line: 0,
            target_lines: &[],
            focus_symbols: &focus,
            policy: &policy,
        };
        let nodes = vec![
            AstNodeSpan {
                start_line: 0,
                end_line: 0,
                depth: 0,
                role: AstNodeRole::Declaration,
                relevance_text: "run".into(),
            },
            AstNodeSpan {
                start_line: 1,
                end_line: 3,
                depth: 1,
                role: AstNodeRole::Branch,
                relevance_text: "if needle work".into(),
            },
        ];
        let slice = select_ast_nodes(&request, &nodes).expect("slice");
        assert!(slice.text.contains("fn run"));
        assert!(slice.text.contains("if needle"));
        assert!(slice.reasons.contains(&"referenced-symbol".to_string()));
    }
}
