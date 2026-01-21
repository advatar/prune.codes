#![cfg(feature = "surreal")]

use anyhow::Result;
use ce_store_core::{CeStore, FileRecord, FragmentRecord, PackRequest, RepoIdentity};
use ce_store_surreal::{ImportEdgeRecord, RelEdgeRecord, SurrealConfig, SurrealEngine, SurrealStore};
use ce_core::model::{FragKind, StrategyConfig};
use surrealdb::sql::Thing;

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

    let edges = vec![RelEdgeRecord {
        repo_id: repo_id.to_string(),
        from_id: "fragone".to_string(),
        etype: "ref_def".to_string(),
        to_id: "fragtwo".to_string(),
        weight: 1.0,
        confidence: 0.9,
        origin: "test".to_string(),
        meta: serde_json::json!({}),
    }];
    store.upsert_rel_edges(&edges).await?;

    let expanded = store
        .expand_graph(repo_id, &["fragone".to_string()], &[], 8)
        .await?;
    assert!(expanded.contains(&"fragtwo".to_string()));
    Ok(())
}

#[tokio::test]
async fn arrow_traversal_imports_both_directions() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "imports".to_string(),
        db: "imports".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: false,
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

    let files = vec![
        FileRecord {
            file_id: "filea".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/a.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
        FileRecord {
            file_id: "fileb".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/b.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
    ];
    store.upsert_files(&files).await?;

    store
        .upsert_import_edges(&[ImportEdgeRecord {
            repo_id: repo_id.to_string(),
            from_file_id: "filea".to_string(),
            to_file_id: "fileb".to_string(),
            lang: "ts".to_string(),
            specifier: "./b".to_string(),
            resolved_path: Some("src/b.ts".to_string()),
            is_type_only: Some(false),
            weight: 1.0,
            confidence: 1.0,
            origin: "test".to_string(),
        }])
        .await?;

    let mut res = store
        .db
        .query("RETURN type::thing(\"file\", $a)->imports->file.id")
        .bind(("a", "filea"))
        .await?;
    let forward: Vec<Thing> = res.take(0)?;
    assert!(forward.iter().any(|id| id.to_string().contains("file:fileb")));

    let mut res = store
        .db
        .query("RETURN type::thing(\"file\", $b)<-imports<-file.id")
        .bind(("b", "fileb"))
        .await?;
    let reverse: Vec<Thing> = res.take(0)?;
    assert!(reverse.iter().any(|id| id.to_string().contains("file:filea")));

    Ok(())
}

#[tokio::test]
async fn collect_returns_bounded_neighborhood() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "collect".to_string(),
        db: "collect".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: false,
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

    let files = vec![
        FileRecord {
            file_id: "filea".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/a.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
        FileRecord {
            file_id: "fileb".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/b.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
        FileRecord {
            file_id: "filec".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/c.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
    ];
    store.upsert_files(&files).await?;

    store
        .upsert_import_edges(&[
            ImportEdgeRecord {
                repo_id: repo_id.to_string(),
                from_file_id: "filea".to_string(),
                to_file_id: "fileb".to_string(),
                lang: "ts".to_string(),
                specifier: "./b".to_string(),
                resolved_path: Some("src/b.ts".to_string()),
                is_type_only: Some(false),
                weight: 1.0,
                confidence: 1.0,
                origin: "test".to_string(),
            },
            ImportEdgeRecord {
                repo_id: repo_id.to_string(),
                from_file_id: "fileb".to_string(),
                to_file_id: "filec".to_string(),
                lang: "ts".to_string(),
                specifier: "./c".to_string(),
                resolved_path: Some("src/c.ts".to_string()),
                is_type_only: Some(false),
                weight: 1.0,
                confidence: 1.0,
                origin: "test".to_string(),
            },
        ])
        .await?;

    let sql =
        "RETURN type::thing(\"file\", $a).{..1+collect}(->imports[WHERE repo_id = $repo_id]->file).id";
    let mut res = store
        .db
        .query(sql)
        .bind(("a", "filea"))
        .bind(("repo_id", repo_id))
        .await?;
    let hop1: Vec<Thing> = res.take(0)?;
    let hop1_ids: Vec<String> = hop1.iter().map(|id| id.to_string()).collect();
    assert!(hop1_ids.iter().any(|id| id.contains("file:fileb")));
    assert!(!hop1_ids.iter().any(|id| id.contains("file:filec")));

    let sql =
        "RETURN type::thing(\"file\", $a).{..2+collect}(->imports[WHERE repo_id = $repo_id]->file).id";
    let mut res = store
        .db
        .query(sql)
        .bind(("a", "filea"))
        .bind(("repo_id", repo_id))
        .await?;
    let hop2: Vec<Thing> = res.take(0)?;
    let hop2_ids: Vec<String> = hop2.iter().map(|id| id.to_string()).collect();
    assert!(hop2_ids.iter().any(|id| id.contains("file:filec")));

    Ok(())
}

#[tokio::test]
async fn shortest_path_returns_connector() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "shortest".to_string(),
        db: "shortest".to_string(),
        engine: SurrealEngine::Mem,
        embedding_dim: 3,
        fts_enabled: false,
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
        file_id: "filex".to_string(),
        repo_id: repo_id.to_string(),
        path: "src/graph.rs".to_string(),
        lang: "rust".to_string(),
        size_bytes: 10,
        mtime_ms: 0,
        content_hash: "hash".to_string(),
    };
    store.upsert_files(&[file]).await?;

    let frags = vec![
        sample_fragment(repo_id, "filex", "src/graph.rs", "fraga", "a", None),
        sample_fragment(repo_id, "filex", "src/graph.rs", "fragb", "b", None),
        sample_fragment(repo_id, "filex", "src/graph.rs", "fragc", "c", None),
        sample_fragment(repo_id, "filex", "src/graph.rs", "fragd", "d", None),
    ];
    store.upsert_fragments(&frags).await?;

    store
        .upsert_rel_edges(&[
            RelEdgeRecord {
                repo_id: repo_id.to_string(),
                from_id: "fraga".to_string(),
                etype: "ref_def".to_string(),
                to_id: "fragb".to_string(),
                weight: 1.0,
                confidence: 0.9,
                origin: "test".to_string(),
                meta: serde_json::json!({}),
            },
            RelEdgeRecord {
                repo_id: repo_id.to_string(),
                from_id: "fraga".to_string(),
                etype: "ref_def".to_string(),
                to_id: "fragd".to_string(),
                weight: 1.0,
                confidence: 0.9,
                origin: "test".to_string(),
                meta: serde_json::json!({}),
            },
            RelEdgeRecord {
                repo_id: repo_id.to_string(),
                from_id: "fragb".to_string(),
                etype: "ref_def".to_string(),
                to_id: "fragc".to_string(),
                weight: 1.0,
                confidence: 0.9,
                origin: "test".to_string(),
                meta: serde_json::json!({}),
            },
            RelEdgeRecord {
                repo_id: repo_id.to_string(),
                from_id: "fragd".to_string(),
                etype: "ref_def".to_string(),
                to_id: "fragc".to_string(),
                weight: 1.0,
                confidence: 0.9,
                origin: "test".to_string(),
                meta: serde_json::json!({}),
            },
        ])
        .await?;

    let ids = ce_store_surreal::shortest_path_frags(
        &store.db,
        repo_id,
        "fraga",
        "fragc",
        &vec!["ref_def".to_string()],
        4,
    )
    .await?;
    assert!(ids.len() >= 3);
    assert!(ids.iter().any(|id| id == "fraga"));
    assert!(ids.iter().any(|id| id == "fragc"));

    Ok(())
}

#[tokio::test]
async fn pack_connectivity_smoke() -> Result<()> {
    let cfg = SurrealConfig {
        ns: "pack".to_string(),
        db: "pack".to_string(),
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

    let files = vec![
        FileRecord {
            file_id: "filea".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/a.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
        FileRecord {
            file_id: "fileb".to_string(),
            repo_id: repo_id.to_string(),
            path: "src/b.ts".to_string(),
            lang: "ts".to_string(),
            size_bytes: 10,
            mtime_ms: 0,
            content_hash: "hash".to_string(),
        },
    ];
    store.upsert_files(&files).await?;

    let mut fraga = sample_fragment(repo_id, "filea", "src/a.ts", "fraga", "alpha", Some(vec![1.0, 0.0, 0.0]));
    fraga.signature = "pub fn alpha()".to_string();
    let mut fragb = sample_fragment(repo_id, "fileb", "src/b.ts", "fragb", "beta", Some(vec![0.0, 1.0, 0.0]));
    fragb.kind = FragKind::ApiSummary;
    fragb.signature = "pub fn beta()".to_string();

    store.upsert_fragments(&[fraga, fragb]).await?;

    store
        .upsert_import_edges(&[ImportEdgeRecord {
            repo_id: repo_id.to_string(),
            from_file_id: "filea".to_string(),
            to_file_id: "fileb".to_string(),
            lang: "ts".to_string(),
            specifier: "./b".to_string(),
            resolved_path: Some("src/b.ts".to_string()),
            is_type_only: Some(false),
            weight: 1.0,
            confidence: 1.0,
            origin: "test".to_string(),
        }])
        .await?;

    store
        .upsert_rel_edges(&[RelEdgeRecord {
            repo_id: repo_id.to_string(),
            from_id: "fraga".to_string(),
            etype: "ref_def".to_string(),
            to_id: "fragb".to_string(),
            weight: 1.0,
            confidence: 0.9,
            origin: "test".to_string(),
            meta: serde_json::json!({}),
        }])
        .await?;

    let mut strategy = StrategyConfig::default();
    strategy.graph_expand = true;
    strategy.candidate_pool_limit = 20;
    strategy.budget_chars = 20000;
    strategy.max_bodies = 4;
    strategy.signals_enabled = false;

    let res = store
        .pack(PackRequest {
            repo_id: repo_id.to_string(),
            query: "alpha".to_string(),
            query_vec: Some(vec![1.0, 0.0, 0.0]),
            strategy,
            seen: None,
        })
        .await?;

    let ids: std::collections::HashSet<String> =
        res.pack.items.iter().map(|it| it.id.clone()).collect();
    assert!(ids.contains("fraga"));
    assert!(ids.contains("fragb"));

    Ok(())
}
