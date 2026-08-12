use anyhow::Result;
use ce_core::model::{FragKind, Fragment, Span};
use ce_store::{Db, GraphReportOptions};
use rusqlite::params;
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::TempDir;

fn temp_db() -> Result<(TempDir, Db)> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("index.sqlite");
    let db = Db::open(&db_path)?;
    Ok((dir, db))
}

fn sample_fragment(
    path: &str,
    id: &str,
    symbol: Option<&str>,
    retrieval: &str,
    start_line: u32,
    refs: Vec<String>,
) -> Fragment {
    Fragment {
        id: id.to_string(),
        ast_hash: format!("ast-{id}"),
        file: PathBuf::from(path),
        kind: FragKind::Function,
        symbol: symbol.map(|s| s.to_string()),
        span: Span {
            start_byte: 0,
            end_byte: 24,
            start_line,
            start_col: 0,
            end_line: start_line + 1,
            end_col: 0,
        },
        signature: "fn sample()".to_string(),
        body: "fn sample() { println!(\"hi\"); }".to_string(),
        doc: String::new(),
        retrieval_text: retrieval.to_string(),
        refs,
    }
}

#[test]
fn upsert_and_query_fragments() -> Result<()> {
    let (_dir, db) = temp_db()?;
    let file_id = db.upsert_file("src/foo.rs", "rust", 24, 123, "hash-foo")?;

    let frag = sample_fragment(
        "src/foo.rs",
        "frag-foo",
        Some("Foo"),
        "helper widget",
        0,
        vec![],
    );
    let rowid = db.upsert_fragment(file_id, &frag)?;

    let got = db.get_fragment_by_id(&frag.id)?.expect("expected fragment");
    assert_eq!(got.0, rowid);
    assert_eq!(got.1.symbol.as_deref(), Some("Foo"));

    let covering = db.fragment_rowids_covering_line("src/foo.rs", 1, 5)?;
    assert!(covering.contains(&rowid));

    let by_path = db.fragment_rowids_for_path("foo.rs", 5)?;
    assert!(by_path.contains(&rowid));

    let hits = db.search_fts("helper", 5)?;
    assert!(hits.iter().any(|(rid, _)| *rid == rowid));

    Ok(())
}

#[test]
fn symbols_and_refs_roundtrip() -> Result<()> {
    let (_dir, db) = temp_db()?;

    let def_file_id = db.upsert_file("src/foo.rs", "rust", 40, 456, "hash-def")?;
    let def_frag = sample_fragment(
        "src/foo.rs",
        "frag-def",
        Some("Widget"),
        "widget struct",
        0,
        vec![],
    );
    let def_rowid = db.upsert_fragment(def_file_id, &def_frag)?;
    db.replace_symbols_for_fragment(def_rowid, &def_frag)?;

    let ref_file_id = db.upsert_file("src/bar.rs", "rust", 40, 789, "hash-ref")?;
    let ref_frag = sample_fragment(
        "src/bar.rs",
        "frag-ref",
        None,
        "uses Widget",
        5,
        vec!["Widget".to_string()],
    );
    let ref_rowid = db.upsert_fragment(ref_file_id, &ref_frag)?;
    db.replace_refs_for_fragment(ref_rowid, &ref_frag.refs)?;

    let defs = db.resolve_symbol_defs("Widget", 5)?;
    assert!(defs.contains(&def_rowid));

    let refs = db.refs_for_fragment(ref_rowid, 5)?;
    assert!(refs.iter().any(|r| r == "Widget"));

    let _ = db.rebuild_ref_edges_all(5, 5)?;
    let edges = db.edges_outgoing(ref_rowid, 10)?;
    assert!(edges
        .iter()
        .any(|(to, edge_type, _)| *to == def_rowid && edge_type == "refers"));

    let mut stmt = db
        .conn()
        .prepare("SELECT symbol FROM symbols WHERE frag_rowid=?1")?;
    let rows = stmt.query_map(params![def_rowid], |r| r.get::<_, String>(0))?;
    let mut symbols = HashSet::new();
    for rr in rows {
        symbols.insert(rr?);
    }
    assert!(symbols.contains("Widget"));
    assert!(symbols.contains("foo::Widget"));
    assert!(symbols.contains("crate::foo::Widget"));

    Ok(())
}

#[test]
fn graph_report_summarizes_edges_and_hubs() -> Result<()> {
    let (_dir, db) = temp_db()?;

    let service_file_id = db.upsert_file("src/service.rs", "rust", 40, 111, "hash-service")?;
    let service_frag = sample_fragment(
        "src/service.rs",
        "frag-service",
        Some("Service"),
        "service entry",
        0,
        vec![],
    );
    let service_rowid = db.upsert_fragment(service_file_id, &service_frag)?;

    let model_file_id = db.upsert_file("src/model.rs", "rust", 40, 222, "hash-model")?;
    let model_frag = sample_fragment(
        "src/model.rs",
        "frag-model",
        Some("Model"),
        "model type",
        10,
        vec![],
    );
    let model_rowid = db.upsert_fragment(model_file_id, &model_frag)?;

    db.upsert_edge(service_rowid, model_rowid, "refers", 1.5)?;
    db.upsert_edge(model_rowid, service_rowid, "imported_by", 0.8)?;

    let report = db.graph_report(GraphReportOptions {
        max_hubs: 5,
        max_edges: 5,
    })?;

    assert_eq!(report.fragment_count, 2);
    assert_eq!(report.edge_count, 2);
    assert!(report
        .edge_types
        .iter()
        .any(|edge_type| edge_type.edge_type == "refers" && edge_type.count == 1));
    assert!(report
        .top_hubs
        .iter()
        .any(|hub| hub.symbol.as_deref() == Some("Service")));
    assert!(report
        .strong_edges
        .iter()
        .any(|edge| edge.edge_type == "refers" && edge.from_path == "src/service.rs"));

    let markdown = report.to_markdown();
    assert!(markdown.contains("# Prune Graph Report"));
    assert!(markdown.contains("## Top Hubs"));
    assert!(markdown.contains("Service (src/service.rs)"));
    assert!(markdown.contains("## Suggested Questions"));

    Ok(())
}

#[test]
fn repository_memory_persists_and_retrieves_decisions_and_golden_paths() -> Result<()> {
    let (_dir, db) = temp_db()?;
    db.add_repository_memory(
        "decision",
        "Use actor isolation",
        "All UI state stays on MainActor",
        "actor isolation ui state mainactor",
        Some("src/App.swift"),
        Some("concurrency"),
    )?;
    db.add_repository_memory(
        "golden_path",
        "Authentication flow",
        "Route login through AuthService",
        "authentication login authservice",
        Some("src/AuthService.swift"),
        None,
    )?;
    assert_eq!(db.list_repository_memory(None, 10, 0)?.len(), 2);
    let hits = db.search_repository_memory("Fix login in src/AuthService.swift", 0.1, 5)?;
    assert_eq!(hits[0].1.kind, "golden_path");
    Ok(())
}
