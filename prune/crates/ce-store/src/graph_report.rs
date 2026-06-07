use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;

#[derive(Debug, Clone, Copy)]
pub struct GraphReportOptions {
    pub max_hubs: usize,
    pub max_edges: usize,
}

impl Default for GraphReportOptions {
    fn default() -> Self {
        Self {
            max_hubs: 10,
            max_edges: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphReport {
    pub fragment_count: usize,
    pub edge_count: usize,
    pub edge_types: Vec<EdgeTypeSummary>,
    pub top_hubs: Vec<GraphHubSummary>,
    pub strong_edges: Vec<GraphEdgeSummary>,
    pub suggested_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeSummary {
    pub edge_type: String,
    pub count: usize,
    pub avg_weight: f32,
    pub total_weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphHubSummary {
    pub rowid: i64,
    pub path: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub incoming_edges: usize,
    pub outgoing_edges: usize,
    pub weighted_degree: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeSummary {
    pub from_rowid: i64,
    pub from_path: String,
    pub from_kind: String,
    pub from_symbol: Option<String>,
    pub to_rowid: i64,
    pub to_path: String,
    pub to_kind: String,
    pub to_symbol: Option<String>,
    pub edge_type: String,
    pub weight: f32,
}

impl Db {
    pub fn graph_report(&self, options: GraphReportOptions) -> Result<GraphReport> {
        let conn = self.conn();

        let fragment_count = conn.query_row("SELECT COUNT(*) FROM fragments", [], |row| {
            row.get::<_, i64>(0)
        })? as usize;
        let edge_count =
            conn.query_row("SELECT COUNT(*) FROM edges", [], |row| row.get::<_, i64>(0))? as usize;

        let mut edge_type_stmt = conn.prepare(
            r#"
            SELECT edge_type, COUNT(*) AS edge_count, AVG(weight), SUM(weight)
            FROM edges
            GROUP BY edge_type
            ORDER BY edge_count DESC, edge_type ASC
            "#,
        )?;
        let edge_type_rows = edge_type_stmt.query_map([], |row| {
            Ok(EdgeTypeSummary {
                edge_type: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
                avg_weight: row.get::<_, f64>(2)? as f32,
                total_weight: row.get::<_, f64>(3)? as f32,
            })
        })?;
        let mut edge_types = Vec::new();
        for row in edge_type_rows {
            edge_types.push(row?);
        }

        let top_hubs = if options.max_hubs == 0 {
            Vec::new()
        } else {
            let mut hub_stmt = conn.prepare(
                r#"
                SELECT
                    f.rowid,
                    f.path,
                    f.kind,
                    f.symbol,
                    COALESCE(in_edges.edge_count, 0) AS incoming_edges,
                    COALESCE(out_edges.edge_count, 0) AS outgoing_edges,
                    COALESCE(in_edges.weight_sum, 0.0) + COALESCE(out_edges.weight_sum, 0.0) AS weighted_degree
                FROM fragments f
                LEFT JOIN (
                    SELECT to_rowid AS rowid, COUNT(*) AS edge_count, SUM(weight) AS weight_sum
                    FROM edges
                    GROUP BY to_rowid
                ) in_edges ON in_edges.rowid = f.rowid
                LEFT JOIN (
                    SELECT from_rowid AS rowid, COUNT(*) AS edge_count, SUM(weight) AS weight_sum
                    FROM edges
                    GROUP BY from_rowid
                ) out_edges ON out_edges.rowid = f.rowid
                WHERE COALESCE(in_edges.edge_count, 0) + COALESCE(out_edges.edge_count, 0) > 0
                ORDER BY
                    (COALESCE(in_edges.edge_count, 0) + COALESCE(out_edges.edge_count, 0)) DESC,
                    weighted_degree DESC,
                    f.path ASC,
                    COALESCE(f.symbol, '') ASC,
                    f.rowid ASC
                LIMIT ?1
                "#,
            )?;
            let hub_rows = hub_stmt.query_map(params![options.max_hubs as i64], |row| {
                Ok(GraphHubSummary {
                    rowid: row.get(0)?,
                    path: row.get(1)?,
                    kind: row.get(2)?,
                    symbol: row.get(3)?,
                    incoming_edges: row.get::<_, i64>(4)? as usize,
                    outgoing_edges: row.get::<_, i64>(5)? as usize,
                    weighted_degree: row.get::<_, f64>(6)? as f32,
                })
            })?;
            let mut hubs = Vec::new();
            for row in hub_rows {
                hubs.push(row?);
            }
            hubs
        };

        let strong_edges = if options.max_edges == 0 {
            Vec::new()
        } else {
            let mut edge_stmt = conn.prepare(
                r#"
                SELECT
                    e.from_rowid,
                    from_frag.path,
                    from_frag.kind,
                    from_frag.symbol,
                    e.to_rowid,
                    to_frag.path,
                    to_frag.kind,
                    to_frag.symbol,
                    e.edge_type,
                    e.weight
                FROM edges e
                JOIN fragments from_frag ON from_frag.rowid = e.from_rowid
                JOIN fragments to_frag ON to_frag.rowid = e.to_rowid
                WHERE from_frag.path <> to_frag.path
                ORDER BY e.weight DESC, e.edge_type ASC, from_frag.path ASC, to_frag.path ASC
                LIMIT ?1
                "#,
            )?;
            let edge_rows = edge_stmt.query_map(params![options.max_edges as i64], |row| {
                Ok(GraphEdgeSummary {
                    from_rowid: row.get(0)?,
                    from_path: row.get(1)?,
                    from_kind: row.get(2)?,
                    from_symbol: row.get(3)?,
                    to_rowid: row.get(4)?,
                    to_path: row.get(5)?,
                    to_kind: row.get(6)?,
                    to_symbol: row.get(7)?,
                    edge_type: row.get(8)?,
                    weight: row.get::<_, f64>(9)? as f32,
                })
            })?;
            let mut edges = Vec::new();
            for row in edge_rows {
                edges.push(row?);
            }
            edges
        };

        let suggested_questions = suggested_questions(&top_hubs, &edge_types, &strong_edges);

        Ok(GraphReport {
            fragment_count,
            edge_count,
            edge_types,
            top_hubs,
            strong_edges,
            suggested_questions,
        })
    }
}

impl GraphReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Prune Graph Report\n\n");
        out.push_str("## Summary\n\n");
        out.push_str(&format!("- Fragments indexed: {}\n", self.fragment_count));
        out.push_str(&format!("- Resolved edges: {}\n", self.edge_count));

        if self.edge_count == 0 {
            out.push_str(
                "\nNo resolved edges are present. Re-run `ce index` without `--skip-edges` to enable graph expansion and explainability.\n",
            );
            return out;
        }

        out.push_str("\n## Edge Types\n\n");
        out.push_str("| Type | Edges | Avg Weight | Total Weight |\n");
        out.push_str("|---|---:|---:|---:|\n");
        for edge_type in &self.edge_types {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                md_cell(&edge_type.edge_type),
                edge_type.count,
                fmt_weight(edge_type.avg_weight),
                fmt_weight(edge_type.total_weight)
            ));
        }

        out.push_str("\n## Top Hubs\n\n");
        if self.top_hubs.is_empty() {
            out.push_str("No connected fragments found.\n");
        } else {
            out.push_str("| Rank | Fragment | Edges | In | Out | Weighted Degree |\n");
            out.push_str("|---:|---|---:|---:|---:|---:|\n");
            for (idx, hub) in self.top_hubs.iter().enumerate() {
                let edge_total = hub.incoming_edges + hub.outgoing_edges;
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    idx + 1,
                    md_cell(&node_label(hub.symbol.as_deref(), &hub.kind, &hub.path)),
                    edge_total,
                    hub.incoming_edges,
                    hub.outgoing_edges,
                    fmt_weight(hub.weighted_degree)
                ));
            }
        }

        out.push_str("\n## Strong Cross-File Relationships\n\n");
        if self.strong_edges.is_empty() {
            out.push_str("No cross-file edges found in the current graph.\n");
        } else {
            out.push_str("| Rank | From | Relation | To | Weight |\n");
            out.push_str("|---:|---|---|---|---:|\n");
            for (idx, edge) in self.strong_edges.iter().enumerate() {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    idx + 1,
                    md_cell(&node_label(
                        edge.from_symbol.as_deref(),
                        &edge.from_kind,
                        &edge.from_path
                    )),
                    md_cell(&edge.edge_type),
                    md_cell(&node_label(
                        edge.to_symbol.as_deref(),
                        &edge.to_kind,
                        &edge.to_path
                    )),
                    fmt_weight(edge.weight)
                ));
            }
        }

        out.push_str("\n## Suggested Questions\n\n");
        for question in &self.suggested_questions {
            out.push_str(&format!("- {}\n", question));
        }

        out
    }
}

fn suggested_questions(
    hubs: &[GraphHubSummary],
    edge_types: &[EdgeTypeSummary],
    edges: &[GraphEdgeSummary],
) -> Vec<String> {
    if hubs.is_empty() {
        return vec![
            "Which files need edge rebuilding before graph expansion is useful?".to_string(),
            "Which source languages should contribute the next resolved edge type?".to_string(),
        ];
    }

    let top_hub = &hubs[0];
    let top_hub_label = node_label(top_hub.symbol.as_deref(), &top_hub.kind, &top_hub.path);
    let mut questions = vec![
        format!("Why is `{top_hub_label}` the most connected fragment?"),
        format!("Which callers or imports should be reviewed before changing `{top_hub_label}`?"),
    ];

    if let Some(edge_type) = edge_types.first() {
        questions.push(format!(
            "What does the dominant `{}` edge type say about this repo's coupling?",
            edge_type.edge_type
        ));
    }

    if let Some(edge) = edges.first() {
        questions.push(format!(
            "Should the `{}` relationship from `{}` to `{}` become an explicit test or doc note?",
            edge.edge_type,
            node_label(
                edge.from_symbol.as_deref(),
                &edge.from_kind,
                &edge.from_path
            ),
            node_label(edge.to_symbol.as_deref(), &edge.to_kind, &edge.to_path)
        ));
    }

    questions
}

fn node_label(symbol: Option<&str>, kind: &str, path: &str) -> String {
    match symbol {
        Some(symbol) if !symbol.is_empty() => format!("{symbol} ({path})"),
        _ => format!("{kind} ({path})"),
    }
}

fn fmt_weight(weight: f32) -> String {
    format!("{weight:.2}")
}

fn md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
