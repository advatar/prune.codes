#![cfg(feature = "surreal")]

use anyhow::Result;
use ce_store_core::{CeStore, EdgeRecord, FileRecord, FragmentRecord, RepoIdentity};
use ce_store_surreal::{SurrealConfig, SurrealEngine, SurrealStore};
use ce_core::model::FragKind;

fn sample_fragment(
    repo_id: &str,
    file_id: &str,
    path: &str,
    frag_id: &str,
    text: &str,
    embedding: Option<Vec<f32>>,
) -> FragmentRecord {
    FragmentRecord {
        frag_id: frag_id.to_string(),
        repo_id: repo_id.to_string(),
        file_id: file_id.to_string(),
        path: path.to_string(),
        lang: "rust".to_string(),
        kind: FragKind::Function,
        symbol: Some("Sample".to_string()),
        start_line: 0,
        end_line: 1,
        start_byte: 0,
        end_byte: 10,
        start_col: 0,
        end_col: 0,
        signature: "fn sample()".to_string(),
        body: text.to_string(),
        doc: String::new(),
        retrieval_text: text.to_string(),
        refs: vec!["Sample".to_string()],
        embedding,
        token_estimate: None,
    }
}

#[tokio::test]
async fn schema_idempotent() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "test".to_string(),
        db: "test".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: true,
    };
    let _store = SurrealStore::connect(cfg.clone()).await?;
    let _store2 = SurrealStore::connect(cfg).await?;
    Ok(())
}

#[tokio::test]
async fn vector_search_returns_hits() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "vec".to_string(),
        db: "vec".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: true,
    };
    let store = SurrealStore::connect(cfg).await?;
    let repo_id = "repo";
    store
        .init_repo(&RepoIdentity {
            repo_id: repo_id.to_string(),
            root_path: "/tmp/repo".to_string(),
            default_branch: None,
        })
        .await?;

    let file = FileRecord {
        file_id: "file1".to_string(),
        repo_id: repo_id.to_string(),
        path: "src/lib.rs".to_string(),
        lang: "rust".to_string(),
        size_bytes: 10,
        mtime_ms: 0,
        content_hash: "hash".to_string(),
    };
    store.upsert_files(&[file]).await?;

    let frags = vec![
        sample_fragment(repo_id, "file1", "src/lib.rs", "fraga", "alpha", Some(vec![1.0, 0.0, 0.0])),
        sample_fragment(repo_id, "file1", "src/lib.rs", "fragb", "beta", Some(vec![0.0, 1.0, 0.0])),
    ];
    store.upsert_fragments(&frags).await?;

    let hits = store.vector_search(repo_id, &[1.0, 0.0, 0.0], 1).await?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].frag_id, "fraga");
    Ok(())
}

#[tokio::test]
async fn fts_search_returns_hits() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "fts".to_string(),
        db: "fts".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: true,
    };
    let store = SurrealStore::connect(cfg).await?;
    let repo_id = "repo";
    store
        .init_repo(&RepoIdentity {
            repo_id: repo_id.to_string(),
            root_path: "/tmp/repo".to_string(),
            default_branch: None,
        })
        .await?;

    let file = FileRecord {
        file_id: "file2".to_string(),
        repo_id: repo_id.to_string(),
        path: "src/main.rs".to_string(),
        lang: "rust".to_string(),
        size_bytes: 10,
        mtime_ms: 0,
        content_hash: "hash".to_string(),
    };
    store.upsert_files(&[file]).await?;

    let frags = vec![
        sample_fragment(repo_id, "file2", "src/main.rs", "fraghello", "hello world", None),
        sample_fragment(repo_id, "file2", "src/main.rs", "fragother", "goodbye", None),
    ];
    store.upsert_fragments(&frags).await?;

    let hits = store.fts_search(repo_id, "hello", 2).await?;
    assert!(hits.iter().any(|h| h.frag_id == "fraghello"));
    Ok(())
}

#[tokio::test]
async fn expand_graph_follows_edges() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "graph".to_string(),
        db: "graph".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: true,
    };
    let store = SurrealStore::connect(cfg).await?;
    let repo_id = "repo";
    store
        .init_repo(&RepoIdentity {
            repo_id: repo_id.to_string(),
            root_path: "/tmp/repo".to_string(),
            default_branch: None,
        })
        .await?;

    let file = FileRecord {
        file_id: "file3".to_string(),
        repo_id: repo_id.to_string(),
        path: "src/graph.rs".to_string(),
        lang: "rust".to_string(),
        size_bytes: 10,
        mtime_ms: 0,
        content_hash: "hash".to_string(),
    };
    store.upsert_files(&[file]).await?;

    let frags = vec![
        sample_fragment(repo_id, "file3", "src/graph.rs", "fragone", "one", None),
        sample_fragment(repo_id, "file3", "src/graph.rs", "fragtwo", "two", None),
    ];
    store.upsert_fragments(&frags).await?;

    let edges = vec![EdgeRecord {
        repo_id: repo_id.to_string(),
        from_id: "fragone".to_string(),
        edge_type: "refers".to_string(),
        to_id: "fragtwo".to_string(),
        weight: 1.0,
        meta: serde_json::json!({}),
    }];
    store.upsert_edges(&edges).await?;

    let expanded = store
        .expand_graph(repo_id, &["fragone".to_string()], &[], 8)
        .await?;
    assert!(expanded.contains(&"fragtwo".to_string()));
    Ok(())
}
