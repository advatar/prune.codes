#[cfg(feature = "surreal")]
use ce_store_core::{
    CeStore, EdgeRecord, FileRecord, FragmentRecord, PackRequest, PackResult, RepoIdentity,
    SearchHit,
};

#[cfg(feature = "surreal")]
mod graph;
#[cfg(feature = "surreal")]
mod pack;
#[cfg(feature = "surreal")]
mod schema;

#[cfg(feature = "surreal")]
pub use graph::{collect_file_neighborhood, collect_frag_neighborhood, shortest_path_frags};

#[cfg(feature = "surreal")]
use anyhow::{Context, Result};
#[cfg(feature = "surreal")]
use async_trait::async_trait;
#[cfg(feature = "surreal")]
use ce_core::model::FragKind;
#[cfg(feature = "surreal")]
use serde_json::json;
#[cfg(feature = "surreal")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "surreal")]
use surrealdb::engine::any::connect;
#[cfg(feature = "surreal")]
use surrealdb::engine::any::Any;
#[cfg(feature = "surreal")]
use surrealdb::sql::Thing;
#[cfg(feature = "surreal")]
use surrealdb::Surreal;

#[cfg(feature = "surreal")]
#[derive(Clone, Debug)]
pub enum SurrealEngine {
    Mem,
    SurrealKv { path: String, versioned: bool },
}

#[cfg(feature = "surreal")]
#[derive(Clone, Debug)]
pub struct SurrealConfig {
    pub ns: String,
    pub db: String,
    pub engine: SurrealEngine,
    pub embedding_dim: usize,
    pub fts_enabled: bool,
}

#[cfg(feature = "surreal")]
pub struct SurrealStore {
    pub cfg: SurrealConfig,
    pub db: Surreal<Any>,
}

#[cfg(feature = "surreal")]
#[derive(Clone, Debug)]
pub struct ImportEdgeRecord {
    pub repo_id: String,
    pub from_file_id: String,
    pub to_file_id: String,
    pub lang: String,
    pub specifier: String,
    pub resolved_path: Option<String>,
    pub is_type_only: Option<bool>,
    pub weight: f32,
    pub confidence: f32,
    pub origin: String,
}

#[cfg(feature = "surreal")]
#[derive(Clone, Debug)]
pub struct ContainsEdgeRecord {
    pub repo_id: String,
    pub file_id: String,
    pub frag_id: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub weight: f32,
    pub confidence: f32,
}

#[cfg(feature = "surreal")]
#[derive(Clone, Debug)]
pub struct RelEdgeRecord {
    pub repo_id: String,
    pub from_id: String,
    pub etype: String,
    pub to_id: String,
    pub weight: f32,
    pub confidence: f32,
    pub origin: String,
    pub meta: serde_json::Value,
}

#[cfg(feature = "surreal")]
const DEFAULT_REL_ETYPES: &[&str] = &["ref_def", "calls", "type_uses", "jsx_uses", "tests"];

#[cfg(feature = "surreal")]
impl SurrealStore {
    pub async fn connect(cfg: SurrealConfig) -> Result<Self> {
        let endpoint = match &cfg.engine {
            SurrealEngine::Mem => "mem://".to_string(),
            SurrealEngine::SurrealKv { path, versioned } => {
                if *versioned {
                    format!("surrealkv+versioned://{path}")
                } else {
                    format!("surrealkv://{path}")
                }
            }
        };

        let db = connect(endpoint)
            .await
            .context("failed to connect to embedded SurrealDB")?;

        db.use_ns(&cfg.ns)
            .use_db(&cfg.db)
            .await
            .context("failed to select namespace/db")?;

        schema::ensure_schema(&db, cfg.embedding_dim).await?;

        Ok(Self { cfg, db })
    }

    fn record_id(table: &str, id: &str) -> String {
        format!("{table}:{id}")
    }

    fn record_thing(table: &str, id: &str) -> Thing {
        Thing::from((table, id))
    }

    fn edge_record_id(from_id: &str, edge_type: &str, to_id: &str) -> String {
        let raw = format!("{from_id}|{edge_type}|{to_id}");
        ce_core::util::hash_text_hex(&raw)
    }

    fn strip_prefix(table: &str, id: &str) -> String {
        id.trim_start_matches(&format!("{table}:"))
            .trim_matches('"')
            .to_string()
    }

    pub async fn upsert_import_edges(&self, edges: &[ImportEdgeRecord]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut q = String::new();
        for e in edges {
            let edge_key = format!("imports|{}", e.specifier);
            let edge_id = Self::edge_record_id(&e.from_file_id, &edge_key, &e.to_file_id);
            let from = Self::record_id("file", &e.from_file_id);
            let to = Self::record_id("file", &e.to_file_id);
            let repo_id = serde_json::to_string(&e.repo_id)?;
            let lang = serde_json::to_string(&e.lang)?;
            let specifier = serde_json::to_string(&e.specifier)?;
            let resolved_path = serde_json::to_string(&e.resolved_path)?;
            let is_type_only = serde_json::to_string(&e.is_type_only)?;
            let origin = serde_json::to_string(&e.origin)?;
            q.push_str(&format!(
                "RELATE {from}->imports:{edge_id}->{to} SET repo_id = {repo_id}, lang = {lang}, specifier = {specifier}, resolved_path = {resolved_path}, is_type_only = {is_type_only}, weight = {weight}, confidence = {confidence}, origin = {origin};\n",
                weight = e.weight,
                confidence = e.confidence
            ));
        }
        self.db.query(q).await?;
        Ok(())
    }

    pub async fn upsert_contains_edges(&self, edges: &[ContainsEdgeRecord]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut q = String::new();
        for e in edges {
            let edge_id = Self::edge_record_id(&e.file_id, "contains", &e.frag_id);
            let from = Self::record_id("file", &e.file_id);
            let to = Self::record_id("frag", &e.frag_id);
            let repo_id = serde_json::to_string(&e.repo_id)?;
            let kind = serde_json::to_string(&e.kind)?;
            let symbol = serde_json::to_string(&e.symbol)?;
            let start_line = serde_json::to_string(&e.start_line)?;
            let end_line = serde_json::to_string(&e.end_line)?;
            q.push_str(&format!(
                "RELATE {from}->contains:{edge_id}->{to} SET repo_id = {repo_id}, kind = {kind}, symbol = {symbol}, start_line = {start_line}, end_line = {end_line}, weight = {weight}, confidence = {confidence};\n",
                weight = e.weight,
                confidence = e.confidence
            ));
        }
        self.db.query(q).await?;
        Ok(())
    }

    pub async fn upsert_rel_edges(&self, edges: &[RelEdgeRecord]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut q = String::new();
        for e in edges {
            let edge_id = Self::edge_record_id(&e.from_id, &e.etype, &e.to_id);
            let from = Self::record_id("frag", &e.from_id);
            let to = Self::record_id("frag", &e.to_id);
            let repo_id = serde_json::to_string(&e.repo_id)?;
            let etype = serde_json::to_string(&e.etype)?;
            let origin = serde_json::to_string(&e.origin)?;
            let meta = serde_json::to_string(&e.meta)?;
            q.push_str(&format!(
                "RELATE {from}->rel:{edge_id}->{to} SET repo_id = {repo_id}, etype = {etype}, weight = {weight}, confidence = {confidence}, origin = {origin}, meta = {meta};\n",
                weight = e.weight,
                confidence = e.confidence
            ));
        }
        self.db.query(q).await?;
        Ok(())
    }

    async fn vector_search_rows(
        &self,
        repo_id: &str,
        qvec: &[f32],
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        let qvec64: Vec<f64> = qvec.iter().map(|x| *x as f64).collect();
        let sql = format!(
            r#"
          SELECT id, path, kind, symbol, signature, vector::distance::knn() AS distance
          FROM frag
          WHERE repo_id = $repo_id
            AND embedding <|{k},100|> $qvec
          ORDER BY distance ASC
          LIMIT {k};
        "#
        );
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("qvec", qvec64))
            .await?;

        #[derive(serde::Deserialize)]
        struct Row {
            id: Thing,
            path: String,
            kind: FragKind,
            symbol: Option<String>,
            signature: String,
            distance: f64,
        }

        let rows: Vec<Row> = res.take(0)?;
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                frag_id: Self::strip_prefix("frag", &r.id.to_string()),
                score: (1.0 / (1.0 + r.distance)) as f32,
                reason: "vector".to_string(),
                path: r.path,
                kind: r.kind,
                symbol: r.symbol,
                signature: r.signature,
            })
            .collect())
    }

    async fn fts_search_rows(
        &self,
        repo_id: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        if !self.cfg.fts_enabled {
            return Ok(Vec::new());
        }
        let sql = r#"
          SELECT id, path, kind, symbol, signature, search::score(0) AS score
          FROM frag
          WHERE repo_id = $repo_id
            AND retrieval_text @0@ $q
          ORDER BY score DESC
          LIMIT $k;
        "#;
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("q", query.to_string()))
            .bind(("k", k as i64))
            .await?;

        #[derive(serde::Deserialize)]
        struct Row {
            id: Thing,
            path: String,
            kind: FragKind,
            symbol: Option<String>,
            signature: String,
            score: f64,
        }

        let rows: Vec<Row> = res.take(0)?;
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                frag_id: Self::strip_prefix("frag", &r.id.to_string()),
                score: r.score as f32,
                reason: "fts".to_string(),
                path: r.path,
                kind: r.kind,
                symbol: r.symbol,
                signature: r.signature,
            })
            .collect())
    }
}

#[cfg(feature = "surreal")]
#[async_trait]
impl CeStore for SurrealStore {
    async fn init_repo(&self, repo: &RepoIdentity) -> Result<()> {
        let rid = Self::record_id("repo", &repo.repo_id);
        let content = json!({
            "repo_id": repo.repo_id,
            "root_path": repo.root_path,
            "default_branch": repo.default_branch,
        });
        let q = format!("UPSERT {rid} CONTENT {content};");
        self.db.query(q).await?;
        Ok(())
    }

    async fn upsert_files(&self, files: &[FileRecord]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut q = String::new();
        for f in files {
            let rid = Self::record_id("file", &f.file_id);
            let content = json!({
                "repo_id": f.repo_id,
                "path": f.path,
                "lang": f.lang,
                "size_bytes": f.size_bytes,
                "mtime_ms": f.mtime_ms,
                "content_hash": f.content_hash,
            });
            q.push_str(&format!("UPSERT {rid} CONTENT {content};\n"));
        }
        self.db.query(q).await?;
        Ok(())
    }

    async fn upsert_fragments(&self, frags: &[FragmentRecord]) -> Result<()> {
        if frags.is_empty() {
            return Ok(());
        }
        let mut q = String::new();
        for f in frags {
            let rid = Self::record_id("frag", &f.frag_id);
            let embedding = f
                .embedding
                .as_ref()
                .map(|v| v.iter().map(|x| *x as f64).collect::<Vec<f64>>());
            let content = json!({
                "repo_id": f.repo_id,
                "file_id": f.file_id,
                "path": f.path,
                "lang": f.lang,
                "kind": f.kind,
                "symbol": f.symbol,
                "start_line": f.start_line,
                "end_line": f.end_line,
                "start_byte": f.start_byte,
                "end_byte": f.end_byte,
                "start_col": f.start_col,
                "end_col": f.end_col,
                "signature": f.signature,
                "body": f.body,
                "doc": f.doc,
                "retrieval_text": f.retrieval_text,
                "refs": f.refs,
                "embedding": embedding,
                "token_estimate": f.token_estimate,
            });
            q.push_str(&format!("UPSERT {rid} CONTENT {content};\n"));
        }
        self.db.query(q).await?;
        Ok(())
    }

    async fn upsert_edges(&self, edges: &[EdgeRecord]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let rel_edges: Vec<RelEdgeRecord> = edges
            .iter()
            .map(|e| RelEdgeRecord {
                repo_id: e.repo_id.clone(),
                from_id: e.from_id.clone(),
                etype: e.edge_type.clone(),
                to_id: e.to_id.clone(),
                weight: e.weight,
                confidence: 1.0,
                origin: "edge_record".to_string(),
                meta: e.meta.clone(),
            })
            .collect();
        self.upsert_rel_edges(&rel_edges).await
    }

    async fn delete_missing_files(&self, repo_id: &str, keep_file_ids: &[String]) -> Result<usize> {
        if keep_file_ids.is_empty() {
            let sql = "DELETE file WHERE repo_id = $repo_id";
            let mut res = self
                .db
                .query(sql)
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let rows: Vec<serde_json::Value> = res.take(0)?;
            let _ = self
                .db
                .query("DELETE frag WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let _ = self
                .db
                .query("DELETE imports WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let _ = self
                .db
                .query("DELETE contains WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let _ = self
                .db
                .query("DELETE rel WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            return Ok(rows.len());
        }
        let keep: Vec<Thing> = keep_file_ids
            .iter()
            .map(|id| Self::record_thing("file", id))
            .collect();
        let keep_files = keep.clone();
        let sql = "DELETE file WHERE repo_id = $repo_id AND id NOT IN $keep";
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep_files))
            .await?;
        let rows: Vec<serde_json::Value> = res.take(0)?;
        let _ = self
            .db
            .query("DELETE frag WHERE repo_id = $repo_id AND file_id NOT IN $keep_ids")
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep_ids", keep_file_ids.to_vec()))
            .await?;
        let _ = self
            .db
            .query(
                "DELETE imports WHERE repo_id = $repo_id AND (in NOT IN $keep OR out NOT IN $keep)",
            )
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep.clone()))
            .await?;
        let _ = self
            .db
            .query("DELETE contains WHERE repo_id = $repo_id AND in NOT IN $keep")
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep))
            .await?;
        let _ = self
            .db
            .query(
                "DELETE rel WHERE repo_id = $repo_id AND (in NOT IN (SELECT VALUE id FROM frag WHERE repo_id = $repo_id) OR out NOT IN (SELECT VALUE id FROM frag WHERE repo_id = $repo_id))",
            )
            .bind(("repo_id", repo_id.to_string()))
            .await?;
        Ok(rows.len())
    }

    async fn delete_missing_fragments(
        &self,
        repo_id: &str,
        keep_frag_ids: &[String],
    ) -> Result<usize> {
        if keep_frag_ids.is_empty() {
            let sql = "DELETE frag WHERE repo_id = $repo_id";
            let mut res = self
                .db
                .query(sql)
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let rows: Vec<serde_json::Value> = res.take(0)?;
            let _ = self
                .db
                .query("DELETE contains WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            let _ = self
                .db
                .query("DELETE rel WHERE repo_id = $repo_id")
                .bind(("repo_id", repo_id.to_string()))
                .await?;
            return Ok(rows.len());
        }
        let keep: Vec<Thing> = keep_frag_ids
            .iter()
            .map(|id| Self::record_thing("frag", id))
            .collect();
        let keep_frags = keep.clone();
        let keep_rel = keep.clone();
        let sql = "DELETE frag WHERE repo_id = $repo_id AND id NOT IN $keep";
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep))
            .await?;
        let rows: Vec<serde_json::Value> = res.take(0)?;
        let _ = self
            .db
            .query("DELETE contains WHERE repo_id = $repo_id AND out NOT IN $keep")
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep_frags))
            .await?;
        let _ = self
            .db
            .query("DELETE rel WHERE repo_id = $repo_id AND (in NOT IN $keep OR out NOT IN $keep)")
            .bind(("repo_id", repo_id.to_string()))
            .bind(("keep", keep_rel))
            .await?;
        Ok(rows.len())
    }

    async fn vector_search(
        &self,
        repo_id: &str,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        self.vector_search_rows(repo_id, query_vec, k).await
    }

    async fn fts_search(&self, repo_id: &str, query: &str, k: usize) -> Result<Vec<SearchHit>> {
        self.fts_search_rows(repo_id, query, k).await
    }

    async fn hybrid_search_rrf(
        &self,
        repo_id: &str,
        query: &str,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        let vec_hits = self.vector_search_rows(repo_id, query_vec, k).await?;
        let fts_hits = self.fts_search_rows(repo_id, query, k).await?;
        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut hit_map: HashMap<String, SearchHit> = HashMap::new();
        let rrf_k = 60.0;

        for (rank, h) in vec_hits.into_iter().enumerate() {
            let score = 1.0 / (rrf_k + rank as f32 + 1.0);
            scores
                .entry(h.frag_id.clone())
                .and_modify(|s| *s += score)
                .or_insert(score);
            hit_map.entry(h.frag_id.clone()).or_insert(h);
        }
        for (rank, h) in fts_hits.into_iter().enumerate() {
            let score = 1.0 / (rrf_k + rank as f32 + 1.0);
            scores
                .entry(h.frag_id.clone())
                .and_modify(|s| *s += score)
                .or_insert(score);
            hit_map.entry(h.frag_id.clone()).or_insert(h);
        }
        let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(k);
        let mut out = Vec::new();
        for (fid, score) in ranked {
            if let Some(mut h) = hit_map.remove(&fid) {
                h.score = score;
                h.reason = "hybrid".to_string();
                out.push(h);
            }
        }
        Ok(out)
    }

    async fn fetch_fragments(
        &self,
        _repo_id: &str,
        frag_ids: &[String],
    ) -> Result<Vec<FragmentRecord>> {
        if frag_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Thing> = frag_ids
            .iter()
            .map(|id| Self::record_thing("frag", id))
            .collect();
        let sql = "SELECT * FROM $ids";
        let mut res = self.db.query(sql).bind(("ids", ids)).await?;

        #[derive(serde::Deserialize)]
        struct Row {
            id: Thing,
            repo_id: String,
            file_id: String,
            path: String,
            lang: String,
            kind: FragKind,
            symbol: Option<String>,
            start_line: u32,
            end_line: u32,
            start_byte: u64,
            end_byte: u64,
            start_col: u32,
            end_col: u32,
            signature: String,
            body: String,
            doc: String,
            retrieval_text: String,
            refs: Option<Vec<String>>,
            embedding: Option<Vec<f64>>,
            token_estimate: Option<u32>,
        }

        let rows: Vec<Row> = res.take(0)?;
        let mut out = Vec::new();
        for r in rows {
            let embedding = r
                .embedding
                .map(|v| v.into_iter().map(|x| x as f32).collect());
            out.push(FragmentRecord {
                frag_id: Self::strip_prefix("frag", &r.id.to_string()),
                repo_id: r.repo_id,
                file_id: r.file_id,
                path: r.path,
                lang: r.lang,
                kind: r.kind,
                symbol: r.symbol,
                start_line: r.start_line,
                end_line: r.end_line,
                start_byte: r.start_byte as usize,
                end_byte: r.end_byte as usize,
                start_col: r.start_col,
                end_col: r.end_col,
                signature: r.signature,
                body: r.body,
                doc: r.doc,
                retrieval_text: r.retrieval_text,
                refs: r.refs.unwrap_or_default(),
                embedding,
                token_estimate: r.token_estimate,
            });
        }
        Ok(out)
    }

    async fn expand_graph(
        &self,
        repo_id: &str,
        seed_ids: &[String],
        edge_types: &[String],
        max_nodes: usize,
    ) -> Result<Vec<String>> {
        if seed_ids.is_empty() || max_nodes == 0 {
            return Ok(Vec::new());
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        let max_hops = 2u32;
        let etypes: Vec<String> = if edge_types.is_empty() {
            DEFAULT_REL_ETYPES.iter().map(|s| s.to_string()).collect()
        } else {
            edge_types.to_vec()
        };

        let has_file_seed = seed_ids.iter().any(|s| s.starts_with("file:"));
        for seed in seed_ids {
            if visited.len() >= max_nodes {
                break;
            }
            let ids = if seed.starts_with("file:") {
                graph::collect_file_neighborhood(&self.db, repo_id, seed, max_hops).await?
            } else {
                graph::collect_frag_neighborhood(&self.db, repo_id, seed, max_hops, &etypes).await?
            };
            for id in ids {
                if visited.insert(id.clone()) {
                    out.push(id);
                    if visited.len() >= max_nodes {
                        break;
                    }
                }
            }
        }

        if !has_file_seed && seed_ids.len() > 1 && visited.len() < max_nodes {
            let hub = seed_ids[0].as_str();
            for seed in seed_ids.iter().skip(1) {
                if visited.len() >= max_nodes {
                    break;
                }
                let path =
                    graph::shortest_path_frags(&self.db, repo_id, seed, hub, &etypes, max_hops)
                        .await?;
                for id in path {
                    if visited.insert(id.clone()) {
                        out.push(id);
                        if visited.len() >= max_nodes {
                            break;
                        }
                    }
                }
            }
        }

        Ok(out)
    }

    async fn pack(&self, req: PackRequest) -> Result<PackResult> {
        pack::build_pack(self, req).await
    }

    async fn list_files(&self, repo_id: &str) -> Result<Vec<FileRecord>> {
        let sql = "SELECT id, repo_id, path, lang, size_bytes, mtime_ms, content_hash FROM file WHERE repo_id = $repo_id";
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .await?;

        #[derive(serde::Deserialize)]
        struct Row {
            id: Thing,
            repo_id: String,
            path: String,
            lang: String,
            size_bytes: i64,
            mtime_ms: i64,
            content_hash: String,
        }

        let rows: Vec<Row> = res.take(0)?;
        Ok(rows
            .into_iter()
            .map(|r| FileRecord {
                file_id: Self::strip_prefix("file", &r.id.to_string()),
                repo_id: r.repo_id,
                path: r.path,
                lang: r.lang,
                size_bytes: r.size_bytes,
                mtime_ms: r.mtime_ms,
                content_hash: r.content_hash,
            })
            .collect())
    }

    async fn get_file_by_path(&self, repo_id: &str, path: &str) -> Result<Option<FileRecord>> {
        let sql = "SELECT id, repo_id, path, lang, size_bytes, mtime_ms, content_hash FROM file WHERE repo_id = $repo_id AND path = $path LIMIT 1";
        let mut res = self
            .db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("path", path.to_string()))
            .await?;
        #[derive(serde::Deserialize)]
        struct Row {
            id: Thing,
            repo_id: String,
            path: String,
            lang: String,
            size_bytes: i64,
            mtime_ms: i64,
            content_hash: String,
        }
        let rows: Vec<Row> = res.take(0)?;
        Ok(rows.into_iter().next().map(|r| FileRecord {
            file_id: Self::strip_prefix("file", &r.id.to_string()),
            repo_id: r.repo_id,
            path: r.path,
            lang: r.lang,
            size_bytes: r.size_bytes,
            mtime_ms: r.mtime_ms,
            content_hash: r.content_hash,
        }))
    }
}

#[cfg(feature = "surreal")]
pub use pack::build_pack;

#[cfg(not(feature = "surreal"))]
pub struct SurrealStore;
#[cfg(not(feature = "surreal"))]
#[derive(Clone, Debug)]
pub enum SurrealEngine {
    Mem,
    SurrealKv { path: String, versioned: bool },
}
#[cfg(not(feature = "surreal"))]
#[derive(Clone, Debug)]
pub struct SurrealConfig {
    pub ns: String,
    pub db: String,
    pub engine: SurrealEngine,
    pub embedding_dim: usize,
    pub fts_enabled: bool,
}
#[cfg(not(feature = "surreal"))]
impl SurrealStore {
    pub async fn connect(_cfg: SurrealConfig) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!(
            "ce-store-surreal built without surreal feature"
        ))
    }
}
