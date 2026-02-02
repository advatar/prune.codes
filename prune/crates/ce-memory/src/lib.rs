use anyhow::{anyhow, Result};
use blake3::Hash;
use ce_core::tokenizer::TokenCounter;
use ce_store::embed::{Embedder, DEFAULT_MODEL};
use chrono::{DateTime, NaiveDateTime, Utc};
use fastembed::EmbeddingModel;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const DEFAULT_TOKENIZER: &str = "o200k_base";
const SCHEMA_SQL: &str = include_str!("schema.sql");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub version: u32,
    pub enabled: bool,
    pub store: MemoryStoreConfig,
    pub retrieval: MemoryRetrievalConfig,
    pub embeddings: MemoryEmbeddingsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStoreConfig {
    pub mode: String,
    pub project_path: String,
    pub global_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalConfig {
    pub k: usize,
    pub rrf_k: usize,
    pub recency_half_life_days: f64,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEmbeddingsConfig {
    pub enabled: bool,
    pub model: String,
    pub dims: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            store: MemoryStoreConfig {
                mode: "project".to_string(),
                project_path: ".prune/memory.db".to_string(),
                global_path: "~/.prune/memory.db".to_string(),
            },
            retrieval: MemoryRetrievalConfig {
                k: 12,
                rrf_k: 60,
                recency_half_life_days: 7.0,
                token_budget: 800,
            },
            embeddings: MemoryEmbeddingsConfig {
                enabled: true,
                model: "nomic-embed-text-v1.5".to_string(),
                dims: 768,
            },
        }
    }
}

impl MemoryConfig {
    pub fn config_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".prune").join("memory.json")
    }

    pub fn default_json() -> Result<String> {
        let cfg = Self::default();
        Ok(serde_json::to_string_pretty(&cfg)?)
    }

    pub fn load_or_create(repo_root: &Path) -> Result<Self> {
        let path = Self::config_path(repo_root);
        if path.exists() {
            let raw = fs::read_to_string(&path)?;
            let cfg: MemoryConfig = serde_json::from_str(&raw)?;
            return Ok(cfg);
        }

        let cfg = Self::default();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
        Ok(cfg)
    }

    pub fn project_db_path(&self, repo_root: &Path) -> PathBuf {
        resolve_path(repo_root, &self.store.project_path)
    }

    pub fn global_db_path(&self) -> PathBuf {
        resolve_path(Path::new("."), &self.store.global_path)
    }

    pub fn store_mode(&self) -> MemoryStoreMode {
        MemoryStoreMode::parse(&self.store.mode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStoreMode {
    Project,
    Global,
    Both,
}

impl MemoryStoreMode {
    fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "global" => MemoryStoreMode::Global,
            "both" => MemoryStoreMode::Both,
            _ => MemoryStoreMode::Project,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub score: f64,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecallResult {
    pub items: Vec<MemoryItem>,
    pub budget: MemoryBudget,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryBudget {
    pub token_budget: usize,
    pub approx_tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
    pub total: usize,
    pub stores: Vec<MemoryStoreStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStoreStats {
    pub scope: String,
    pub path: String,
    pub total: usize,
}

#[derive(Debug)]
struct MemoryRow {
    id: i64,
    project_id: Option<String>,
    content: String,
    tags: Vec<String>,
    source: Option<String>,
    created_at: String,
}

pub struct MemoryManager {
    config: MemoryConfig,
    repo_root: PathBuf,
    project: Option<MemoryStore>,
    global: Option<MemoryStore>,
}

impl MemoryManager {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let repo_root = repo_root.to_path_buf();
        let config = MemoryConfig::load_or_create(&repo_root)?;

        if !config.enabled {
            return Ok(Self {
                config,
                repo_root,
                project: None,
                global: None,
            });
        }

        let mut project = None;
        let mut global = None;

        match config.store_mode() {
            MemoryStoreMode::Project => {
                let path = config.project_db_path(&repo_root);
                project = Some(MemoryStore::open(path, &config.embeddings)?);
            }
            MemoryStoreMode::Global => {
                let path = config.global_db_path();
                global = Some(MemoryStore::open(path, &config.embeddings)?);
            }
            MemoryStoreMode::Both => {
                let path = config.project_db_path(&repo_root);
                project = Some(MemoryStore::open(path, &config.embeddings)?);
                let path = config.global_db_path();
                global = Some(MemoryStore::open(path, &config.embeddings)?);
            }
        }

        Ok(Self {
            config,
            repo_root,
            project,
            global,
        })
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn recall(
        &self,
        query: &str,
        project_id: Option<&str>,
        k: Option<usize>,
        token_budget: Option<usize>,
    ) -> Result<MemoryRecallResult> {
        if !self.config.enabled {
            return Err(anyhow!("memory disabled in .prune/memory.json"));
        }
        let k = k.unwrap_or(self.config.retrieval.k);
        let token_budget = token_budget.unwrap_or(self.config.retrieval.token_budget);
        let mut scored: Vec<MemoryItem> = Vec::new();

        let mut stores: Vec<(&str, &MemoryStore)> = Vec::new();
        if let Some(store) = &self.project {
            stores.push(("project", store));
        }
        if let Some(store) = &self.global {
            stores.push(("global", store));
        }

        let both = self.project.is_some() && self.global.is_some();
        for (scope, store) in stores {
            let mut items = store.recall(
                query,
                project_id,
                k,
                self.config.retrieval.rrf_k,
                self.config.retrieval.recency_half_life_days,
            )?;
            for item in items.iter_mut() {
                item.store = if both {
                    Some(scope.to_string())
                } else {
                    None
                };
                item.id = format_memory_id(&item.id, scope, both);
            }
            scored.extend(items);
        }

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        if scored.len() > k {
            scored.truncate(k);
        }

        let counter = TokenCounter::new(DEFAULT_TOKENIZER);
        let mut budget_used = 0usize;
        let mut final_items: Vec<MemoryItem> = Vec::new();
        for item in scored {
            let token_text = format!("{}\nTags: {}", item.content, item.tags.join(", "));
            let tokens = counter.count(&token_text);
            if !final_items.is_empty() && budget_used + tokens > token_budget {
                break;
            }
            budget_used += tokens;
            final_items.push(item);
        }

        Ok(MemoryRecallResult {
            items: final_items,
            budget: MemoryBudget {
                token_budget,
                approx_tokens: budget_used,
            },
        })
    }

    pub fn remember(
        &self,
        content: &str,
        project_id: Option<&str>,
        tags: &[String],
        source: Option<&str>,
    ) -> Result<Vec<MemoryItem>> {
        if !self.config.enabled {
            return Err(anyhow!("memory disabled in .prune/memory.json"));
        }

        let mut out: Vec<MemoryItem> = Vec::new();
        let both = self.project.is_some() && self.global.is_some();
        if let Some(store) = &self.project {
            let mut item = store.remember(content, project_id, tags, source, "project")?;
            item.store = if both {
                Some("project".to_string())
            } else {
                None
            };
            item.id = format_memory_id(&item.id, "project", both);
            out.push(item);
        }
        if let Some(store) = &self.global {
            let mut item = store.remember(content, project_id, tags, source, "global")?;
            item.store = if both {
                Some("global".to_string())
            } else {
                None
            };
            item.id = format_memory_id(&item.id, "global", both);
            out.push(item);
        }
        Ok(out)
    }

    pub fn save_session(
        &self,
        content: &str,
        project_id: Option<&str>,
        tags: &[String],
    ) -> Result<Vec<MemoryItem>> {
        self.remember(content, project_id, tags, Some("session_save"))
    }

    pub fn delete(&self, id: &str, project_id: Option<&str>) -> Result<()> {
        if !self.config.enabled {
            return Err(anyhow!("memory disabled in .prune/memory.json"));
        }
        let parsed = parse_memory_id(id);
        match parsed.scope.as_deref() {
            Some("project") => {
                if let Some(store) = &self.project {
                    store.delete(parsed.id, project_id)?;
                }
                Ok(())
            }
            Some("global") => {
                if let Some(store) = &self.global {
                    store.delete(parsed.id, project_id)?;
                }
                Ok(())
            }
            _ => {
                if let Some(store) = &self.project {
                    store.delete(parsed.id, project_id)?;
                }
                if let Some(store) = &self.global {
                    store.delete(parsed.id, project_id)?;
                }
                Ok(())
            }
        }
    }

    pub fn stats(&self, project_id: Option<&str>) -> Result<MemoryStats> {
        let mut stores: Vec<MemoryStoreStats> = Vec::new();
        let mut total = 0usize;

        if let Some(store) = &self.project {
            let count = store.stats(project_id)?;
            stores.push(MemoryStoreStats {
                scope: "project".to_string(),
                path: store.path.display().to_string(),
                total: count,
            });
            total += count;
        }
        if let Some(store) = &self.global {
            let count = store.stats(project_id)?;
            stores.push(MemoryStoreStats {
                scope: "global".to_string(),
                path: store.path.display().to_string(),
                total: count,
            });
            total += count;
        }

        Ok(MemoryStats { total, stores })
    }
}

struct MemoryStore {
    path: PathBuf,
    conn: Connection,
    embedder: Option<Embedder>,
    embedding_dim: usize,
}

impl MemoryStore {
    fn open(path: PathBuf, embeddings: &MemoryEmbeddingsConfig) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA_SQL)?;

        let (embedder, embedding_dim) = if embeddings.enabled {
            let model = embedding_model_for_config(embeddings)?;
            let embedder = Embedder::new(model)?;
            let dim = embedder.dim();
            (Some(embedder), dim.max(1))
        } else {
            (None, embeddings.dims.max(1))
        };

        Ok(Self {
            path,
            conn,
            embedder,
            embedding_dim,
        })
    }

    fn remember(
        &self,
        content: &str,
        project_id: Option<&str>,
        tags: &[String],
        source: Option<&str>,
        scope: &str,
    ) -> Result<MemoryItem> {
        let content = content.trim();
        if content.is_empty() {
            return Err(anyhow!("memory content is empty"));
        }

        let tags_text = normalize_tags(tags).join(", ");
        let content_hash = hash_content(content);
        let embedding = if let Some(embedder) = &self.embedder {
            let vec = embedder.embed_passages(&[content.to_string()])?;
            vec.first().cloned()
        } else {
            None
        };
        let embedding_blob = embedding
            .as_ref()
            .map(|v| bytemuck::cast_slice(v).to_vec());

        let affected = self.conn.execute(
            "INSERT OR IGNORE INTO memories (project_id, content, content_hash, tags, source, embedding) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![project_id, content, content_hash, tags_text, source, embedding_blob],
        )?;

        let id = if affected == 0 {
            let id: i64 = self.conn
                .query_row(
                    "SELECT id FROM memories WHERE content_hash = ?1",
                    params![content_hash],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| anyhow!("memory dedupe failed"))?;
            if !tags_text.is_empty() {
                self.conn
                    .execute("UPDATE memories SET tags = ?1 WHERE id = ?2", params![tags_text, id])?;
            }
            if let Some(blob) = embedding_blob.clone() {
                self.conn.execute(
                    "UPDATE memories SET embedding = ?1 WHERE id = ?2",
                    params![blob, id],
                )?;
            }
            id
        } else {
            self.conn.last_insert_rowid()
        };

        self.conn
            .execute("DELETE FROM memories_fts WHERE rowid = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO memories_fts(rowid, content, tags) VALUES (?1, ?2, ?3)",
            params![id, content, tags_text],
        )?;

        let created_at: String = self.conn.query_row(
            "SELECT created_at FROM memories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        Ok(MemoryItem {
            id: id.to_string(),
            score: 0.0,
            content: content.to_string(),
            tags: normalize_tags(tags),
            created_at,
            source: source.map(|s| s.to_string()),
            project_id: project_id.map(|p| p.to_string()),
            store: Some(scope.to_string()),
        })
    }

    fn recall(
        &self,
        query: &str,
        project_id: Option<&str>,
        k: usize,
        rrf_k: usize,
        half_life_days: f64,
    ) -> Result<Vec<MemoryItem>> {
        let fts_rows = self.fts_search(query, project_id, k * 4)?;
        let vec_rows = self.vector_search(query, project_id, k * 4)?;

        let mut rows: HashMap<i64, MemoryRow> = HashMap::new();
        let mut fts_rank: HashMap<i64, usize> = HashMap::new();
        let mut vec_rank: HashMap<i64, usize> = HashMap::new();

        for (idx, row) in fts_rows.into_iter().enumerate() {
            fts_rank.insert(row.id, idx + 1);
            rows.entry(row.id).or_insert(row);
        }
        for (idx, row) in vec_rows.into_iter().enumerate() {
            vec_rank.insert(row.id, idx + 1);
            rows.entry(row.id).or_insert(row);
        }

        let mut scored: Vec<MemoryItem> = Vec::new();
        for (id, row) in rows {
            let mut score = 0.0f64;
            if let Some(rank) = fts_rank.get(&id) {
                score += 1.0 / ((rrf_k + rank) as f64);
            }
            if let Some(rank) = vec_rank.get(&id) {
                score += 1.0 / ((rrf_k + rank) as f64);
            }
            score *= recency_weight(&row.created_at, half_life_days);

            scored.push(MemoryItem {
                id: id.to_string(),
                score,
                content: row.content,
                tags: row.tags,
                created_at: row.created_at,
                source: row.source,
                project_id: row.project_id,
                store: None,
            });
        }

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        if scored.len() > k {
            scored.truncate(k);
        }

        Ok(scored)
    }

    fn stats(&self, project_id: Option<&str>) -> Result<usize> {
        let count: i64 = if let Some(pid) = project_id {
            self.conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE project_id = ?1",
                params![pid],
                |row| row.get(0),
            )?
        } else {
            self.conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?
        };
        Ok(count as usize)
    }

    fn delete(&self, id: i64, project_id: Option<&str>) -> Result<()> {
        if let Some(pid) = project_id {
            self.conn.execute(
                "DELETE FROM memories WHERE id = ?1 AND project_id = ?2",
                params![id, pid],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        }
        self.conn
            .execute("DELETE FROM memories_fts WHERE rowid = ?1", params![id])?;
        Ok(())
    }

    fn fts_search(&self, query: &str, project_id: Option<&str>, limit: usize) -> Result<Vec<MemoryRow>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let mut rows: Vec<MemoryRow> = Vec::new();
        if let Some(pid) = project_id {
            let mut stmt = self.conn.prepare(
                "SELECT m.id, m.project_id, m.content, m.tags, m.source, m.created_at \n                 FROM memories_fts \n                 JOIN memories m ON m.id = memories_fts.rowid \n                 WHERE memories_fts MATCH ?1 AND m.project_id = ?2 \n                 ORDER BY bm25(memories_fts) ASC \n                 LIMIT ?3",
            )?;
            let mut iter = stmt.query(params![query, pid, limit as i64])?;
            while let Some(row) = iter.next()? {
                rows.push(row_to_memory(row)?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT m.id, m.project_id, m.content, m.tags, m.source, m.created_at \n                 FROM memories_fts \n                 JOIN memories m ON m.id = memories_fts.rowid \n                 WHERE memories_fts MATCH ?1 \n                 ORDER BY bm25(memories_fts) ASC \n                 LIMIT ?2",
            )?;
            let mut iter = stmt.query(params![query, limit as i64])?;
            while let Some(row) = iter.next()? {
                rows.push(row_to_memory(row)?);
            }
        }
        Ok(rows)
    }

    fn vector_search(&self, query: &str, project_id: Option<&str>, limit: usize) -> Result<Vec<MemoryRow>> {
        let Some(embedder) = &self.embedder else {
            return Ok(Vec::new());
        };
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let qvec = embedder.embed_query(query)?;
        let qnorm = vector_norm(&qvec);
        if qnorm == 0.0 {
            return Ok(Vec::new());
        }

        let mut stmt = if project_id.is_some() {
            self.conn.prepare(
                "SELECT id, project_id, content, tags, source, created_at, embedding \n                 FROM memories WHERE embedding IS NOT NULL AND project_id = ?1",
            )?
        } else {
            self.conn.prepare(
                "SELECT id, project_id, content, tags, source, created_at, embedding \n                 FROM memories WHERE embedding IS NOT NULL",
            )?
        };

        let mut scored: Vec<(MemoryRow, f32)> = Vec::new();
        let mut rows = if let Some(pid) = project_id {
            stmt.query(params![pid])?
        } else {
            stmt.query([])?
        };

        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let project_id: Option<String> = row.get(1)?;
            let content: String = row.get(2)?;
            let tags_raw: Option<String> = row.get(3)?;
            let source: Option<String> = row.get(4)?;
            let created_at: String = row.get(5)?;
            let blob: Option<Vec<u8>> = row.get(6)?;
            let Some(blob) = blob else { continue };
            let emb = bytes_to_f32(&blob, self.embedding_dim)?;
            let score = cosine_similarity(&qvec, &emb, qnorm);
            let row = MemoryRow {
                id,
                project_id,
                content,
                tags: split_tags(tags_raw),
                source,
                created_at,
            };
            scored.push((row, score));
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if scored.len() > limit {
            scored.truncate(limit);
        }
        Ok(scored.into_iter().map(|(row, _)| row).collect())
    }
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        content: row.get(2)?,
        tags: split_tags(row.get::<_, Option<String>>(3)?),
        source: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn split_tags(raw: Option<String>) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    raw.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn hash_content(content: &str) -> String {
    let hash = Hash::from(blake3::hash(content.as_bytes()));
    hash.to_hex().to_string()
}

fn recency_weight(created_at: &str, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let Ok(dt) = NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S") else {
        return 1.0;
    };
    let created_at = DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
    let age = Utc::now().signed_duration_since(created_at);
    let age_days = age.num_seconds() as f64 / 86_400.0;
    0.5_f64.powf(age_days / half_life_days)
}

pub fn embedding_model_for_config(cfg: &MemoryEmbeddingsConfig) -> Result<EmbeddingModel> {
    let raw = cfg.model.trim();
    if raw.is_empty() {
        return Ok(DEFAULT_MODEL);
    }
    if let Ok(model) = EmbeddingModel::from_str(raw) {
        return Ok(model);
    }
    match raw.to_lowercase().as_str() {
        "nomic-embed-text-v1.5" => Ok(EmbeddingModel::NomicEmbedTextV15),
        "nomic-embed-text-v1" => Ok(EmbeddingModel::NomicEmbedTextV1),
        "all-minilm-l6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),
        _ => Err(anyhow!("Unknown embedding model: {raw}")),
    }
}

fn resolve_path(repo_root: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    if raw.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(raw.replacen('~', &home, 1));
        }
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    }
}

struct ParsedMemoryId {
    id: i64,
    scope: Option<String>,
}

fn format_memory_id(id: &str, scope: &str, both: bool) -> String {
    if both {
        format!("mem:{scope}:{id}")
    } else {
        format!("mem:{id}")
    }
}

fn parse_memory_id(id: &str) -> ParsedMemoryId {
    let trimmed = id.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() >= 3 && parts[0] == "mem" {
        let scope = parts[1].to_string();
        let id = parts[2].parse().unwrap_or(0);
        return ParsedMemoryId {
            id,
            scope: Some(scope),
        };
    }
    if parts.len() == 2 && parts[0] == "mem" {
        let id = parts[1].parse().unwrap_or(0);
        return ParsedMemoryId { id, scope: None };
    }
    let id = trimmed.parse().unwrap_or(0);
    ParsedMemoryId { id, scope: None }
}

fn bytes_to_f32(bytes: &[u8], dim: usize) -> Result<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("invalid embedding blob"));
    }
    let slice: &[f32] = bytemuck::cast_slice(bytes);
    if slice.len() < dim {
        return Err(anyhow!("embedding dim mismatch"));
    }
    Ok(slice[..dim].to_vec())
}

fn vector_norm(vec: &[f32]) -> f32 {
    vec.iter().map(|v| v * v).sum::<f32>().sqrt()
}

fn cosine_similarity(query: &[f32], other: &[f32], query_norm: f32) -> f32 {
    let other_norm = vector_norm(other);
    if query_norm == 0.0 || other_norm == 0.0 {
        return 0.0;
    }
    let dot: f32 = query.iter().zip(other.iter()).map(|(a, b)| a * b).sum();
    dot / (query_norm * other_norm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn schema_is_idempotent() -> Result<()> {
        let dir = TempDir::new()?;
        let db_path = dir.path().join("mem.db");
        let store = MemoryStore::open(db_path, &MemoryEmbeddingsConfig {
            enabled: false,
            model: "".to_string(),
            dims: 16,
        })?;
        store.conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    #[test]
    fn dedupe_by_content_hash() -> Result<()> {
        let dir = TempDir::new()?;
        let db_path = dir.path().join("mem.db");
        let store = MemoryStore::open(db_path, &MemoryEmbeddingsConfig {
            enabled: false,
            model: "".to_string(),
            dims: 16,
        })?;
        let tags = vec!["alpha".to_string()];
        let a = store.remember("hello world", None, &tags, Some("explicit"), "project")?;
        let b = store.remember("hello world", None, &[], Some("explicit"), "project")?;
        assert_eq!(a.id, b.id);
        Ok(())
    }

    #[test]
    fn recency_decay_prefers_newer() {
        let now = Utc::now();
        let older = now - chrono::Duration::days(14);
        let older_str = older.format("%Y-%m-%d %H:%M:%S").to_string();
        let newer_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let half = 7.0;
        let w_old = recency_weight(&older_str, half);
        let w_new = recency_weight(&newer_str, half);
        assert!(w_new > w_old);
    }

    #[test]
    fn rrf_fusion_rewards_high_ranks() -> Result<()> {
        let mut rows = HashMap::new();
        rows.insert(1, MemoryRow {
            id: 1,
            project_id: None,
            content: "a".to_string(),
            tags: vec![],
            source: None,
            created_at: Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });
        let mut fts_rank = HashMap::new();
        let mut vec_rank = HashMap::new();
        fts_rank.insert(1, 1);
        vec_rank.insert(1, 5);
        let rrf_k = 60usize;
        let row = rows.get(&1).unwrap();
        let mut score = 0.0f64;
        score += 1.0 / ((rrf_k + fts_rank[&1]) as f64);
        score += 1.0 / ((rrf_k + vec_rank[&1]) as f64);
        let weight = recency_weight(&row.created_at, 7.0);
        let final_score = score * weight;
        assert!(final_score > 0.0);
        Ok(())
    }
}
