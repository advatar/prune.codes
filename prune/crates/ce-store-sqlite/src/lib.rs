use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ce_core::model::{Fragment, Span};
use ce_store::query;
use ce_store::Db;
use ce_store_core::{
    file_id_for_path, CeStore, EdgeRecord, FileRecord, FragmentRecord, PackRequest, PackResult,
    RepoIdentity, SearchHit,
};
use rusqlite::params;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub struct SqliteStore {
    db_path: PathBuf,
    hnsw_dir: PathBuf,
}

impl SqliteStore {
    pub fn new(db_path: impl AsRef<Path>, hnsw_dir: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            hnsw_dir: hnsw_dir.as_ref().to_path_buf(),
        }
    }

    fn open(&self) -> Result<Db> {
        Db::open(&self.db_path)
    }

    fn frag_record_from_db(repo_id: &str, frag: Fragment) -> FragmentRecord {
        let file_id = file_id_for_path(repo_id, &frag.file.display().to_string());
        FragmentRecord {
            frag_id: frag.id.clone(),
            repo_id: repo_id.to_string(),
            file_id,
            path: frag.file.display().to_string(),
            lang: String::new(),
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
            embedding: None,
            token_estimate: None,
        }
    }

    fn map_hits(&self, hits: Vec<ce_store::types::SearchHit>, reason: &str) -> Vec<SearchHit> {
        hits.into_iter()
            .map(|h| SearchHit {
                frag_id: h.frag_id,
                score: h.score,
                reason: reason.to_string(),
                path: h.path,
                kind: h.kind,
                symbol: h.symbol,
                signature: h.signature,
            })
            .collect()
    }

    fn rowids_to_frag_ids(db: &Db, rowids: &[i64]) -> Result<Vec<String>> {
        if rowids.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        let mut stmt = db
            .conn()
            .prepare("SELECT frag_id FROM fragments WHERE rowid=?1")?;
        for rid in rowids {
            let frag_id: String = stmt.query_row(params![rid], |r| r.get(0))?;
            ids.push(frag_id);
        }
        Ok(ids)
    }
}

#[async_trait]
impl CeStore for SqliteStore {
    async fn init_repo(&self, repo: &RepoIdentity) -> Result<()> {
        let db = self.open()?;
        let _ = db.set_meta("repo.id", &repo.repo_id);
        let _ = db.set_meta("repo.root", &repo.root_path);
        if let Some(branch) = &repo.default_branch {
            let _ = db.set_meta("repo.branch", branch);
        }
        Ok(())
    }

    async fn upsert_files(&self, files: &[FileRecord]) -> Result<()> {
        let db = self.open()?;
        for f in files {
            db.upsert_file(&f.path, &f.lang, f.size_bytes, f.mtime_ms, &f.content_hash)?;
        }
        Ok(())
    }

    async fn upsert_fragments(&self, frags: &[FragmentRecord]) -> Result<()> {
        let db = self.open()?;
        for f in frags {
            let file_id = db
                .get_file_info(&f.path)?
                .map(|(id, _)| id)
                .ok_or_else(|| anyhow!("missing file row for {}", f.path))?;
            let frag = Fragment {
                id: f.frag_id.clone(),
                ast_hash: String::new(),
                file: f.path.clone().into(),
                kind: f.kind,
                symbol: f.symbol.clone(),
                span: Span {
                    start_byte: f.start_byte,
                    end_byte: f.end_byte,
                    start_line: f.start_line,
                    start_col: f.start_col,
                    end_line: f.end_line,
                    end_col: f.end_col,
                },
                signature: f.signature.clone(),
                body: f.body.clone(),
                doc: f.doc.clone(),
                retrieval_text: f.retrieval_text.clone(),
                refs: f.refs.clone(),
            };
            let rowid = db.upsert_fragment(file_id, &frag)?;
            db.replace_symbols_for_fragment(rowid, &frag)?;
            db.replace_refs_for_fragment(rowid, &frag.refs)?;
            if let Some(vec) = &f.embedding {
                let dim = vec.len() as i64;
                let blob: Vec<u8> = bytemuck::cast_slice::<f32, u8>(vec).to_vec();
                db.insert_embedding(rowid, "unknown", dim, &blob)?;
            }
        }
        Ok(())
    }

    async fn upsert_edges(&self, edges: &[EdgeRecord]) -> Result<()> {
        let db = self.open()?;
        for e in edges {
            let from = db.get_fragment_by_id(&e.from_id)?.map(|r| r.0);
            let to = db.get_fragment_by_id(&e.to_id)?.map(|r| r.0);
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            db.upsert_edge(from, to, &e.edge_type, e.weight)?;
        }
        Ok(())
    }

    async fn delete_missing_files(&self, repo_id: &str, keep_file_ids: &[String]) -> Result<usize> {
        let db = self.open()?;
        let mut removed = 0usize;
        let mut stmt = db.conn().prepare("SELECT file_id, path FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for rr in rows {
            let (file_id, path) = rr?;
            let fid = file_id_for_path(repo_id, &path);
            if !keep_file_ids.contains(&fid) {
                db.delete_file_by_id(file_id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    async fn delete_missing_fragments(
        &self,
        _repo_id: &str,
        keep_frag_ids: &[String],
    ) -> Result<usize> {
        let db = self.open()?;
        if keep_frag_ids.is_empty() {
            let deleted = db.conn().execute("DELETE FROM fragments", [])?;
            return Ok(deleted as usize);
        }
        let mut deleted = 0usize;
        let mut stmt = db.conn().prepare("SELECT frag_id FROM fragments")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for rr in rows {
            let fid = rr?;
            if !keep_frag_ids.contains(&fid) {
                let n = db
                    .conn()
                    .execute("DELETE FROM fragments WHERE frag_id=?1", params![fid])?;
                deleted += n as usize;
            }
        }
        Ok(deleted)
    }

    async fn vector_search(
        &self,
        _repo_id: &str,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        let db = self.open()?;
        let vec_index =
            query::load_or_build_hnsw(&db, &self.hnsw_dir, query::DEFAULT_HNSW_BASE, false)?;
        let nn = vec_index.search(query_vec, k, 64);
        let mut ranked: Vec<(i64, f32)> = Vec::new();
        for n in nn {
            let sim = 1.0 - (n.distance as f32);
            ranked.push((n.d_id as i64, sim.max(0.0)));
        }
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(k);
        let rowids: Vec<i64> = ranked.iter().map(|(rid, _)| *rid).collect();
        let mut hits = db.search_hits_by_rowids(&rowids)?;
        let score_map: HashMap<i64, f32> = ranked.into_iter().collect();
        for h in hits.iter_mut() {
            h.score = *score_map.get(&h.rowid).unwrap_or(&0.0);
        }
        Ok(self.map_hits(hits, "vector"))
    }

    async fn fts_search(
        &self,
        _repo_id: &str,
        query_text: &str,
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        let db = self.open()?;
        let rowids = db.search_fts(query_text, k)?;
        let rowids_only: Vec<i64> = rowids.into_iter().map(|(rid, _)| rid).collect();
        let hits = db.search_hits_by_rowids(&rowids_only)?;
        Ok(self.map_hits(hits, "fts"))
    }

    async fn hybrid_search_rrf(
        &self,
        repo_id: &str,
        query_text: &str,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<SearchHit>> {
        let vec_hits = self.vector_search(repo_id, query_vec, k).await?;
        let fts_hits = self.fts_search(repo_id, query_text, k).await?;
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
        repo_id: &str,
        frag_ids: &[String],
    ) -> Result<Vec<FragmentRecord>> {
        let db = self.open()?;
        let mut out = Vec::new();
        for fid in frag_ids {
            if let Some((_rowid, frag)) = db.get_fragment_by_id(fid)? {
                let mut rec = Self::frag_record_from_db(repo_id, frag);
                if let Some((_, _dim, blob)) = db.get_embedding_blob(_rowid)? {
                    let vec: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&blob).to_vec();
                    rec.embedding = Some(vec);
                }
                out.push(rec);
            }
        }
        Ok(out)
    }

    async fn expand_graph(
        &self,
        _repo_id: &str,
        seed_ids: &[String],
        edge_types: &[String],
        max_nodes: usize,
    ) -> Result<Vec<String>> {
        let db = self.open()?;
        let mut visited: HashSet<String> = HashSet::new();
        let mut q: VecDeque<String> = VecDeque::new();

        for s in seed_ids {
            visited.insert(s.clone());
            q.push_back(s.clone());
        }

        while let Some(node) = q.pop_front() {
            if visited.len() >= max_nodes {
                break;
            }
            let Some((rowid, _)) = db.get_fragment_by_id(&node)? else {
                continue;
            };
            let outs = db.edges_outgoing(rowid, 128)?;
            let ins = db.edges_incoming(rowid, 128)?;
            let mut neighbors: Vec<i64> = Vec::new();
            for (to, ty, _) in outs {
                if edge_types.is_empty() || edge_types.contains(&ty) {
                    neighbors.push(to);
                }
            }
            for (from, ty, _) in ins {
                if edge_types.is_empty() || edge_types.contains(&ty) {
                    neighbors.push(from);
                }
            }
            let frag_ids = Self::rowids_to_frag_ids(&db, &neighbors)?;
            for fid in frag_ids {
                if visited.insert(fid.clone()) {
                    q.push_back(fid);
                }
                if visited.len() >= max_nodes {
                    break;
                }
            }
        }

        Ok(visited.into_iter().collect())
    }

    async fn pack(&self, _req: PackRequest) -> Result<PackResult> {
        Err(anyhow!("SqliteStore pack not implemented"))
    }

    async fn list_files(&self, repo_id: &str) -> Result<Vec<FileRecord>> {
        let db = self.open()?;
        let mut out = Vec::new();
        let mut stmt = db
            .conn()
            .prepare("SELECT path, language, size_bytes, mtime_ms, content_hash FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for rr in rows {
            let (path, lang, size_bytes, mtime_ms, content_hash) = rr?;
            out.push(FileRecord {
                file_id: file_id_for_path(repo_id, &path),
                repo_id: repo_id.to_string(),
                path,
                lang,
                size_bytes,
                mtime_ms,
                content_hash,
            });
        }
        Ok(out)
    }

    async fn get_file_by_path(&self, repo_id: &str, path: &str) -> Result<Option<FileRecord>> {
        let db = self.open()?;
        let mut stmt = db.conn().prepare(
            "SELECT language, size_bytes, mtime_ms, content_hash FROM files WHERE path=?1",
        )?;
        let row = stmt.query_row(params![path], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        });
        match row {
            Ok((lang, size_bytes, mtime_ms, content_hash)) => Ok(Some(FileRecord {
                file_id: file_id_for_path(repo_id, path),
                repo_id: repo_id.to_string(),
                path: path.to_string(),
                lang,
                size_bytes,
                mtime_ms,
                content_hash,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
