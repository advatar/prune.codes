use crate::model::{ContextPack, Span, StrategyConfig};
use crate::pack::{pack_with_strategy, Candidate, CandidateNeighbor};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGraphStratum {
    LpmExact,
    SourceApproximate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceNodeKind {
    Declaration,
    Parameter,
    Statement,
    Expression,
    Call,
    Branch,
    Return,
    Projection,
    Compute,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceEdgeKind {
    Contains,
    Defines,
    Reads,
    Writes,
    Calls,
    ReturnsTo,
    Controls,
    DataDependsOn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceNode {
    pub id: String,
    pub owner_fragment_id: String,
    pub kind: EvidenceNodeKind,
    pub span: Option<Span>,
    #[serde(default)]
    pub retrieval_text: String,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub estimated_tokens: usize,
    /// Exact trace facts, not an execution-accuracy-derived proxy.
    #[serde(default)]
    pub trace_touched: bool,
    #[serde(default)]
    pub causally_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceEdge {
    pub source: String,
    pub target: String,
    pub kind: EvidenceEdgeKind,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceGraph {
    pub graph_id: String,
    pub stratum: EvidenceGraphStratum,
    #[serde(default)]
    pub nodes: Vec<EvidenceNode>,
    #[serde(default)]
    pub edges: Vec<EvidenceEdge>,
}

impl EvidenceGraph {
    pub fn validate(&self) -> Result<()> {
        let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        if ids.len() != self.nodes.len() {
            return Err(anyhow!("duplicate evidence node id"));
        }
        for edge in &self.edges {
            if !ids.contains(edge.source.as_str()) || !ids.contains(edge.target.as_str()) {
                return Err(anyhow!(
                    "edge {} -> {} references an unknown node",
                    edge.source,
                    edge.target
                ));
            }
        }
        Ok(())
    }

    /// Canonical LPM ingestion format is JSONL with one header followed by node/edge records.
    pub fn from_lpm_jsonl(input: &str) -> Result<Self> {
        let mut graph_id = None;
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for (line_no, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: LpmRecord = serde_json::from_str(line)
                .with_context(|| format!("invalid LPM JSONL record at line {}", line_no + 1))?;
            match record {
                LpmRecord::Graph { graph_id: id } => graph_id = Some(id),
                LpmRecord::Node { node } => nodes.push(node),
                LpmRecord::Edge { edge } => edges.push(edge),
            }
        }
        let graph = Self {
            graph_id: graph_id.ok_or_else(|| anyhow!("missing graph header"))?,
            stratum: EvidenceGraphStratum::LpmExact,
            nodes,
            edges,
        };
        graph.validate()?;
        Ok(graph)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum LpmRecord {
    Graph {
        graph_id: String,
    },
    Node {
        #[serde(flatten)]
        node: EvidenceNode,
    },
    Edge {
        #[serde(flatten)]
        edge: EvidenceEdge,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePack {
    pub graph_id: String,
    pub stratum: EvidenceGraphStratum,
    pub selected_evidence_node_ids: Vec<String>,
    pub rendered_owner_fragment_ids: Vec<String>,
    pub context: ContextPack,
}

/// Select fine-grained evidence while rendering each owning declaration at most once.
pub fn pack_evidence_with_strategy(
    strategy: &StrategyConfig,
    graph: &EvidenceGraph,
    owners: Vec<Candidate>,
) -> Result<EvidencePack> {
    graph.validate()?;
    let mut ranked_nodes: Vec<&EvidenceNode> = graph.nodes.iter().collect();
    ranked_nodes.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    let evidence_budget = strategy.budget_tokens.unwrap_or(usize::MAX);
    let mut evidence_tokens = 0usize;
    let selected_nodes: Vec<&EvidenceNode> = ranked_nodes
        .into_iter()
        .filter(|node| node.score > 0.0)
        .filter(|node| {
            let cost = node.estimated_tokens.max(1);
            if evidence_tokens.saturating_add(cost) > evidence_budget {
                false
            } else {
                evidence_tokens += cost;
                true
            }
        })
        .collect();
    let node_by_id: HashMap<&str, &EvidenceNode> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut owner_scores: HashMap<&str, f32> = HashMap::new();
    let mut owner_neighbors: HashMap<&str, HashMap<&str, f32>> = HashMap::new();
    for node in &selected_nodes {
        owner_scores
            .entry(&node.owner_fragment_id)
            .and_modify(|v| *v = v.max(node.score))
            .or_insert(node.score);
    }
    for edge in &graph.edges {
        let source = node_by_id[edge.source.as_str()].owner_fragment_id.as_str();
        let target = node_by_id[edge.target.as_str()].owner_fragment_id.as_str();
        if source != target {
            owner_neighbors
                .entry(source)
                .or_default()
                .entry(target)
                .and_modify(|v| *v = v.max(edge.confidence))
                .or_insert(edge.confidence);
        }
    }
    let owners: Vec<Candidate> = owners
        .into_iter()
        .filter_map(|mut owner| {
            let score = owner_scores.get(owner.id.as_str())?;
            owner.score = *score;
            owner.reason = format!("evidence:{}", graph.graph_id);
            owner.neighbors = owner_neighbors
                .remove(owner.id.as_str())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, weight)| CandidateNeighbor {
                    id: id.to_string(),
                    weight,
                })
                .collect();
            Some(owner)
        })
        .collect();
    let context = pack_with_strategy(strategy, owners);
    let rendered: HashSet<&str> = context.items.iter().map(|item| item.id.as_str()).collect();
    let mut selected: Vec<String> = selected_nodes
        .into_iter()
        .filter(|node| rendered.contains(node.owner_fragment_id.as_str()))
        .map(|node| node.id.clone())
        .collect();
    selected.sort();
    let mut rendered_owner_fragment_ids: Vec<String> =
        rendered.into_iter().map(str::to_string).collect();
    rendered_owner_fragment_ids.sort();
    Ok(EvidencePack {
        graph_id: graph.graph_id.clone(),
        stratum: graph.stratum,
        selected_evidence_node_ids: selected,
        rendered_owner_fragment_ids,
        context,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FragKind, FragmentView};

    fn owner(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            rowid: 1,
            path: "src/lib.rs".into(),
            kind: FragKind::Function,
            symbol: Some(id.into()),
            span: Span {
                start_byte: 0,
                end_byte: 20,
                start_line: 0,
                start_col: 0,
                end_line: 1,
                end_col: 0,
            },
            score: 0.0,
            reason: String::new(),
            signature: format!("fn {id}();"),
            body: format!("fn {id}() {{ work(); }}"),
            required_symbols: vec![],
            neighbors: vec![],
            body_view: FragmentView::Slice,
        }
    }

    #[test]
    fn regions_are_selected_but_owner_is_rendered_once() {
        let graph = EvidenceGraph {
            graph_id: "lpm-1".into(),
            stratum: EvidenceGraphStratum::LpmExact,
            nodes: vec![
                EvidenceNode {
                    id: "compute-1".into(),
                    owner_fragment_id: "run".into(),
                    kind: EvidenceNodeKind::Compute,
                    span: None,
                    retrieval_text: "add".into(),
                    score: 2.0,
                    estimated_tokens: 2,
                    trace_touched: true,
                    causally_required: true,
                },
                EvidenceNode {
                    id: "return-1".into(),
                    owner_fragment_id: "run".into(),
                    kind: EvidenceNodeKind::Return,
                    span: None,
                    retrieval_text: "return".into(),
                    score: 1.0,
                    estimated_tokens: 1,
                    trace_touched: true,
                    causally_required: true,
                },
            ],
            edges: vec![EvidenceEdge {
                source: "compute-1".into(),
                target: "return-1".into(),
                kind: EvidenceEdgeKind::DataDependsOn,
                confidence: 1.0,
            }],
        };
        let pack =
            pack_evidence_with_strategy(&StrategyConfig::default(), &graph, vec![owner("run")])
                .unwrap();
        assert_eq!(
            pack.selected_evidence_node_ids,
            vec!["compute-1", "return-1"]
        );
        assert_eq!(pack.rendered_owner_fragment_ids, vec!["run"]);
        assert_eq!(
            pack.context
                .items
                .iter()
                .filter(|item| item.id == "run")
                .count(),
            1
        );
    }

    #[test]
    fn lpm_jsonl_sets_exact_stratum_and_trace_truth() {
        let input = r#"{"record":"graph","graph_id":"g"}
{"record":"node","id":"n","owner_fragment_id":"f","kind":"compute","span":null,"score":1.0,"trace_touched":true,"causally_required":true}"#;
        let graph = EvidenceGraph::from_lpm_jsonl(input).unwrap();
        assert_eq!(graph.stratum, EvidenceGraphStratum::LpmExact);
        assert!(graph.nodes[0].causally_required);
    }
}
