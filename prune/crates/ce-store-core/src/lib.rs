use async_trait::async_trait;
use ce_core::model::{ContextPack, FragKind, Fragment, Span, StrategyConfig};
use ce_core::util::hash_text_hex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoIdentity {
    pub repo_id: String,
    pub root_path: String,
    pub default_branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: String,
    pub repo_id: String,
    pub path: String,
    pub lang: String,
    pub size_bytes: i64,
    pub mtime_ms: i64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FragmentRecord {
    pub frag_id: String,
    pub repo_id: String,
    pub file_id: String,
    pub path: String,
    pub lang: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_col: u32,
    pub end_col: u32,
    pub signature: String,
    pub body: String,
    pub doc: String,
    pub retrieval_text: String,
    pub refs: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub token_estimate: Option<u32>,
}

impl FragmentRecord {
    pub fn to_fragment(&self) -> Fragment {
        Fragment {
            id: self.frag_id.clone(),
            ast_hash: String::new(),
            file: self.path.clone().into(),
            kind: self.kind,
            symbol: self.symbol.clone(),
            span: Span {
                start_byte: self.start_byte,
                end_byte: self.end_byte,
                start_line: self.start_line,
                start_col: self.start_col,
                end_line: self.end_line,
                end_col: self.end_col,
            },
            signature: self.signature.clone(),
            body: self.body.clone(),
            doc: self.doc.clone(),
            retrieval_text: self.retrieval_text.clone(),
            refs: self.refs.clone(),
        }
    }

    pub fn from_fragment(
        repo_id: String,
        file_id: String,
        path: String,
        lang: String,
        frag: &Fragment,
        embedding: Option<Vec<f32>>,
        token_estimate: Option<u32>,
    ) -> Self {
        // Stable, file-unique fragment ID to avoid cross-file collisions.
        let frag_id = hash_text_hex(&format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            path.as_str(),
            frag.span.start_byte,
            frag.span.end_byte,
            frag.span.start_line,
            frag.span.start_col,
            frag.span.end_line,
            frag.span.end_col,
            frag.id
        ));
        Self {
            frag_id,
            repo_id,
            file_id,
            path,
            lang,
            kind: frag.kind,
            symbol: frag.symbol.clone(),
            start_line: frag.span.start_line,
            end_line: frag.span.end_line,
            start_byte: frag.span.start_byte,
            end_byte: frag.span.end_byte,
            start_col: frag.span.start_col,
            end_col: frag.span.end_col,
            signature: frag.signature.clone(),
            body: frag.body.clone(),
            doc: frag.doc.clone(),
            retrieval_text: frag.retrieval_text.clone(),
            refs: frag.refs.clone(),
            embedding,
            token_estimate,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub repo_id: String,
    pub from_id: String,
    pub edge_type: String,
    pub to_id: String,
    pub weight: f32,
    pub meta: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub frag_id: String,
    pub score: f32,
    pub reason: String,
    pub path: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    pub signature: String,
}

#[derive(Clone, Debug)]
pub struct PackRequest {
    pub repo_id: String,
    pub query: String,
    pub query_vec: Option<Vec<f32>>,
    pub strategy: StrategyConfig,
    pub seen: Option<HashSet<String>>,
}

#[derive(Clone, Debug)]
pub struct PackResult {
    pub pack: ContextPack,
    pub debug: Value,
}

#[async_trait]
pub trait CeStore: Send + Sync {
    async fn init_repo(&self, repo: &RepoIdentity) -> anyhow::Result<()>;

    async fn upsert_files(&self, files: &[FileRecord]) -> anyhow::Result<()>;
    async fn upsert_fragments(&self, frags: &[FragmentRecord]) -> anyhow::Result<()>;
    async fn upsert_edges(&self, edges: &[EdgeRecord]) -> anyhow::Result<()>;

    async fn delete_missing_files(&self, repo_id: &str, keep_file_ids: &[String]) -> anyhow::Result<usize>;
    async fn delete_missing_fragments(&self, repo_id: &str, keep_frag_ids: &[String]) -> anyhow::Result<usize>;

    async fn vector_search(&self, repo_id: &str, query_vec: &[f32], k: usize) -> anyhow::Result<Vec<SearchHit>>;
    async fn fts_search(&self, repo_id: &str, query: &str, k: usize) -> anyhow::Result<Vec<SearchHit>>;
    async fn hybrid_search_rrf(
        &self,
        repo_id: &str,
        query: &str,
        query_vec: &[f32],
        k: usize,
    ) -> anyhow::Result<Vec<SearchHit>>;

    async fn fetch_fragments(&self, repo_id: &str, frag_ids: &[String]) -> anyhow::Result<Vec<FragmentRecord>>;
    async fn expand_graph(
        &self,
        repo_id: &str,
        seed_ids: &[String],
        edge_types: &[String],
        max_nodes: usize,
    ) -> anyhow::Result<Vec<String>>;

    async fn pack(&self, req: PackRequest) -> anyhow::Result<PackResult>;

    async fn list_files(&self, repo_id: &str) -> anyhow::Result<Vec<FileRecord>>;
    async fn get_file_by_path(&self, repo_id: &str, path: &str) -> anyhow::Result<Option<FileRecord>>;
}

pub fn file_id_for_path(repo_id: &str, path: &str) -> String {
    ce_core::util::hash_text_hex(&format!("{repo_id}:{path}"))
}
