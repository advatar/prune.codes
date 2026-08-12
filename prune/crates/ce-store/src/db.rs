use anyhow::{anyhow, Result};
use ce_core::model::{FragKind, Fragment};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use blake3::Hasher;

use crate::types::{RecipeRecord, RepositoryMemoryRecord, SearchHit, StrategyRecord};

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct EmbeddingsMeta {
    pub total_count: usize,
    pub model: String,
    pub dim: usize,
    pub max_created_at_ms: i64,
    pub state_hash: String,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn migrate(&self) -> Result<()> {
        // v1 schema
        let sql1 = include_str!("../migrations/001_init.sql");
        if let Err(err) = self.conn.execute_batch(sql1) {
            let fallback_ok = matches!(
                &err,
                rusqlite::Error::SqliteFailure(_, Some(msg))
                    if msg.contains("tokenize") || msg.contains("tokenizer")
            );
            if fallback_ok {
                let rich = r#"tokenize = 'unicode61 tokenchars "_-"'"#;
                if sql1.contains(rich) {
                    let fallback_sql = sql1.replace(rich, "tokenize = 'unicode61'");
                    self.conn.execute_batch(&fallback_sql)?;
                } else {
                    return Err(err.into());
                }
            } else {
                return Err(err.into());
            }
        }

        // Lightweight migration system based on a single `meta.schema_version` key.
        //
        // This repo started as a single init migration. As we iterate quickly, we
        // keep migrations simple and additive.
        let ver_str: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let mut ver: i64 = ver_str.as_deref().unwrap_or("1").parse().unwrap_or(1);

        if ver < 2 {
            let sql2 = include_str!("../migrations/002_add_crate.sql");
            self.conn.execute_batch(sql2)?;
            ver = 2;
            self.conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![ver.to_string()],
            )?;
        }

        if ver < 3 {
            let sql3 = include_str!("../migrations/003_add_symbol_source.sql");
            self.conn.execute_batch(sql3)?;
            ver = 3;
            self.conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![ver.to_string()],
            )?;

            // Best-effort: classify existing symbol rows into `source` categories.
            // This enables clean rebuild of generated aliases without requiring a full reindex.
            let _ = self.backfill_symbol_sources();
        }
        if ver < 4 {
            let sql4 = include_str!("../migrations/004_add_recipes.sql");
            self.conn.execute_batch(sql4)?;
            ver = 4;
            self.conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![ver.to_string()],
            )?;
        }
        if ver < 5 {
            let sql5 = include_str!("../migrations/005_add_repository_memory.sql");
            self.conn.execute_batch(sql5)?;
            ver = 5;
            self.conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![ver.to_string()],
            )?;
        }

        Ok(())
    }

    pub fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    // ---------------------------------------------------------------------
    // Meta (key/value)
    // ---------------------------------------------------------------------

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(v)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn backfill_symbol_sources(&self) -> Result<()> {
        // If the `source` column isn't present for some reason, just no-op.
        // (This can happen if a caller runs against a partially-migrated DB.)
        let mut stmt = match self.conn.prepare(
            r#"
            SELECT s.symbol, s.frag_rowid, f.symbol, files.crate_name
            FROM symbols s
            JOIN fragments f ON f.rowid = s.frag_rowid
            JOIN files ON files.file_id = f.file_id
            "#,
        ) {
            Ok(s) => s,
            Err(_) => return Ok(()),
        };

        let mut upd = match self
            .conn
            .prepare("UPDATE symbols SET source=?1 WHERE symbol=?2 AND frag_rowid=?3")
        {
            Ok(u) => u,
            Err(_) => return Ok(()),
        };

        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,         // symbol row value
                r.get::<_, i64>(1)?,            // frag_rowid
                r.get::<_, Option<String>>(2)?, // fragment primary symbol
                r.get::<_, String>(3)?,         // crate_name
            ))
        })?;

        for rr in rows {
            let (sym, rid, frag_sym, crate_name) = rr?;
            let src = classify_symbol_source(&sym, frag_sym.as_deref(), &crate_name);
            let _ = upd.execute(params![src, sym, rid]);
        }

        Ok(())
    }

    pub fn set_meta_i64(&self, key: &str, value: i64) -> Result<()> {
        self.set_meta(key, &value.to_string())
    }

    pub fn set_meta_usize(&self, key: &str, value: usize) -> Result<()> {
        self.set_meta(key, &value.to_string())
    }

    /// Compute a stable-ish hash of the indexed repo state (based on the `files` table).
    ///
    /// This is cheap to compute (compared to hashing all embeddings) and changes whenever
    /// any file's `content_hash` changes.
    pub fn compute_repo_state_hash(&self) -> Result<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, language, content_hash, crate_name FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut hasher = Hasher::new();
        for rr in rows {
            let (path, lang, h, crate_name) = rr?;
            let path_norm = path.replace('\\', "/");
            hasher.update(path_norm.as_bytes());
            hasher.update(b"\0");
            hasher.update(lang.as_bytes());
            hasher.update(b"\0");
            hasher.update(h.as_bytes());
            hasher.update(b"\0");
            hasher.update(crate_name.as_bytes());
            hasher.update(b"\n");
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Update and persist repo state metadata.
    ///
    /// Returns the computed hash.
    pub fn update_repo_meta(&self) -> Result<String> {
        let h = self.compute_repo_state_hash()?;
        self.set_meta("repo.state_hash", &h)?;
        self.set_meta_i64("repo.updated_at_ms", Self::now_ms())?;
        Ok(h)
    }

    /// Compute summary metadata about the current embeddings and store it in `meta`.
    ///
    /// This provides a fast, durable staleness check for persisted HNSW dumps.
    pub fn update_embeddings_meta(&self) -> Result<EmbeddingsMeta> {
        // Total embeddings
        let total_count = self.embedding_count()?;

        // Primary (model, dim) group by count.
        let row: Option<(String, i64, i64, i64)> = self
            .conn
            .query_row(
                r#"
                SELECT model, dim, COUNT(*) AS c, MAX(created_at_ms) AS mx
                FROM embeddings
                GROUP BY model, dim
                ORDER BY c DESC
                LIMIT 1
                "#,
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        let (model, dim, _group_count, max_created_at_ms) =
            row.unwrap_or(("".to_string(), 0, 0, 0));
        let dim_usize = dim.max(0) as usize;

        // Fast “state hash” derived from (model, dim, total_count, max_created_at).
        // This is not a cryptographic commitment to all vectors, but it's a robust and cheap
        // staleness indicator for local indexes.
        let state = format!(
            "model={};dim={};count={};max_created_at_ms={}",
            model, dim_usize, total_count, max_created_at_ms
        );
        let state_hash = {
            let mut hasher = Hasher::new();
            hasher.update(state.as_bytes());
            hasher.finalize().to_hex().to_string()
        };

        // Persist meta
        self.set_meta("embeddings.model", &model)?;
        self.set_meta_usize("embeddings.dim", dim_usize)?;
        self.set_meta_usize("embeddings.count", total_count)?;
        self.set_meta_i64("embeddings.max_created_at_ms", max_created_at_ms)?;
        self.set_meta("embeddings.state_hash", &state_hash)?;
        self.set_meta_i64("embeddings.updated_at_ms", Self::now_ms())?;

        Ok(EmbeddingsMeta {
            total_count,
            model,
            dim: dim_usize,
            max_created_at_ms,
            state_hash,
        })
    }

    /// Count how many embedding vectors we currently have.
    ///
    /// This is useful for validating whether an on-disk HNSW dump is stale.
    pub fn embedding_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
        Ok(n.max(0) as usize)
    }

    pub fn upsert_file(
        &self,
        path: &str,
        language: &str,
        size_bytes: i64,
        mtime_ms: i64,
        content_hash: &str,
    ) -> Result<i64> {
        let now = Self::now_ms();
        self.conn.execute(
            r#"
            INSERT INTO files(path, language, size_bytes, mtime_ms, content_hash, updated_at_ms)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(path) DO UPDATE SET
              language=excluded.language,
              size_bytes=excluded.size_bytes,
              mtime_ms=excluded.mtime_ms,
              content_hash=excluded.content_hash,
              updated_at_ms=excluded.updated_at_ms
            "#,
            params![path, language, size_bytes, mtime_ms, content_hash, now],
        )?;

        let file_id: i64 = self.conn.query_row(
            "SELECT file_id FROM files WHERE path=?1",
            params![path],
            |r| r.get(0),
        )?;
        Ok(file_id)
    }

    /// Update the best-effort owning crate name for a file.
    ///
    /// For non-Rust files or when crate resolution is unknown, callers should
    /// pass an empty string.
    pub fn update_file_crate_name(&self, file_id: i64, crate_name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET crate_name=?2 WHERE file_id=?1",
            params![file_id, crate_name],
        )?;
        Ok(())
    }

    /// Fetch the crate name for a fragment rowid (best-effort).
    pub fn crate_name_for_fragment_rowid(&self, frag_rowid: i64) -> Result<Option<String>> {
        let row: Option<String> = self
            .conn
            .query_row(
                r#"
                SELECT files.crate_name
                FROM fragments
                JOIN files ON fragments.file_id = files.file_id
                WHERE fragments.rowid=?1
                "#,
                params![frag_rowid],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row)
    }

    /// Fetch the existing (file_id, content_hash) for a file path, if present.
    pub fn get_file_info(&self, path: &str) -> Result<Option<(i64, String)>> {
        let row: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT file_id, content_hash FROM files WHERE path=?1",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// Delete all fragments for a given file_id.
    ///
    /// Cascades to embeddings/symbols/refs/edges via foreign keys, and keeps
    /// FTS in sync via triggers.
    pub fn delete_fragments_by_file_id(&self, file_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM fragments WHERE file_id=?1", params![file_id])?;
        Ok(())
    }

    /// List all indexed files for a given language.
    pub fn list_files_by_language(&self, language: &str) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_id, path FROM files WHERE language=?1")?;
        let rows = stmt.query_map(params![language], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    /// Delete a file row by id (cascades to fragments).
    pub fn delete_file_by_id(&self, file_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE file_id=?1", params![file_id])?;
        Ok(())
    }

    pub fn upsert_fragment(&self, file_id: i64, frag: &Fragment) -> Result<i64> {
        let now = Self::now_ms();

        let kind_str = format!("{:?}", frag.kind);
        let symbol = frag.symbol.as_deref().unwrap_or("");

        self.conn.execute(
            r#"
            INSERT INTO fragments
              (frag_id, ast_hash, file_id, path, kind, symbol,
               start_byte, end_byte, start_line, start_col, end_line, end_col,
               signature, body, doc, retrieval_text, updated_at_ms)
            VALUES
              (?1, ?2, ?3, ?4, ?5, NULLIF(?6, ''),
               ?7, ?8, ?9, ?10, ?11, ?12,
               ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(frag_id) DO UPDATE SET
              ast_hash=excluded.ast_hash,
              file_id=excluded.file_id,
              path=excluded.path,
              kind=excluded.kind,
              symbol=excluded.symbol,
              start_byte=excluded.start_byte,
              end_byte=excluded.end_byte,
              start_line=excluded.start_line,
              start_col=excluded.start_col,
              end_line=excluded.end_line,
              end_col=excluded.end_col,
              signature=excluded.signature,
              body=excluded.body,
              doc=excluded.doc,
              retrieval_text=excluded.retrieval_text,
              updated_at_ms=excluded.updated_at_ms
            "#,
            params![
                frag.id,
                frag.ast_hash,
                file_id,
                frag.file.display().to_string(),
                kind_str,
                symbol,
                frag.span.start_byte as i64,
                frag.span.end_byte as i64,
                frag.span.start_line as i64,
                frag.span.start_col as i64,
                frag.span.end_line as i64,
                frag.span.end_col as i64,
                frag.signature,
                frag.body,
                frag.doc,
                frag.retrieval_text,
                now
            ],
        )?;

        let rowid: i64 = self.conn.query_row(
            "SELECT rowid FROM fragments WHERE frag_id=?1",
            params![frag.id],
            |r| r.get(0),
        )?;
        Ok(rowid)
    }

    pub fn replace_symbols_for_fragment(&self, frag_rowid: i64, frag: &Fragment) -> Result<()> {
        // Delete existing
        self.conn.execute(
            "DELETE FROM symbols WHERE frag_rowid=?1",
            params![frag_rowid],
        )?;

        let kind = format!("{:?}", frag.kind);
        let path = frag.file.display().to_string();

        if let Some(sym) = &frag.symbol {
            // Primary symbol (possibly qualified, e.g. `Type::method`).
            self.conn.execute(
                "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![sym, frag_rowid, kind, path, "extracted"],
            )?;

            // Alias: unqualified tail segment for qualified names.
            // This improves definition lookup when refs only contain the short name.
            if let Some((_, tail)) = sym.rsplit_once("::") {
                let tail = tail.trim();
                if !tail.is_empty() && tail != sym {
                    self.conn.execute(
                        "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![tail, frag_rowid, kind, path, "alias_tail"],
                    )?;
                }
            }

            // Best-effort module-qualified aliases for Rust sources under `src/`.
            //
            // Example: `src/foo.rs` defining `Bar` also becomes available as:
            // - `foo::Bar`
            // - `crate::foo::Bar`
            //
            // This significantly improves resolution for refs like `foo::Bar` and `crate::foo::Bar`
            // without requiring rust-analyzer.
            if let Some(mod_prefix) = rust_module_prefix_for_index_path(&path) {
                if !mod_prefix.is_empty() {
                    let q = format!("{}::{}", mod_prefix, sym);
                    self.conn.execute(
                        "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![q, frag_rowid, kind, path, "alias_module"],
                    )?;
                    let cq = format!("crate::{}", q);
                    self.conn.execute(
                        "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![cq, frag_rowid, kind, path, "alias_crate"],
                    )?;
                } else {
                    // Root-level items in `lib.rs`/`main.rs` can still be referenced as `crate::Item`.
                    let cq = format!("crate::{}", sym);
                    self.conn.execute(
                        "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![cq, frag_rowid, kind, path, "alias_crate"],
                    )?;
                }
            }
        }

        Ok(())
    }

    pub fn replace_refs_for_fragment(&self, frag_rowid: i64, refs: &[String]) -> Result<()> {
        self.conn
            .execute("DELETE FROM refs WHERE from_rowid=?1", params![frag_rowid])?;
        for r in refs {
            self.conn.execute(
                "INSERT OR IGNORE INTO refs(from_rowid, ref_text) VALUES (?1, ?2)",
                params![frag_rowid, r],
            )?;
        }
        Ok(())
    }

    /// Delete all edges that originate from `from_rowid`.
    pub fn delete_edges_from(&self, from_rowid: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM edges WHERE from_rowid=?1", params![from_rowid])?;
        Ok(())
    }

    /// Insert (or replace) a resolved edge between fragments.
    pub fn upsert_edge(
        &self,
        from_rowid: i64,
        to_rowid: i64,
        edge_type: &str,
        weight: f32,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT OR REPLACE INTO edges(from_rowid, to_rowid, edge_type, weight)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![from_rowid, to_rowid, edge_type, weight as f64],
        )?;
        Ok(())
    }

    /// Return up to `k` outgoing edges for a fragment.
    ///
    /// Each tuple is (to_rowid, edge_type, weight).
    pub fn edges_outgoing(&self, from_rowid: i64, k: usize) -> Result<Vec<(i64, String, f32)>> {
        if k == 0 {
            return Ok(vec![]);
        }
        let mut stmt = self.conn.prepare(
            r#"
            SELECT to_rowid, edge_type, weight
            FROM edges
            WHERE from_rowid=?1
            ORDER BY weight DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![from_rowid, k as i64], |r| {
            let to: i64 = r.get(0)?;
            let ty: String = r.get(1)?;
            let w: f32 = r.get::<_, f64>(2)? as f32;
            Ok((to, ty, w))
        })?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    /// Return up to `k` incoming edges for a fragment.
    ///
    /// Each tuple is (from_rowid, edge_type, weight).
    pub fn edges_incoming(&self, to_rowid: i64, k: usize) -> Result<Vec<(i64, String, f32)>> {
        if k == 0 {
            return Ok(vec![]);
        }
        let mut stmt = self.conn.prepare(
            r#"
            SELECT from_rowid, edge_type, weight
            FROM edges
            WHERE to_rowid=?1
            ORDER BY weight DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![to_rowid, k as i64], |r| {
            let from: i64 = r.get(0)?;
            let ty: String = r.get(1)?;
            let w: f32 = r.get::<_, f64>(2)? as f32;
            Ok((from, ty, w))
        })?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    /// Rebuild resolved reference edges for all fragments.
    ///
    /// Edges are derived from the `refs` and `symbols` tables:
    /// - for each fragment (from_rowid)
    /// - take up to `max_refs_per_fragment` referenced identifiers
    /// - resolve each identifier to up to `max_defs_per_ref` definition fragments
    /// - insert edges of type `refers`
    ///
    /// This is an MVP implementation intended to make edge-based subgraph
    /// expansion possible. For very large repos you may want incremental
    /// rebuilding (only affected rowids) or a SQL-join based bulk build.
    pub fn rebuild_ref_edges_all(
        &self,
        max_refs_per_fragment: usize,
        max_defs_per_ref: usize,
    ) -> Result<usize> {
        let max_refs_per_fragment = max_refs_per_fragment.max(1);
        let max_defs_per_ref = max_defs_per_ref.max(1);

        // Use an explicit transaction for speed.
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut committed = false;

        let res: Result<usize> = (|| {
            // Keep other edge types (e.g. module/import edges). Only rebuild `refers` here.
            self.conn
                .execute("DELETE FROM edges WHERE edge_type='refers'", [])?;

            // Prepare statements used in the loop.
            let mut stmt_frags = self.conn.prepare(
                r#"
                SELECT fragments.rowid, fragments.path, files.crate_name
                FROM fragments
                JOIN files ON fragments.file_id = files.file_id
                "#,
            )?;
            let frag_rows = stmt_frags.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;

            let mut stmt_refs = self
                .conn
                .prepare(r#"SELECT ref_text FROM refs WHERE from_rowid=?1 LIMIT ?2"#)?;
            let mut stmt_defs = self.conn.prepare(
                r#"
                SELECT DISTINCT fragments.rowid, fragments.path, fragments.kind, fragments.symbol, files.crate_name
                FROM symbols
                JOIN fragments ON symbols.frag_rowid = fragments.rowid
                JOIN files ON fragments.file_id = files.file_id
                WHERE symbols.symbol=?1
                LIMIT ?2
                "#,
            )?;
            let mut stmt_ins = self.conn.prepare(
                r#"INSERT OR IGNORE INTO edges(from_rowid, to_rowid, edge_type, weight)
                   VALUES (?1, ?2, 'refers', ?3)"#,
            )?;

            let mut edge_count: usize = 0;

            for rr in frag_rows {
                let (from_rowid, from_path, from_crate) = rr?;

                // Collect refs
                let refs_iter = stmt_refs.query_map(
                    params![from_rowid, (max_refs_per_fragment * 6) as i64],
                    |r| r.get::<_, String>(0),
                )?;
                let mut refs: Vec<String> = Vec::new();
                for rrr in refs_iter {
                    let s = rrr?;
                    if is_good_ref_for_edges(&s) {
                        refs.push(s);
                    }
                }
                refs.sort();
                refs.dedup();
                refs.truncate(max_refs_per_fragment);

                for r in refs {
                    let lim = ((max_defs_per_ref * 12).max(24)).min(240) as i64;
                    // Qualified refs like `foo::Bar` won't usually exist as-is in the
                    // symbols table (definitions are often stored as `Bar`). Query both
                    // the full ref and its tail segment, but score using the full ref so
                    // module-path hints can disambiguate.
                    let mut variants: Vec<&str> = vec![&r];
                    if r.contains("::") {
                        let tail = last_segment(&r);
                        if tail != r {
                            variants.push(tail);
                        }
                    }

                    let mut cands: Vec<(i64, f64)> = Vec::new();
                    let mut seen_defs: HashSet<i64> = HashSet::new();
                    for qsym in variants {
                        let defs_iter = stmt_defs.query_map(params![qsym, lim], |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        })?;

                        for drr in defs_iter {
                            let (to_rowid, to_path, kind_s, sym_opt, to_crate) = drr?;
                            if to_rowid == from_rowid {
                                continue;
                            }
                            if !seen_defs.insert(to_rowid) {
                                continue;
                            }
                            let Some(kind) = parse_kind(&kind_s) else {
                                continue;
                            };
                            if kind == FragKind::ApiSummary {
                                continue;
                            }
                            let w = score_def_candidate(
                                &from_path,
                                &from_crate,
                                &r,
                                &to_path,
                                &to_crate,
                                kind,
                                sym_opt.as_deref(),
                            );
                            if w <= 0.0 {
                                continue;
                            }
                            cands.push((to_rowid, w));
                        }
                    }

                    // Best K by weight.
                    cands
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    cands.truncate(max_defs_per_ref);

                    for (to_rowid, w) in cands {
                        stmt_ins.execute(params![from_rowid, to_rowid, w])?;
                        edge_count += 1;
                    }
                }
            }

            Ok(edge_count)
        })();

        match res {
            Ok(n) => {
                self.conn.execute_batch("COMMIT;")?;
                committed = true;
                Ok(n)
            }
            Err(e) => {
                if !committed {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                Err(e)
            }
        }
    }

    /// Incrementally rebuild `edges` for a subset of the repo.
    ///
    /// This is a performance optimization for iterative agent loops:
    /// - When only a small set of files changed, a full rebuild of all resolved
    ///   edges can dominate indexing time.
    /// - However, correctness requires more than “outgoing edges from changed
    ///   fragments” because changed fragments may DEFINE symbols that other
    ///   fragments reference.
    ///
    /// Strategy:
    /// 1) Find fragments in changed files.
    /// 2) Find symbols defined by those fragments.
    /// 3) Find all fragments that reference any of those symbols.
    /// 4) Rebuild outgoing edges for the union of (1) and (3).
    ///
    /// Edges are of type `refers` and have a heuristic weight (>0) that
    /// biases toward definitions in the same file / directory and kind matches.
    pub fn rebuild_ref_edges_incremental(
        &self,
        changed_file_ids: &[i64],
        max_refs_per_fragment: usize,
        max_defs_per_ref: usize,
    ) -> Result<usize> {
        let max_refs_per_fragment = max_refs_per_fragment.max(1);
        let max_defs_per_ref = max_defs_per_ref.max(1);

        if changed_file_ids.is_empty() {
            return Ok(0);
        }

        // 1) fragments in changed files
        let changed_frags = self.fragment_rowids_for_file_ids(changed_file_ids)?;
        if changed_frags.is_empty() {
            return Ok(0);
        }

        // 2) symbols defined in changed fragments (filter stop-refs to avoid huge fanout)
        let mut changed_syms = self.symbols_for_frag_rowids(&changed_frags)?;
        changed_syms.retain(|s| is_good_ref_for_edges(s));
        // De-dup to keep param sets small.
        changed_syms.sort();
        changed_syms.dedup();

        // 3) fragments referencing changed symbols
        let mut affected_from: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for &rid in &changed_frags {
            affected_from.insert(rid);
        }

        if !changed_syms.is_empty() {
            let refs = self.from_rowids_referencing_symbols(&changed_syms)?;
            for rid in refs {
                affected_from.insert(rid);
            }
        }

        let mut affected: Vec<i64> = affected_from.into_iter().collect();
        affected.sort();

        // Use an explicit transaction for speed.
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut committed = false;

        let res: Result<usize> = (|| {
            // Delete outgoing `refers` edges for affected fragments (keep other edge types).
            self.delete_edges_from_rowids_by_type(&affected, "refers")?;

            let mut stmt_get_path = self.conn.prepare(
                r#"
                SELECT fragments.path, files.crate_name
                FROM fragments
                JOIN files ON fragments.file_id = files.file_id
                WHERE fragments.rowid=?1
                "#,
            )?;

            let mut stmt_refs = self
                .conn
                .prepare(r#"SELECT ref_text FROM refs WHERE from_rowid=?1 LIMIT ?2"#)?;
            let mut stmt_defs = self.conn.prepare(
                r#"
                SELECT DISTINCT fragments.rowid, fragments.path, fragments.kind, fragments.symbol, files.crate_name
                FROM symbols
                JOIN fragments ON symbols.frag_rowid = fragments.rowid
                JOIN files ON fragments.file_id = files.file_id
                WHERE symbols.symbol=?1
                LIMIT ?2
                "#,
            )?;
            let mut stmt_ins = self.conn.prepare(
                r#"INSERT OR IGNORE INTO edges(from_rowid, to_rowid, edge_type, weight)
                   VALUES (?1, ?2, 'refers', ?3)"#,
            )?;

            let mut edge_count: usize = 0;

            for &from_rowid in &affected {
                let (from_path, from_crate): (String, String) =
                    stmt_get_path.query_row(params![from_rowid], |r| Ok((r.get(0)?, r.get(1)?)))?;
                // Collect refs
                let refs_iter = stmt_refs.query_map(
                    params![from_rowid, (max_refs_per_fragment * 6) as i64],
                    |r| r.get::<_, String>(0),
                )?;
                let mut refs: Vec<String> = Vec::new();
                for rrr in refs_iter {
                    let s = rrr?;
                    if is_good_ref_for_edges(&s) {
                        refs.push(s);
                    }
                }
                refs.sort();
                refs.dedup();
                refs.truncate(max_refs_per_fragment);

                for r in refs {
                    let lim = ((max_defs_per_ref * 12).max(24)).min(240) as i64;
                    // Qualified refs like `foo::Bar` won't usually exist as-is in the
                    // symbols table (definitions are often stored as `Bar`). Query both
                    // the full ref and its tail segment, but score using the full ref so
                    // module-path hints can disambiguate.
                    let mut variants: Vec<&str> = vec![&r];
                    if r.contains("::") {
                        let tail = last_segment(&r);
                        if tail != r {
                            variants.push(tail);
                        }
                    }

                    let mut cands: Vec<(i64, f64)> = Vec::new();
                    let mut seen_defs: HashSet<i64> = HashSet::new();
                    for qsym in variants {
                        let defs_iter = stmt_defs.query_map(params![qsym, lim], |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        })?;

                        for drr in defs_iter {
                            let (to_rowid, to_path, kind_s, sym_opt, to_crate) = drr?;
                            if to_rowid == from_rowid {
                                continue;
                            }
                            if !seen_defs.insert(to_rowid) {
                                continue;
                            }
                            let Some(kind) = parse_kind(&kind_s) else {
                                continue;
                            };
                            if kind == FragKind::ApiSummary {
                                continue;
                            }
                            let w = score_def_candidate(
                                &from_path,
                                &from_crate,
                                &r,
                                &to_path,
                                &to_crate,
                                kind,
                                sym_opt.as_deref(),
                            );
                            if w <= 0.0 {
                                continue;
                            }
                            cands.push((to_rowid, w));
                        }
                    }

                    cands
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    cands.truncate(max_defs_per_ref);

                    for (to_rowid, w) in cands {
                        stmt_ins.execute(params![from_rowid, to_rowid, w])?;
                        edge_count += 1;
                    }
                }
            }

            Ok(edge_count)
        })();

        match res {
            Ok(n) => {
                self.conn.execute_batch("COMMIT;")?;
                committed = true;
                Ok(n)
            }
            Err(e) => {
                if !committed {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                Err(e)
            }
        }
    }

    /// Rebuild Rust module/import edges at the file level.
    ///
    /// These edges are inserted **between ApiSummary fragments** (one per file)
    /// and capture lightweight relationships:
    /// - `mod` => `mod foo;` / `pub mod foo;`
    /// - `use` => `use crate::...` / `use self::...` / `use super::...` (and workspace-member `use other_crate::...`)
    /// - reverse edges (explainability): `imported_by` / `modded_by`
    ///
    /// The goal is not perfect Rust name resolution (that would require a full Rust compiler / rust-analyzer).
    /// When available, we use Cargo metadata to improve multi-crate module resolution in workspaces.
    ///
    /// This is intentionally approximate, but it helps retrieval explain *why* a file is connected to another.
    pub fn rebuild_rust_module_edges_all(&self, repo_root: &Path) -> Result<usize> {
        // Collect all indexed Rust file paths (repo-relative).
        let files = self.list_files_by_language("rust")?;
        if files.is_empty() {
            return Ok(0);
        }

        let all_paths: HashSet<String> = files.iter().map(|(_, p)| p.clone()).collect();
        let api_map = self.api_summary_map_all()?;

        let ws = load_rust_workspace(repo_root, &all_paths);

        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut committed = false;

        let res: Result<usize> = (|| {
            // Remove old module/import edges, keep other edge types (e.g. `refers`).
            self.conn.execute(
                "DELETE FROM edges WHERE edge_type IN ('mod', 'use', 'imported_by', 'modded_by')",
                [],
            )?;

            let mut stmt_upd_crate = self
                .conn
                .prepare("UPDATE files SET crate_name=?2 WHERE file_id=?1")?;

            let mut stmt_ins = self.conn.prepare(
                r#"INSERT OR IGNORE INTO edges(from_rowid, to_rowid, edge_type, weight)
                   VALUES (?1, ?2, ?3, ?4)"#,
            )?;

            let mut edge_count: usize = 0;

            for (file_id, path) in files.iter() {
                // Ensure module-qualified symbol aliases exist for this file.
                // This upgrades older indexes without requiring a full reindex.
                let _ = self.augment_rust_module_qualified_symbols_for_file(*file_id, path)?;
                let Some(&from_rid) = api_map.get(path) else {
                    continue;
                };

                // Best-effort: annotate this file with its owning Rust crate name (workspace-aware).
                let tgt = best_rust_target_for_file(Path::new(path), &ws);
                let crate_name = tgt.map(|t| t.crate_name.clone()).unwrap_or_default();
                let is_lib = tgt.map(|t| t.kind == RustTargetKind::Lib).unwrap_or(false);
                stmt_upd_crate.execute(params![file_id, &crate_name])?;

                // Also add crate-qualified symbol aliases for lib/proc-macro targets so that
                // refs like `my_crate::foo::Bar` can resolve directly.
                if is_lib && !crate_name.is_empty() {
                    let _ = self.augment_rust_crate_qualified_symbols_for_file(
                        *file_id,
                        path,
                        &crate_name,
                    )?;
                }

                // Read source from disk.
                let disk_path = repo_root.join(Path::new(path));
                let src = match fs::read_to_string(&disk_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let crate_mod_dir = crate_module_dir_for_file(Path::new(path), &ws, &all_paths);

                let targets =
                    rust_module_targets(&src, Path::new(path), &all_paths, &ws, &crate_mod_dir);
                for (edge_type, target_path, weight) in targets {
                    let Some(&to_rid) = api_map.get(&target_path) else {
                        continue;
                    };
                    if to_rid == from_rid {
                        continue;
                    }
                    stmt_ins.execute(params![from_rid, to_rid, edge_type, weight])?;
                    edge_count += 1;

                    // Optional reverse edges for explainability (and for expansion even when
                    // only outgoing edges are enabled).
                    if edge_type == "use" {
                        let w2 = weight * 0.9;
                        stmt_ins.execute(params![to_rid, from_rid, "imported_by", w2])?;
                        edge_count += 1;
                    } else if edge_type == "mod" {
                        let w2 = weight * 0.9;
                        stmt_ins.execute(params![to_rid, from_rid, "modded_by", w2])?;
                        edge_count += 1;
                    }
                }
            }

            Ok(edge_count)
        })();

        match res {
            Ok(n) => {
                self.conn.execute_batch("COMMIT;")?;
                committed = true;
                Ok(n)
            }
            Err(e) => {
                if !committed {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                Err(e)
            }
        }
    }

    /// Rebuild TypeScript/TSX import edges between file-level `ApiSummary` fragments.
    ///
    /// This is a lightweight graph that connects local (repo) modules based on
    /// `import ... from "..."` and `export ... from "..."` specifiers.
    ///
    /// Edge types:
    /// - `ts_import`: from importing file → imported file
    /// - `ts_imported_by`: reverse edge for explainability
    /// - `jsx_uses` / `jsx_used_by`: high-signal edges from TSX fragments to imported
    ///   components they actually use as JSX tags.
    ///
    /// Only *relative* imports and tsconfig path aliases are resolved.
    /// Non-relative specifiers (npm packages) are ignored (no local target).
    pub fn rebuild_ts_module_edges_all(&self, repo_root: &Path) -> Result<usize> {
        // Collect all indexed TS/TSX file paths (repo-relative).
        let mut ts_files = self.list_files_by_language("ts")?;
        let mut tsx_files = self.list_files_by_language("tsx")?;
        let mut files = ts_files.clone();
        files.append(&mut tsx_files.clone());
        if files.is_empty() {
            return Ok(0);
        }

        let all_paths: HashSet<String> = files.iter().map(|(_, p)| p.clone()).collect();
        let api_map = self.api_summary_map_all()?;

        // Load tsconfig path aliases once (best-effort).
        let aliases = ts_load_alias_config(repo_root);

        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut committed = false;

        let res: Result<usize> = (|| {
            // Remove old TS-related edges, keep other edge types (e.g. `refers`).
            self.conn.execute(
                "DELETE FROM edges WHERE edge_type IN ('ts_import', 'ts_imported_by', 'jsx_uses', 'jsx_used_by')",
                [],
            )?;

            let mut stmt_ins = self.conn.prepare(
                r#"INSERT OR IGNORE INTO edges(from_rowid, to_rowid, edge_type, weight)
                   VALUES (?1, ?2, ?3, ?4)"#,
            )?;

            // Prepared statements for JSX edges.
            let mut stmt_ref_froms = self.conn.prepare(
                r#"
                SELECT DISTINCT refs.from_rowid
                FROM refs
                JOIN fragments ON fragments.rowid = refs.from_rowid
                WHERE fragments.path=?1 AND refs.ref_text=?2
                LIMIT ?3
                "#,
            )?;

            let mut stmt_symbol_defs = self.conn.prepare(
                r#"
                SELECT frag_rowid
                FROM symbols
                WHERE symbol=?1 AND path=?2
                LIMIT ?3
                "#,
            )?;

            let mut edge_count: usize = 0;

            // -----------------------------------------------------------------
            // TS/TSX import graph (file-level ApiSummary → ApiSummary)
            // -----------------------------------------------------------------
            for (_file_id, path) in files.iter() {
                let Some(&from_rid) = api_map.get(path) else {
                    continue;
                };

                // Read source from disk.
                let disk_path = repo_root.join(Path::new(path));
                let src = match fs::read_to_string(&disk_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let targets = ts_module_targets(&src, Path::new(path), &all_paths, &aliases);
                for (target_path, weight) in targets {
                    let Some(&to_rid) = api_map.get(&target_path) else {
                        continue;
                    };
                    if to_rid == from_rid {
                        continue;
                    }
                    stmt_ins.execute(params![from_rid, to_rid, "ts_import", weight])?;
                    edge_count += 1;

                    // Reverse edge for explainability.
                    let w2 = weight * 0.9;
                    stmt_ins.execute(params![to_rid, from_rid, "ts_imported_by", w2])?;
                    edge_count += 1;
                }
            }

            // -----------------------------------------------------------------
            // TSX JSX tag usage edges (fragment → definition fragment)
            // -----------------------------------------------------------------
            // We build edges only when we can resolve a JSX tag to an imported local module.
            // This improves precision vs global ref→def resolution.
            let max_from_frags_per_tag: i64 = 24;
            let max_defs_per_tag: i64 = 6;
            let mut jsx_edges_added: usize = 0;
            let max_jsx_edges_total: usize = 20_000; // safety cap for huge repos

            for (_file_id, path) in tsx_files.iter() {
                if jsx_edges_added >= max_jsx_edges_total {
                    break;
                }

                // Source
                let disk_path = repo_root.join(Path::new(path));
                let src = match fs::read_to_string(&disk_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // File-level node (useful hub)
                let from_file_sum = api_map.get(path).copied();

                // Import bindings (local name → specifier)
                let mut local_map: HashMap<String, TsImportBinding> = HashMap::new();
                for stmt in ts_gather_import_statements(&src) {
                    for b in ts_parse_import_bindings(&stmt) {
                        // last write wins
                        local_map.insert(b.local.clone(), b);
                    }
                }
                if local_map.is_empty() {
                    continue;
                }

                // Extract JSX tags in the file.
                let tags = ts_extract_jsx_tags(&src);
                if tags.is_empty() {
                    continue;
                }

                for tag in tags {
                    if jsx_edges_added >= max_jsx_edges_total {
                        break;
                    }

                    // Determine how to resolve the tag.
                    let (lookup_local, desired_symbol, specifier, is_default): (
                        String,
                        String,
                        String,
                        bool,
                    ) = if let Some((head, tail)) = tag.split_once('.') {
                        // Member tag: Icons.Add
                        if let Some(b) = local_map.get(head) {
                            if b.kind == TsImportKind::Namespace {
                                (
                                    head.to_string(),
                                    tail.to_string(),
                                    b.specifier.clone(),
                                    false,
                                )
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    } else {
                        // Simple tag: Button
                        let Some(b) = local_map.get(&tag) else {
                            continue;
                        };
                        let sym = b.imported.clone().unwrap_or_else(|| tag.clone());
                        (
                            tag.clone(),
                            sym,
                            b.specifier.clone(),
                            b.kind == TsImportKind::Default,
                        )
                    };

                    // Resolve specifier to local TS/TSX paths.
                    let targets = ts_resolve_module_specifier_any(
                        Path::new(path),
                        &specifier,
                        &all_paths,
                        &aliases,
                    );
                    if targets.is_empty() {
                        continue; // npm package or unresolvable
                    }

                    // Find fragments in THIS file that actually reference the local name.
                    let mut from_frags: Vec<i64> = Vec::new();
                    {
                        let rows = stmt_ref_froms.query_map(
                            params![path, &lookup_local, max_from_frags_per_tag],
                            |r| r.get(0),
                        )?;
                        for rr in rows {
                            from_frags.push(rr?);
                        }
                    }

                    // If we couldn't localize to fragments, still allow a file-level link.
                    if from_frags.is_empty() {
                        if let Some(rid) = from_file_sum {
                            from_frags.push(rid);
                        } else {
                            continue;
                        }
                    }

                    // Resolve definition fragments in the imported target file(s).
                    let mut def_rowids: Vec<i64> = Vec::new();
                    for tpath in targets {
                        // First try desired symbol.
                        let rows = stmt_symbol_defs
                            .query_map(params![&desired_symbol, &tpath, max_defs_per_tag], |r| {
                                r.get(0)
                            })?;
                        for rr in rows {
                            def_rowids.push(rr?);
                        }

                        // For default imports, also try file stem as a fallback (common for `export default`).
                        if def_rowids.is_empty() && is_default {
                            if let Some(stem) =
                                Path::new(&tpath).file_stem().and_then(|s| s.to_str())
                            {
                                let rows2 = stmt_symbol_defs
                                    .query_map(params![stem, &tpath, max_defs_per_tag], |r| {
                                        r.get(0)
                                    })?;
                                for rr in rows2 {
                                    def_rowids.push(rr?);
                                }
                            }
                        }

                        // As a last resort, link to the target file ApiSummary (still local and useful).
                        if def_rowids.is_empty() {
                            if let Some(&sum_rid) = api_map.get(&tpath) {
                                def_rowids.push(sum_rid);
                            }
                        }
                    }

                    if def_rowids.is_empty() {
                        continue;
                    }

                    def_rowids.sort();
                    def_rowids.dedup();
                    if def_rowids.len() > max_defs_per_tag as usize {
                        def_rowids.truncate(max_defs_per_tag as usize);
                    }

                    // Insert edges.
                    for &from_rid in &from_frags {
                        for &to_rid in &def_rowids {
                            if from_rid == to_rid {
                                continue;
                            }
                            // JSX edges are high-signal; give them a slightly higher base weight.
                            let w: f32 = 1.15;
                            stmt_ins.execute(params![from_rid, to_rid, "jsx_uses", w])?;
                            edge_count += 1;
                            jsx_edges_added += 1;

                            // Reverse for explainability.
                            let w2: f32 = w * 0.9;
                            stmt_ins.execute(params![to_rid, from_rid, "jsx_used_by", w2])?;
                            edge_count += 1;
                            jsx_edges_added += 1;

                            if jsx_edges_added >= max_jsx_edges_total {
                                break;
                            }
                        }
                        if jsx_edges_added >= max_jsx_edges_total {
                            break;
                        }
                    }
                }
            }

            Ok(edge_count)
        })();

        match res {
            Ok(n) => {
                self.conn.execute_batch("COMMIT;")?;
                committed = true;
                Ok(n)
            }
            Err(e) => {
                if !committed {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                }
                Err(e)
            }
        }
    }

    /// Insert additional module-qualified symbols for all fragments in a file.
    ///
    /// This is a cheap “repair/upgrade” pass that lets new context-engine versions add
    /// better symbol aliases without forcing a full re-index.
    ///
    /// For Rust sources under `src/`, we add:
    /// - `foo::Bar` style aliases (module-qualified)
    /// - `crate::foo::Bar` style aliases
    ///
    /// (When the module prefix is empty, we still add `crate::Bar`.)
    fn augment_rust_module_qualified_symbols_for_file(
        &self,
        file_id: i64,
        file_path: &str,
    ) -> Result<usize> {
        let Some(mod_prefix) = rust_module_prefix_for_index_path(file_path) else {
            return Ok(0);
        };

        // Remove previously-generated module/crate aliases for this file's fragments.
        self.conn.execute(
            "DELETE FROM symbols WHERE frag_rowid IN (SELECT rowid FROM fragments WHERE file_id=?1) AND source IN ('alias_module','alias_crate')",
            params![file_id],
        )?;

        let mut stmt_sel = self.conn.prepare(
            "SELECT rowid, symbol, kind, path FROM fragments WHERE file_id=?1 AND symbol IS NOT NULL",
        )?;
        let mut stmt_ins = self.conn.prepare(
            "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        let rows = stmt_sel.query_map(params![file_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut added = 0usize;
        for rr in rows {
            let (rid, sym, kind, path) = rr?;

            if !mod_prefix.is_empty() {
                let q = format!("{mod_prefix}::{sym}");
                stmt_ins.execute(params![q, rid, kind, path, "alias_module"])?;
                let cq = format!("crate::{mod_prefix}::{sym}");
                stmt_ins.execute(params![cq, rid, kind, path, "alias_crate"])?;
                added += 2;
            } else {
                let cq = format!("crate::{sym}");
                stmt_ins.execute(params![cq, rid, kind, path, "alias_crate"])?;
                added += 1;
            }
        }

        Ok(added)
    }

    /// Insert additional crate-qualified symbols for all fragments in a file.
    ///
    /// This is used to support cross-crate refs like `my_crate::foo::Bar` when we can
    /// determine (best-effort) the owning crate name of each file.
    ///
    /// We only do this for files under `src/` (workspace member crates), and we only
    /// apply it for lib/proc-macro targets (not bins) to reduce symbol noise.
    fn augment_rust_crate_qualified_symbols_for_file(
        &self,
        file_id: i64,
        file_path: &str,
        crate_name: &str,
    ) -> Result<usize> {
        let crate_name = crate_name.trim();
        if crate_name.is_empty() {
            return Ok(0);
        }

        // Only qualify normal crate source files.
        let Some(mod_prefix) = rust_module_prefix_for_index_path(file_path) else {
            return Ok(0);
        };

        // Remove previously-generated crate-name aliases for this file's fragments.
        self.conn.execute(
            "DELETE FROM symbols WHERE frag_rowid IN (SELECT rowid FROM fragments WHERE file_id=?1) AND source IN ('alias_crate_name')",
            params![file_id],
        )?;

        let mut stmt_sel = self.conn.prepare(
            "SELECT rowid, symbol, kind, path FROM fragments WHERE file_id=?1 AND symbol IS NOT NULL",
        )?;
        let mut stmt_ins = self.conn.prepare(
            "INSERT OR IGNORE INTO symbols(symbol, frag_rowid, kind, path, source) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        let rows = stmt_sel.query_map(params![file_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut added = 0usize;
        for rr in rows {
            let (rid, sym, kind, path) = rr?;

            let q = if mod_prefix.is_empty() {
                format!("{crate_name}::{sym}")
            } else {
                format!("{crate_name}::{mod_prefix}::{sym}")
            };

            stmt_ins.execute(params![q, rid, kind, path, "alias_crate_name"])?;
            added += 1;
        }

        Ok(added)
    }

    fn fragment_rowids_for_file_ids(&self, file_ids: &[i64]) -> Result<Vec<i64>> {
        if file_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut out: Vec<i64> = Vec::new();
        for chunk in file_ids.chunks(500) {
            let sql = format!(
                "SELECT rowid FROM fragments WHERE file_id IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                r.get::<_, i64>(0)
            })?;
            for rr in rows {
                out.push(rr?);
            }
        }

        Ok(out)
    }

    fn symbols_for_frag_rowids(&self, frag_rowids: &[i64]) -> Result<Vec<String>> {
        if frag_rowids.is_empty() {
            return Ok(vec![]);
        }
        let mut out: Vec<String> = Vec::new();
        for chunk in frag_rowids.chunks(500) {
            let sql = format!(
                "SELECT DISTINCT symbol FROM symbols WHERE frag_rowid IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                r.get::<_, String>(0)
            })?;
            for rr in rows {
                out.push(rr?);
            }
        }
        Ok(out)
    }

    fn from_rowids_referencing_symbols(&self, symbols: &[String]) -> Result<Vec<i64>> {
        if symbols.is_empty() {
            return Ok(vec![]);
        }
        let mut out: Vec<i64> = Vec::new();
        for chunk in symbols.chunks(400) {
            let sql = format!(
                "SELECT DISTINCT from_rowid FROM refs WHERE ref_text IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                r.get::<_, i64>(0)
            })?;
            for rr in rows {
                out.push(rr?);
            }
        }
        Ok(out)
    }

    fn delete_edges_from_rowids_by_type(&self, from_rowids: &[i64], edge_type: &str) -> Result<()> {
        if from_rowids.is_empty() {
            return Ok(());
        }
        for chunk in from_rowids.chunks(500) {
            let sql = format!(
                "DELETE FROM edges WHERE edge_type=?1 AND from_rowid IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            // First param is edge_type, then the IN (...) params.
            let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(1 + chunk.len());
            params.push(edge_type.to_string().into());
            for v in chunk {
                params.push((*v).into());
            }
            stmt.execute(rusqlite::params_from_iter(params))?;
        }
        Ok(())
    }

    fn delete_edges_from_rowids(&self, from_rowids: &[i64]) -> Result<()> {
        if from_rowids.is_empty() {
            return Ok(());
        }
        for chunk in from_rowids.chunks(500) {
            let sql = format!(
                "DELETE FROM edges WHERE from_rowid IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            stmt.execute(rusqlite::params_from_iter(chunk.iter()))?;
        }
        Ok(())
    }

    pub fn insert_embedding(
        &self,
        frag_rowid: i64,
        model: &str,
        dim: i64,
        vec_blob: &[u8],
    ) -> Result<()> {
        let now = Self::now_ms();
        self.conn.execute(
            r#"
            INSERT INTO embeddings(rowid, model, dim, vec, created_at_ms)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(rowid) DO UPDATE SET
              model=excluded.model,
              dim=excluded.dim,
              vec=excluded.vec,
              created_at_ms=excluded.created_at_ms
            "#,
            params![frag_rowid, model, dim, vec_blob, now],
        )?;
        Ok(())
    }

    pub fn get_fragment_by_rowid(&self, rowid: i64) -> Result<Fragment> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT frag_id, ast_hash, path, kind, symbol,
                   start_byte, end_byte, start_line, start_col, end_line, end_col,
                   signature, body, doc, retrieval_text
            FROM fragments
            WHERE rowid=?1
            "#,
        )?;
        let frag = stmt.query_row(params![rowid], |r| {
            let kind_str: String = r.get(3)?;
            let kind = parse_kind(&kind_str).unwrap_or(FragKind::Other);

            Ok(Fragment {
                id: r.get(0)?,
                ast_hash: r.get(1)?,
                file: std::path::PathBuf::from(r.get::<_, String>(2)?),
                kind,
                symbol: r.get(4)?,
                span: ce_core::model::Span {
                    start_byte: r.get::<_, i64>(5)? as usize,
                    end_byte: r.get::<_, i64>(6)? as usize,
                    start_line: r.get::<_, i64>(7)? as u32,
                    start_col: r.get::<_, i64>(8)? as u32,
                    end_line: r.get::<_, i64>(9)? as u32,
                    end_col: r.get::<_, i64>(10)? as u32,
                },
                signature: r.get(11)?,
                body: r.get(12)?,
                doc: r.get(13)?,
                retrieval_text: r.get(14)?,
                refs: vec![],
            })
        })?;
        Ok(frag)
    }

    pub fn get_fragment_by_id(&self, frag_id: &str) -> Result<Option<(i64, Fragment)>> {
        let row: Option<(i64, Fragment)> = self
            .conn
            .query_row(
                r#"
            SELECT rowid,
                   frag_id, ast_hash, path, kind, symbol,
                   start_byte, end_byte, start_line, start_col, end_line, end_col,
                   signature, body, doc, retrieval_text
            FROM fragments
            WHERE frag_id=?1
            "#,
                params![frag_id],
                |r| {
                    let rowid: i64 = r.get(0)?;
                    let kind_str: String = r.get(4)?;
                    let kind = parse_kind(&kind_str).unwrap_or(FragKind::Other);

                    Ok((
                        rowid,
                        Fragment {
                            id: r.get(1)?,
                            ast_hash: r.get(2)?,
                            file: std::path::PathBuf::from(r.get::<_, String>(3)?),
                            kind,
                            symbol: r.get(5)?,
                            span: ce_core::model::Span {
                                start_byte: r.get::<_, i64>(6)? as usize,
                                end_byte: r.get::<_, i64>(7)? as usize,
                                start_line: r.get::<_, i64>(8)? as u32,
                                start_col: r.get::<_, i64>(9)? as u32,
                                end_line: r.get::<_, i64>(10)? as u32,
                                end_col: r.get::<_, i64>(11)? as u32,
                            },
                            signature: r.get(12)?,
                            body: r.get(13)?,
                            doc: r.get(14)?,
                            retrieval_text: r.get(15)?,
                            refs: vec![],
                        },
                    ))
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Find fragments in a given path that cover a 1-based line number.
    ///
    /// This is used for “signal” based retrieval (e.g. compiler errors / stack traces).
    /// Returns up to `k` rowids ordered by smallest span first (most specific fragment).
    pub fn fragment_rowids_covering_line(
        &self,
        path: &str,
        line_1based: u32,
        k: usize,
    ) -> Result<Vec<i64>> {
        if k == 0 {
            return Ok(vec![]);
        }
        let line0: i64 = line_1based.saturating_sub(1) as i64;

        let mut out = Vec::new();

        // Exact path match first.
        {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT rowid
                FROM fragments
                WHERE path=?1
                  AND start_line <= ?2
                  AND end_line >= ?2
                ORDER BY (end_line - start_line) ASC
                LIMIT ?3
                "#,
            )?;
            let rows = stmt.query_map(params![path, line0, k as i64], |r| r.get(0))?;
            for rr in rows {
                out.push(rr?);
            }
        }

        // Fallback: suffix match (`%path`) for cases where the task text contains
        // a relative path but the index stored an absolute path (or vice versa).
        if out.is_empty() {
            let like = format!("%{path}");
            let mut stmt = self.conn.prepare(
                r#"
                SELECT rowid
                FROM fragments
                WHERE path LIKE ?1
                  AND start_line <= ?2
                  AND end_line >= ?2
                ORDER BY (end_line - start_line) ASC
                LIMIT ?3
                "#,
            )?;
            let rows = stmt.query_map(params![like, line0, k as i64], |r| r.get(0))?;
            for rr in rows {
                out.push(rr?);
            }
        }

        Ok(out)
    }

    /// Fetch up to `k` fragments for a given path (path or suffix match).
    pub fn fragment_rowids_for_path(&self, path: &str, k: usize) -> Result<Vec<i64>> {
        if k == 0 {
            return Ok(vec![]);
        }

        let mut out = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT rowid
                FROM fragments
                WHERE path=?1
                ORDER BY start_line ASC
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![path, k as i64], |r| r.get(0))?;
            for rr in rows {
                out.push(rr?);
            }
        }

        if out.is_empty() {
            let like = format!("%{path}");
            let mut stmt = self.conn.prepare(
                r#"
                SELECT rowid
                FROM fragments
                WHERE path LIKE ?1
                ORDER BY start_line ASC
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![like, k as i64], |r| r.get(0))?;
            for rr in rows {
                out.push(rr?);
            }
        }

        Ok(out)
    }

    /// Lexical search via FTS5.
    /// Returns rowid + a lexical score (higher is better) based on bm25.
    pub fn search_fts(&self, query: &str, k: usize) -> Result<Vec<(i64, f32)>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT rowid, bm25(fragments_fts) AS rank
            FROM fragments_fts
            WHERE fragments_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
            "#,
        )?;
        let mut rows = stmt.query(params![query, k as i64])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let rowid: i64 = r.get(0)?;
            let rank: f32 = r.get::<_, f64>(1)? as f32;
            // For bm25, smaller is better. Convert to similarity-ish score:
            // score = 1 / (1 + rank) (rank can be negative; clamp).
            let denom = 1.0 + rank.max(0.0);
            let score = 1.0 / denom;
            out.push((rowid, score));
        }
        Ok(out)
    }

    pub fn search_hits_by_rowids(&self, rowids: &[i64]) -> Result<Vec<SearchHit>> {
        let mut out = Vec::new();
        for &rid in rowids {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT frag_id, path, kind, symbol, signature
                FROM fragments
                WHERE rowid=?1
                "#,
            )?;
            let row = stmt
                .query_row(params![rid], |r| {
                    let kind_str: String = r.get(2)?;
                    let kind = parse_kind(&kind_str).unwrap_or(FragKind::Other);
                    Ok(SearchHit {
                        frag_id: r.get(0)?,
                        rowid: rid,
                        path: r.get(1)?,
                        kind,
                        symbol: r.get(3)?,
                        score: 0.0,
                        signature: r.get(4)?,
                    })
                })
                .optional()?;
            if let Some(hit) = row {
                out.push(hit);
            }
        }
        Ok(out)
    }

    /// Fetch fragment paths for a set of rowids.
    ///
    /// This is used for cheap grouping (e.g. injecting file-level ApiSummary fragments).
    pub fn paths_for_rowids(
        &self,
        rowids: &[i64],
    ) -> Result<std::collections::HashMap<i64, String>> {
        use std::collections::HashMap;

        let mut out: HashMap<i64, String> = HashMap::new();
        if rowids.is_empty() {
            return Ok(out);
        }

        // Chunk to avoid SQLite parameter limits.
        for chunk in rowids.chunks(500) {
            let sql = format!(
                "SELECT rowid, path FROM fragments WHERE rowid IN {}",
                make_in_clause(chunk.len())
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            for rr in rows {
                let (rid, path) = rr?;
                out.insert(rid, path);
            }
        }

        Ok(out)
    }

    /// Return the rowid for the synthetic file-level ApiSummary fragment for `path` (if present).
    pub fn api_summary_rowid_for_path(&self, path: &str) -> Result<Option<i64>> {
        let row: Option<i64> = self
            .conn
            .query_row(
                "SELECT rowid FROM fragments WHERE path=?1 AND kind='ApiSummary' LIMIT 1",
                params![path],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row)
    }

    /// Return a mapping of `path -> rowid` for all file-level `ApiSummary` fragments.
    ///
    /// This is useful for building graph edges that operate at the file level
    /// (e.g. Rust `mod`/`use` edges).
    pub fn api_summary_map_all(&self) -> Result<HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rowid, path FROM fragments WHERE kind='ApiSummary'")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out: HashMap<String, i64> = HashMap::new();
        for rr in rows {
            let (rid, path) = rr?;
            out.entry(path).or_insert(rid);
        }
        Ok(out)
    }

    pub fn get_embedding_blob(&self, frag_rowid: i64) -> Result<Option<(String, i64, Vec<u8>)>> {
        let row: Option<(String, i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT model, dim, vec FROM embeddings WHERE rowid=?1",
                params![frag_rowid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn resolve_symbol_defs(&self, symbol: &str, k: usize) -> Result<Vec<i64>> {
        if k == 0 {
            return Ok(vec![]);
        }

        // Many refs in Rust are qualified paths like `foo::Bar`. Our `symbols` table
        // often stores only the unqualified definition name (`Bar`). To make
        // definition lookup usable for scoped refs, we try the full string first,
        // then fall back to the tail segment.
        let mut variants: Vec<&str> = vec![symbol];
        if symbol.contains("::") {
            let tail = last_segment(symbol);
            if tail != symbol {
                variants.push(tail);
            }
        }

        let mut stmt = self.conn.prepare(
            r#"
            SELECT frag_rowid
            FROM symbols
            WHERE symbol=?1
            LIMIT ?2
            "#,
        )?;

        let mut out: Vec<i64> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        for qsym in variants {
            let rows = stmt.query_map(params![qsym, k as i64], |r| r.get(0))?;
            for rr in rows {
                let rid: i64 = rr?;
                if seen.insert(rid) {
                    out.push(rid);
                    if out.len() >= k {
                        return Ok(out);
                    }
                }
            }
        }

        Ok(out)
    }

    /// Resolve likely definition fragments for a referenced symbol, biased toward
    /// definitions "near" a given fragment (same file/dir, kind match, etc.).
    ///
    /// This is used during subgraph expansion to reduce ambiguity for common
    /// names like `Error` or `Config`.
    pub fn resolve_symbol_defs_near(
        &self,
        symbol: &str,
        from_rowid: i64,
        k: usize,
    ) -> Result<Vec<i64>> {
        if k == 0 {
            return Ok(vec![]);
        }
        let from: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
            SELECT fragments.path, files.crate_name
            FROM fragments
            JOIN files ON fragments.file_id = files.file_id
            WHERE fragments.rowid=?1
            "#,
                params![from_rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (from_path, from_crate) = from.unwrap_or_default();

        let lim = ((k * 12).max(24)).min(240) as i64;
        let mut stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT fragments.rowid, fragments.path, fragments.kind, fragments.symbol, files.crate_name
            FROM symbols
            JOIN fragments ON symbols.frag_rowid = fragments.rowid
            JOIN files ON fragments.file_id = files.file_id
            WHERE symbols.symbol=?1
            LIMIT ?2
            "#,
        )?;

        // As with `resolve_symbol_defs`, handle qualified refs by also querying the
        // tail segment. We still score using the full `symbol` so module-path hints
        // can disambiguate.
        let mut variants: Vec<&str> = vec![symbol];
        if symbol.contains("::") {
            let tail = last_segment(symbol);
            if tail != symbol {
                variants.push(tail);
            }
        }

        let mut cands: Vec<(i64, f64)> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        for qsym in variants {
            let rows = stmt.query_map(params![qsym, lim], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;

            for rr in rows {
                let (rid, path, kind_s, sym_opt, cand_crate) = rr?;
                if rid == from_rowid {
                    continue;
                }
                if !seen.insert(rid) {
                    continue;
                }
                let Some(kind) = parse_kind(&kind_s) else {
                    continue;
                };
                if kind == FragKind::ApiSummary {
                    continue;
                }
                let w = score_def_candidate(
                    &from_path,
                    &from_crate,
                    symbol,
                    &path,
                    &cand_crate,
                    kind,
                    sym_opt.as_deref(),
                );
                if w > 0.0 {
                    cands.push((rid, w));
                }
            }
        }

        cands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        cands.truncate(k);
        Ok(cands.into_iter().map(|(rid, _)| rid).collect())
    }

    /// Return up to `k` referenced identifiers for a fragment.
    ///
    /// These are stored in the `refs` table by the indexer and are intended
    /// for cheap “graph-ish” expansion (e.g. fetch definitions of referenced symbols).
    pub fn refs_for_fragment(&self, from_rowid: i64, k: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT ref_text
            FROM refs
            WHERE from_rowid=?1
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![from_rowid, k as i64], |r| r.get(0))?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    /// Return up to `k` neighboring fragments in the same file as `rowid`.
    ///
    /// Neighbors are selected by start-byte proximity. This is a cheap but
    /// often-effective way to pull in related helper functions/types adjacent
    /// to a relevant fragment.
    pub fn neighbors_in_file(&self, rowid: i64, k: usize) -> Result<Vec<i64>> {
        if k == 0 {
            return Ok(vec![]);
        }
        let mut stmt = self.conn.prepare(
            r#"
            WITH base AS (
              SELECT file_id AS fid, start_byte AS sb
              FROM fragments
              WHERE rowid=?1
            )
            SELECT f.rowid
            FROM fragments f, base b
            WHERE f.file_id=b.fid AND f.rowid!=?1
            ORDER BY ABS(f.start_byte - b.sb)
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![rowid, k as i64], |r| r.get(0))?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    // ---------------------------------------------------------------------
    // Strategy management (DGM-style “genome”)
    // ---------------------------------------------------------------------

    /// Insert or update a strategy config.
    ///
    /// `config_json` should be a serialized `ce_core::model::StrategyConfig`.
    ///
    /// Returns the strategy_id.
    pub fn upsert_strategy(
        &self,
        strategy_id: &str,
        name: &str,
        config_json: &str,
        parent_id: Option<&str>,
        score: Option<f64>,
    ) -> Result<String> {
        let now = Self::now_ms();
        self.conn.execute(
            r#"
            INSERT INTO strategies(strategy_id, name, config_json, parent_id, score, created_at_ms)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(strategy_id) DO UPDATE SET
              name=excluded.name,
              config_json=excluded.config_json,
              parent_id=excluded.parent_id,
              score=COALESCE(excluded.score, strategies.score)
            "#,
            params![strategy_id, name, config_json, parent_id, score, now],
        )?;
        Ok(strategy_id.to_string())
    }

    /// Convenience helper: compute a deterministic id from `config_json` and upsert.
    pub fn add_strategy(
        &self,
        name: &str,
        config_json: &str,
        parent_id: Option<&str>,
    ) -> Result<String> {
        let strategy_id = ce_core::util::hash_text_hex(config_json);
        self.upsert_strategy(&strategy_id, name, config_json, parent_id, None)
    }

    /// Fetch a strategy record by id.
    pub fn get_strategy(&self, strategy_id: &str) -> Result<Option<StrategyRecord>> {
        let row: Option<StrategyRecord> = self
            .conn
            .query_row(
                r#"
                SELECT strategy_id, name, config_json, parent_id, score, created_at_ms
                FROM strategies
                WHERE strategy_id=?1
                "#,
                params![strategy_id],
                |r| {
                    Ok(StrategyRecord {
                        strategy_id: r.get(0)?,
                        name: r.get(1)?,
                        config_json: r.get(2)?,
                        parent_id: r.get(3)?,
                        score: r.get(4)?,
                        created_at_ms: r.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// List strategies ordered by most recent.
    pub fn list_strategies(&self, limit: usize, offset: usize) -> Result<Vec<StrategyRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT strategy_id, name, config_json, parent_id, score, created_at_ms
            FROM strategies
            ORDER BY created_at_ms DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |r| {
            Ok(StrategyRecord {
                strategy_id: r.get(0)?,
                name: r.get(1)?,
                config_json: r.get(2)?,
                parent_id: r.get(3)?,
                score: r.get(4)?,
                created_at_ms: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    // ---------------------------------------------------------------------
    // Recipe memory
    // ---------------------------------------------------------------------

    pub fn add_recipe(
        &self,
        fingerprint: &str,
        fingerprint_hash: &str,
        tokens: &str,
        failure_excerpt: &str,
        pack_summary: &str,
        patch_meta: &str,
        tags: Option<&str>,
        success_tokens: Option<i64>,
        iterations: Option<i64>,
    ) -> Result<i64> {
        let now = Self::now_ms();
        self.conn.execute(
            r#"
            INSERT INTO recipes(
              fingerprint,
              fingerprint_hash,
              tokens,
              failure_excerpt,
              pack_summary,
              patch_meta,
              tags,
              success_tokens,
              iterations,
              created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                fingerprint,
                fingerprint_hash,
                tokens,
                failure_excerpt,
                pack_summary,
                patch_meta,
                tags,
                success_tokens,
                iterations,
                now
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_recipes(&self, limit: usize, offset: usize) -> Result<Vec<RecipeRecord>> {
        let lim = limit.max(1).min(500) as i64;
        let off = offset as i64;
        let mut stmt = self.conn.prepare(
            r#"
            SELECT recipe_id, fingerprint, fingerprint_hash, tokens, failure_excerpt, pack_summary,
                   patch_meta, tags, success_tokens, iterations, created_at_ms
            FROM recipes
            ORDER BY created_at_ms DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;
        let rows = stmt.query_map(params![lim, off], |r| {
            Ok(RecipeRecord {
                recipe_id: r.get(0)?,
                fingerprint: r.get(1)?,
                fingerprint_hash: r.get(2)?,
                tokens: r.get(3)?,
                failure_excerpt: r.get(4)?,
                pack_summary: r.get(5)?,
                patch_meta: r.get(6)?,
                tags: r.get(7)?,
                success_tokens: r.get(8)?,
                iterations: r.get(9)?,
                created_at_ms: r.get(10)?,
            })
        })?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    pub fn load_recipes(&self, limit: usize) -> Result<Vec<RecipeRecord>> {
        let lim = limit.max(1).min(2000) as i64;
        let mut stmt = self.conn.prepare(
            r#"
            SELECT recipe_id, fingerprint, fingerprint_hash, tokens, failure_excerpt, pack_summary,
                   patch_meta, tags, success_tokens, iterations, created_at_ms
            FROM recipes
            ORDER BY created_at_ms DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![lim], |r| {
            Ok(RecipeRecord {
                recipe_id: r.get(0)?,
                fingerprint: r.get(1)?,
                fingerprint_hash: r.get(2)?,
                tokens: r.get(3)?,
                failure_excerpt: r.get(4)?,
                pack_summary: r.get(5)?,
                patch_meta: r.get(6)?,
                tags: r.get(7)?,
                success_tokens: r.get(8)?,
                iterations: r.get(9)?,
                created_at_ms: r.get(10)?,
            })
        })?;
        let mut out = Vec::new();
        for rr in rows {
            out.push(rr?);
        }
        Ok(out)
    }

    pub fn add_repository_memory(
        &self,
        kind: &str,
        title: &str,
        content: &str,
        tokens: &str,
        path: Option<&str>,
        tags: Option<&str>,
    ) -> Result<i64> {
        if !matches!(kind, "decision" | "golden_path") {
            return Err(anyhow!(
                "repository memory kind must be decision or golden_path"
            ));
        }
        let now = Self::now_ms();
        self.conn.execute(
            "INSERT INTO repository_memory(kind,title,content,tokens,path,tags,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![kind, title, content, tokens, path, tags, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_repository_memory(
        &self,
        kind: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RepositoryMemoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT memory_id,kind,title,content,tokens,path,tags,created_at_ms,updated_at_ms FROM repository_memory WHERE (?1 IS NULL OR kind=?1) ORDER BY updated_at_ms DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(
            params![kind, limit.max(1).min(1000) as i64, offset as i64],
            |row| {
                Ok(RepositoryMemoryRecord {
                    memory_id: row.get(0)?,
                    kind: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    tokens: row.get(4)?,
                    path: row.get(5)?,
                    tags: row.get(6)?,
                    created_at_ms: row.get(7)?,
                    updated_at_ms: row.get(8)?,
                })
            },
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn search_repository_memory(
        &self,
        query: &str,
        min_similarity: f32,
        limit: usize,
    ) -> Result<Vec<(f32, RepositoryMemoryRecord)>> {
        let mut query_tokens = ce_core::util::failure_tokens(query);
        query_tokens.sort();
        query_tokens.dedup();
        let mut scored = Vec::new();
        for record in self.list_repository_memory(None, 1000, 0)? {
            let mut tokens: Vec<String> = record
                .tokens
                .split_whitespace()
                .map(str::to_string)
                .collect();
            tokens.sort();
            tokens.dedup();
            let score = ce_core::util::jaccard_sorted(&query_tokens, &tokens);
            let path_bonus = record
                .path
                .as_ref()
                .filter(|path| query.contains(path.as_str()))
                .map(|_| 0.35)
                .unwrap_or(0.0);
            let score = (score + path_bonus).min(1.0);
            if score >= min_similarity {
                scored.push((score, record));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}

/// Stop-list for edge building.
///
/// Keep this conservative to avoid creating huge edge fanout.
fn make_in_clause(n: usize) -> String {
    // Build a parenthesized list of `?` placeholders suitable for `IN (...)`.
    //
    // Example: n=3 -> "(?,?,?)"
    let mut s = String::new();
    s.push('(');
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s.push(')');
    s
}

fn is_stop_ref_for_edges(s: &str) -> bool {
    matches!(
        s,
        // Rust keywords / module roots
        "self" | "Self" | "super" | "crate" | "std" | "core" | "alloc" |
        // Very common constructors / methods
        "new" | "default" | "len" | "iter" | "into_iter" | "as_ref" | "as_mut" |
        // Common enums / variants
        "Ok" | "Err" | "Some" | "None" |
        // Common prelude-ish types
        "Result" | "Option" | "Vec" | "String" |
        // Primitive-ish names
        "str" | "bool" | "char" |
        "u8" | "u16" | "u32" | "u64" | "usize" |
        "i8" | "i16" | "i32" | "i64" | "isize" |
        "f32" | "f64" |
        // Traits that show up everywhere
        "Clone" | "Copy" | "Debug" | "Default" | "Send" | "Sync" |
        "Into" | "From" | "TryFrom" | "Iterator" | "IntoIterator" |
        // Common collection names
        "HashMap" | "HashSet"
    )
}

fn score_def_candidate(
    from_path: &str,
    from_crate: &str,
    ref_text: &str,
    cand_path: &str,
    cand_crate: &str,
    cand_kind: FragKind,
    cand_symbol: Option<&str>,
) -> f64 {
    // Base weight.
    let mut w: f64 = 1.0;

    // Path proximity (very valuable for disambiguation).
    if !from_path.is_empty() && from_path == cand_path {
        w += 1.0;
    } else {
        let a_dir = parent_dir(from_path);
        let b_dir = parent_dir(cand_path);
        if !a_dir.is_empty() && a_dir == b_dir {
            w += 0.4;
        } else if !a_dir.is_empty()
            && !b_dir.is_empty()
            && (a_dir.starts_with(b_dir) || b_dir.starts_with(a_dir))
        {
            w += 0.2;
        }
    }

    // Crate proximity (Rust workspaces).
    //
    // This is intentionally conservative: it nudges ambiguous symbols toward
    // same-crate definitions, while still allowing cross-crate matches.
    if !from_crate.is_empty() && !cand_crate.is_empty() {
        if from_crate == cand_crate {
            w += 0.35;
        } else {
            w -= 0.20;
        }
    }

    // Module-path hint for qualified refs inside the same crate.
    //
    // Many Rust references look like `foo::Bar` or `foo::bar::Baz`. Our symbols table
    // often stores only the unqualified name (`Bar`, `Baz`), so disambiguation has to
    // happen at scoring time.
    //
    // Heuristic: interpret the lower_snake_case prefix segments as a module path and
    // prefer candidates whose file path matches `.../foo.rs` or `.../foo/mod.rs`
    // (and similarly for `foo/bar.rs` / `foo/bar/mod.rs`).
    if ref_text.contains("::") && !from_crate.is_empty() && from_crate == cand_crate {
        if let Some(prefix) = module_prefix_for_ref_path(ref_text, from_crate) {
            if !prefix.is_empty() {
                let e1 = format!("/{prefix}.rs");
                let e2 = format!("/{prefix}/mod.rs");
                if cand_path.ends_with(&e1) || cand_path.ends_with(&e2) {
                    w += 0.55;
                } else {
                    let needle = format!("/{prefix}/");
                    if cand_path.contains(&needle) {
                        w += 0.15;
                    }
                }
            }
        }
    }

    // Kind match heuristics.
    let ref_is_type = looks_type_like(ref_text);
    let ref_is_fn = looks_fn_like(ref_text);
    if ref_is_type
        && matches!(
            cand_kind,
            FragKind::Struct | FragKind::Enum | FragKind::Trait | FragKind::TypeAlias
        )
    {
        w += 0.35;
    }
    if ref_is_fn && matches!(cand_kind, FragKind::Function | FragKind::Method) {
        w += 0.35;
    }

    // Penalize test fragments (but don't ban them; sometimes tasks are test-related).
    if cand_kind == FragKind::Test {
        w -= 0.75;
    }

    // Qualified references are stronger when they match a qualified symbol.
    if let Some(sym) = cand_symbol {
        if ref_text.contains("::") {
            if sym == ref_text {
                w += 0.35;
            }
        } else {
            // Unqualified reference that matched a qualified symbol via tail-alias: slight penalty.
            let tail = sym.rsplit("::").next().unwrap_or(sym);
            if tail == ref_text && sym.contains("::") {
                w -= 0.15;
            }
        }
    }

    // Clamp to sane range.
    if w.is_nan() {
        return 0.0;
    }
    w = w.max(0.05);
    w.min(3.0)
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

fn last_segment(sym: &str) -> &str {
    sym.rsplit("::").next().unwrap_or(sym)
}

fn looks_type_like(s: &str) -> bool {
    let seg = last_segment(s);
    seg.chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false)
}

fn looks_fn_like(s: &str) -> bool {
    let seg = last_segment(s);
    seg.chars()
        .next()
        .map(|c| c.is_ascii_lowercase())
        .unwrap_or(false)
}

/// Extract a module-path prefix from a qualified ref like `foo::bar::Baz`.
///
/// Returns a `/`-joined path like `foo/bar` suitable for matching `foo/bar.rs`
/// or `foo/bar/mod.rs`.
///
/// Rules (best-effort):
/// - drop leading roots: `crate`, `self`, `super`
/// - drop leading self-crate name (common in 2015 edition or explicit paths)
/// - drop the final segment (the referenced item)
/// - stop before the first TypeLike (UpperCamelCase) segment
fn module_prefix_for_ref_path(ref_text: &str, from_crate: &str) -> Option<String> {
    if !ref_text.contains("::") {
        return None;
    }
    let mut parts: Vec<&str> = ref_text.split("::").collect();
    if parts.len() < 2 {
        return None;
    }

    // Drop leading roots and (optionally) the crate name.
    while let Some(p0) = parts.first().copied() {
        let p = p0.trim();
        if p.is_empty() {
            parts.remove(0);
            continue;
        }
        if p == "crate" || p == "self" || p == "super" {
            parts.remove(0);
            continue;
        }
        if !from_crate.is_empty() && p == from_crate {
            parts.remove(0);
            continue;
        }
        break;
    }
    if parts.len() < 2 {
        return None;
    }

    // Drop the referenced item.
    parts.pop();
    if parts.is_empty() {
        return None;
    }

    let mut mods: Vec<&str> = Vec::new();
    for seg in parts {
        let s = seg.trim();
        if s.is_empty() {
            break;
        }
        // Stop when the path transitions from modules to types (UpperCamelCase).
        if looks_type_like(s) {
            break;
        }
        mods.push(s);
    }

    if mods.is_empty() {
        None
    } else {
        Some(mods.join("/"))
    }
}

fn is_good_ref_for_edges(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.len() < 2 || s.len() > 80 {
        return false;
    }
    if is_stop_ref_for_edges(s) {
        return false;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------
// Rust module/import edge helpers
// ---------------------------------------------------------------------

fn norm_path(p: &Path) -> String {
    p.to_string_lossy().to_string().replace('\\', "/")
}

fn normalize_crate_name(s: &str) -> String {
    s.trim().replace('-', "_")
}

/// Compute a best-effort Rust module path prefix (e.g., `foo::bar`) from a repo-relative file path.
///
/// Returns `None` when the path does not look like a normal Rust source file under `.../src/...`.
/// Returns `Some(prefix)` when under `src`, where `prefix` may be empty for crate roots
/// (`lib.rs`, `main.rs`, bin roots).
///
/// This is intentionally heuristic and not a substitute for rust-analyzer.
fn rust_module_prefix_for_index_path(path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();

    // Find the last `src` segment (workspace crates often have `crates/<name>/src/...`).
    let mut src_i: Option<usize> = None;
    for (i, seg) in parts.iter().enumerate() {
        if *seg == "src" {
            src_i = Some(i);
        }
    }
    let si = src_i?;
    let rel = &parts[si + 1..];
    if rel.is_empty() {
        return Some(String::new());
    }

    // Handle Cargo bin targets:
    // - `src/bin/foo.rs` is a crate root for binary `foo` -> prefix ""
    // - `src/bin/foo/main.rs` is also a crate root -> prefix ""
    // - `src/bin/foo/<mods>.rs` live under that crate root -> prefix derived from <mods>
    if rel.len() >= 2 && rel[0] == "bin" {
        if rel.len() == 2 {
            // `src/bin/foo.rs`
            return Some(String::new());
        }
        // `src/bin/foo/...`
        let after = &rel[2..];
        return Some(rust_module_prefix_from_src_rel_parts(after));
    }

    Some(rust_module_prefix_from_src_rel_parts(rel))
}

fn rust_module_prefix_from_src_rel_parts(rel: &[&str]) -> String {
    if rel.is_empty() {
        return String::new();
    }

    let file = rel[rel.len() - 1];
    let dirs = &rel[..rel.len() - 1];

    let mut segs: Vec<String> = dirs
        .iter()
        .filter(|d| **d != "." && **d != "")
        .map(|s| s.to_string())
        .collect();

    let stem = file.trim_end_matches(".rs");
    if stem == "mod" {
        // module corresponds to directories only
    } else if stem == "lib" || stem == "main" {
        // crate root for this module, ignore file stem
    } else {
        segs.push(stem.to_string());
    }

    segs.join("::")
}

#[derive(Debug, Clone)]
struct RustWorkspace {
    /// Workspace member targets (lib/bin). Used to map files -> owning crate root.
    targets: Vec<RustTarget>,
    /// Map of crate name (as used in code) -> module directory (repo-relative).
    /// This is primarily populated from lib/proc-macro targets.
    crate_name_to_module_dir: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct RustTarget {
    crate_name: String,
    root_file: String,
    module_dir: String,
    kind: RustTargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustTargetKind {
    Lib,
    Bin,
    Other,
}

fn rel_norm_from_path(repo_root: &Path, p: &Path) -> Option<String> {
    // cargo_metadata typically returns absolute paths.
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };
    let rel = abs.strip_prefix(repo_root).ok()?;
    Some(norm_path(rel))
}

/// Determine the directory in which this module's *child modules* are resolved.
///
/// Rust's module file rules (2018+):
/// - lib.rs / main.rs: children live in the parent directory
/// - mod.rs: children live in the parent directory
/// - foo.rs: children live in `parent/foo/`
fn module_dir_for_file(file_path: &Path) -> PathBuf {
    let parent = file_path.parent().unwrap_or(Path::new(""));
    let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    if file_name == "lib.rs" || file_name == "main.rs" || file_name == "mod.rs" {
        return parent.to_path_buf();
    }

    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        if !stem.is_empty() {
            return parent.join(stem);
        }
    }

    parent.to_path_buf()
}

/// Determine the parent module's directory for `super::` resolution.
fn parent_module_dir_for_file(file_path: &Path) -> PathBuf {
    let parent = file_path.parent().unwrap_or(Path::new(""));
    let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    if file_name == "mod.rs" {
        return parent.parent().unwrap_or(Path::new("")).to_path_buf();
    }

    parent.to_path_buf()
}

fn load_rust_workspace(repo_root: &Path, _all_paths: &HashSet<String>) -> RustWorkspace {
    // If this isn't a Cargo workspace, or cargo isn't available, we just return an empty mapping.
    let manifest = repo_root.join("Cargo.toml");
    if !manifest.exists() {
        return RustWorkspace {
            targets: vec![],
            crate_name_to_module_dir: HashMap::new(),
        };
    }

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec();

    let Ok(metadata) = metadata else {
        return RustWorkspace {
            targets: vec![],
            crate_name_to_module_dir: HashMap::new(),
        };
    };

    // Keep only workspace members.
    let members: std::collections::HashSet<cargo_metadata::PackageId> =
        metadata.workspace_members.iter().cloned().collect();

    let mut targets: Vec<RustTarget> = Vec::new();
    let mut crate_name_to_module_dir: HashMap<String, String> = HashMap::new();

    for pkg in metadata.packages {
        if !members.contains(&pkg.id) {
            continue;
        }

        for tgt in pkg.targets {
            // Determine target kind.
            let kind = if tgt.kind.iter().any(|k| k == "lib" || k == "proc-macro") {
                RustTargetKind::Lib
            } else if tgt.kind.iter().any(|k| k == "bin") {
                RustTargetKind::Bin
            } else {
                RustTargetKind::Other
            };

            let crate_name = normalize_crate_name(&tgt.name);
            let abs_src = tgt.src_path.as_std_path();
            let Some(root_file) = rel_norm_from_path(repo_root, abs_src) else {
                continue;
            };

            // Compute module directory for this crate root.
            let module_dir = norm_path(&module_dir_for_file(Path::new(&root_file)));

            targets.push(RustTarget {
                crate_name: crate_name.clone(),
                root_file: root_file.clone(),
                module_dir: module_dir.clone(),
                kind,
            });

            // For cross-crate `use foo::bar`, we care mainly about lib targets.
            if kind == RustTargetKind::Lib {
                crate_name_to_module_dir
                    .entry(crate_name)
                    .or_insert(module_dir);
            }
        }
    }

    RustWorkspace {
        targets,
        crate_name_to_module_dir,
    }
}

fn classify_symbol_source(sym: &str, frag_sym: Option<&str>, crate_name: &str) -> &'static str {
    if let Some(fs) = frag_sym {
        if sym == fs {
            return "extracted";
        }
        if fs.contains("::") {
            if let Some(tail) = fs.rsplit("::").next() {
                if sym == tail {
                    return "alias_tail";
                }
            }
        }
    }

    if sym.starts_with("crate::") {
        return "alias_crate";
    }

    let cn = crate_name.trim();
    if !cn.is_empty() {
        let pfx = format!("{cn}::");
        if sym.starts_with(&pfx) {
            return "alias_crate_name";
        }
    }

    if sym.contains("::") {
        return "alias_module";
    }

    "alias_other"
}

fn looks_ident(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap_or('_');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    true
}

fn find_nearest_rust_crate_root_dir(
    file_path: &Path,
    all_paths: &HashSet<String>,
) -> Option<PathBuf> {
    let mut cur = file_path.parent();
    while let Some(dir) = cur {
        let lib = norm_path(&dir.join("lib.rs"));
        let main = norm_path(&dir.join("main.rs"));
        if all_paths.contains(&lib) || all_paths.contains(&main) {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

fn infer_special_rust_target_root_file(
    file_path: &Path,
    all_paths: &HashSet<String>,
) -> Option<PathBuf> {
    let p = norm_path(file_path);
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }

    // Prefer the deepest match (closest to the file).
    for i in (0..parts.len()).rev() {
        // `.../src/bin/<target>/...`
        if parts[i] == "bin" && i > 0 && parts[i - 1] == "src" && i + 1 < parts.len() {
            if let Some(root) = infer_special_root_from_dir(&parts, i, all_paths) {
                return Some(root);
            }
        }

        // `.../examples/<target>/...`, `.../tests/<target>/...`, `.../benches/<target>/...`
        if (parts[i] == "examples" || parts[i] == "tests" || parts[i] == "benches")
            && i + 1 < parts.len()
        {
            if let Some(root) = infer_special_root_from_dir(&parts, i, all_paths) {
                return Some(root);
            }
        }
    }

    None
}

fn infer_special_root_from_dir(
    parts: &[&str],
    dir_i: usize,
    all_paths: &HashSet<String>,
) -> Option<PathBuf> {
    if dir_i + 1 >= parts.len() {
        return None;
    }

    let mut base = PathBuf::new();
    for seg in &parts[..=dir_i] {
        base.push(seg);
    }

    let stem = parts[dir_i + 1];

    // If the next segment is already a Rust source file, treat it as the root.
    if stem.ends_with(".rs") {
        let cand = norm_path(&base.join(stem));
        if all_paths.contains(&cand) {
            return Some(PathBuf::from(cand));
        }
        return None;
    }

    // Common conventions:
    // - `<stem>.rs`
    // - `<stem>/main.rs`
    // - `<stem>/mod.rs`
    let cand1 = norm_path(&base.join(format!("{stem}.rs")));
    if all_paths.contains(&cand1) {
        return Some(PathBuf::from(cand1));
    }

    let cand2 = norm_path(&base.join(stem).join("main.rs"));
    if all_paths.contains(&cand2) {
        return Some(PathBuf::from(cand2));
    }

    let cand3 = norm_path(&base.join(stem).join("mod.rs"));
    if all_paths.contains(&cand3) {
        return Some(PathBuf::from(cand3));
    }

    None
}

fn crate_module_dir_for_file(
    file_path: &Path,
    ws: &RustWorkspace,
    all_paths: &HashSet<String>,
) -> PathBuf {
    if let Some(t) = best_rust_target_for_file(file_path, ws) {
        if !t.module_dir.is_empty() {
            return PathBuf::from(t.module_dir.clone());
        }
    }

    // Fallback heuristics when Cargo metadata isn't available:
    // 1) Try to infer bin/example/test/bench crate roots from common workspace layouts.
    if let Some(root_file) = infer_special_rust_target_root_file(file_path, all_paths) {
        return module_dir_for_file(&root_file);
    }

    // 2) Climb for lib.rs/main.rs.
    find_nearest_rust_crate_root_dir(file_path, all_paths)
        .unwrap_or_else(|| file_path.parent().unwrap_or(Path::new("")).to_path_buf())
}

/// Pick the "best" Rust target (workspace member) for a given file path.
///
/// This is a best-effort mapping used for:
/// - crate-aware symbol resolution
/// - module graph resolution (`crate::...`)
fn best_rust_target_for_file<'a>(
    file_path: &Path,
    ws: &'a RustWorkspace,
) -> Option<&'a RustTarget> {
    let fp = norm_path(file_path);

    let mut best_score: usize = 0;
    let mut best: Option<&RustTarget> = None;

    for t in &ws.targets {
        let mut score = 0usize;
        if fp == t.root_file {
            // Exact match to a crate root is the strongest signal.
            score = 1_000_000 + t.root_file.len();
        } else {
            // Files under a target's module_dir are assumed to belong to that target.
            let pref = format!("{}/", t.module_dir);
            if !t.module_dir.is_empty() && fp.starts_with(&pref) {
                score = t.module_dir.len();
            }
        }

        if score > best_score {
            best_score = score;
            best = Some(t);
        }
    }

    best
}

fn resolve_module_file(
    base_dir: &Path,
    segments: &[&str],
    all_paths: &HashSet<String>,
) -> Option<String> {
    if segments.is_empty() {
        return None;
    }
    for k in (1..=segments.len()).rev() {
        let mut pb = base_dir.to_path_buf();
        for seg in &segments[..k] {
            pb.push(seg);
        }
        let cand_rs = norm_path(&pb.with_extension("rs"));
        if all_paths.contains(&cand_rs) {
            return Some(cand_rs);
        }
        let cand_mod = norm_path(&pb.join("mod.rs"));
        if all_paths.contains(&cand_mod) {
            return Some(cand_mod);
        }
    }
    None
}

fn parse_path_attr(code: &str) -> Option<String> {
    let s = code.trim();
    if !s.starts_with("#[") || !s.contains("path") {
        return None;
    }

    // Very small best-effort parser for:
    // - `#[path = "foo.rs"]`
    // - `#[path="foo.rs"]`
    // - `#[path = r#"foo.rs"#]` (we still capture between the first two quotes)
    let i = s.find("path")?;
    let after = &s[i..];

    let (q, qpos) = if let Some(p) = after.find('"') {
        ('"', p)
    } else if let Some(p) = after.find('\'') {
        ('\'', p)
    } else {
        return None;
    };

    let rest = &after[qpos + 1..];
    let end = rest.find(q)?;
    let val = &rest[..end];
    if val.trim().is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

fn resolve_path_attr_module(
    file_path: &Path,
    attr_path: &str,
    all_paths: &HashSet<String>,
) -> Option<String> {
    let raw = attr_path.trim();
    if raw.is_empty() {
        return None;
    }

    // Rust's `#[path]` is relative to the directory containing the *current* file.
    let base_dir = if raw.starts_with('/') {
        PathBuf::new()
    } else {
        file_path.parent().unwrap_or(Path::new("")).to_path_buf()
    };

    let rel = raw.trim_start_matches('/');

    let cand = norm_path(&base_dir.join(rel));
    if all_paths.contains(&cand) {
        return Some(cand);
    }

    // If no explicit `.rs`, try `<path>.rs` and `<path>/mod.rs`.
    if !rel.ends_with(".rs") {
        let mut pb = base_dir.join(rel);
        pb.set_extension("rs");
        let cand_rs = norm_path(&pb);
        if all_paths.contains(&cand_rs) {
            return Some(cand_rs);
        }

        let cand_mod = norm_path(&base_dir.join(rel).join("mod.rs"));
        if all_paths.contains(&cand_mod) {
            return Some(cand_mod);
        }
    }

    None
}

/// Parse a Rust source file and produce a set of file-level module edges.
///
/// Returns tuples: (edge_type, target_path, weight)
fn rust_module_targets(
    src: &str,
    file_path: &Path,
    all_paths: &HashSet<String>,
    ws: &RustWorkspace,
    crate_mod_dir: &Path,
) -> Vec<(String, String, f32)> {
    let mut out: Vec<(String, String, f32)> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let file_module_dir = module_dir_for_file(file_path);
    let super_module_dir = parent_module_dir_for_file(file_path);

    // Local map for `mod foo;` targets in this file. This helps resolve `use ...`
    // when modules are declared with `#[path="..."]`.
    let mut local_mods: HashMap<String, String> = HashMap::new();
    let mut pending_path: Option<String> = None;

    for line in src.lines() {
        // Drop end-of-line comments.
        let mut code = line;
        if let Some((before, _after)) = code.split_once("//") {
            code = before;
        }
        let code = code.trim();
        if code.is_empty() {
            continue;
        }

        // Doc comments / inner doc comments shouldn't clear `#[path]`.
        if code.starts_with("///") || code.starts_with("//!") {
            continue;
        }

        // Attribute lines (possibly including `#[path="..."]`).
        if code.starts_with("#[") {
            if let Some(p) = parse_path_attr(code) {
                pending_path = Some(p);
            }
            continue;
        }

        // `mod foo;` / `pub mod foo;` / `pub(crate) mod foo;` etc.
        if code.ends_with(';') {
            let mut mod_code = code;
            if !mod_code.starts_with("mod ") && mod_code.starts_with("pub") {
                if let Some(mi) = mod_code.find("mod ") {
                    mod_code = &mod_code[mi..];
                }
            }

            if mod_code.starts_with("mod ") {
                let name = mod_code
                    .strip_prefix("mod ")
                    .unwrap_or("")
                    .trim_end_matches(';')
                    .trim();
                if !looks_ident(name) {
                    pending_path = None;
                    continue;
                }

                let target: Option<String> = if let Some(p) = pending_path.take() {
                    resolve_path_attr_module(file_path, &p, all_paths)
                } else {
                    let cand1 = norm_path(&file_module_dir.join(format!("{name}.rs")));
                    let cand2 = norm_path(&file_module_dir.join(name).join("mod.rs"));
                    if all_paths.contains(&cand1) {
                        Some(cand1)
                    } else if all_paths.contains(&cand2) {
                        Some(cand2)
                    } else {
                        None
                    }
                };

                if let Some(t) = target {
                    if seen.insert(("mod".to_string(), t.clone())) {
                        out.push(("mod".to_string(), t.clone(), 1.0));
                    }
                    local_mods.insert(name.to_string(), t);
                }
                continue;
            }
        }

        // Any non-attribute, non-mod item clears pending path attribute.
        pending_path = None;

        // `use ...;` / `pub use ...;` / `pub(crate) use ...;` etc.
        let mut use_code = code;
        if use_code.starts_with("pub") {
            if let Some(ui) = use_code.find("use ") {
                use_code = &use_code[ui..];
            }
        }
        if use_code.starts_with("use ") {
            let rest = use_code.strip_prefix("use ").unwrap_or("").trim();
            let rest = rest.trim_end_matches(';').trim();
            if rest.is_empty() {
                continue;
            }

            // Expand simple group imports:
            // - `use crate::{a, b};` => `crate::a`, `crate::b`
            // - `use foo::{a, b};` => `foo::a`, `foo::b`
            let mut paths: Vec<String> = Vec::new();
            if let Some((before, after)) = rest.split_once('{') {
                let prefix0 = before.trim_end().trim_end_matches("::").trim();
                let inside = after.split_once('}').map(|(a, _)| a).unwrap_or(after);
                for raw in inside.split(',') {
                    let item = raw.trim();
                    if item.is_empty() || item.contains('{') {
                        continue;
                    }
                    // Strip `as Alias`.
                    let item = item.split_whitespace().next().unwrap_or("").trim();
                    if item.is_empty() || item == "self" {
                        continue;
                    }
                    let full = if prefix0.is_empty() {
                        item.to_string()
                    } else {
                        format!("{prefix0}::{item}")
                    };
                    paths.push(full);
                }
                if paths.is_empty() {
                    // Fallback: at least consider the prefix itself.
                    paths.push(prefix0.to_string());
                }
            } else {
                paths.push(rest.to_string());
            }

            for p in paths {
                let prefix = p.split_whitespace().next().unwrap_or("").trim();
                if prefix.is_empty() {
                    continue;
                }

                // Determine base directory + tail.
                let mut weight: f32 = 0.7;
                let (base_dir, tail): (PathBuf, &str) =
                    if let Some(t) = prefix.strip_prefix("crate::") {
                        (crate_mod_dir.to_path_buf(), t)
                    } else if let Some(t) = prefix.strip_prefix("self::") {
                        (file_module_dir.clone(), t)
                    } else if let Some(t) = prefix.strip_prefix("super::") {
                        (super_module_dir.clone(), t)
                    } else {
                        // Cross-crate import: `use other_crate::foo::bar` (workspace members only).
                        let first = prefix.split("::").next().unwrap_or("").trim();
                        if first.is_empty() {
                            continue;
                        }
                        if let Some(md) = ws.crate_name_to_module_dir.get(first) {
                            let tail = prefix.strip_prefix(&format!("{first}::")).unwrap_or("");
                            if tail.is_empty() {
                                continue;
                            }
                            weight = 0.6;
                            (PathBuf::from(md), tail)
                        } else {
                            // Fall back to treating this as a crate-local absolute path.
                            // This helps with common Rust 2018+ code that omits `crate::`.
                            weight = 0.65;
                            (crate_mod_dir.to_path_buf(), prefix)
                        }
                    };

                let segs: Vec<&str> = tail.split("::").filter(|s| !s.is_empty()).collect();
                if segs.is_empty() {
                    continue;
                }

                let mut target = resolve_module_file(&base_dir, &segs, all_paths);

                // If this fails, try local `mod` mappings (including `#[path]`).
                if target.is_none() {
                    if let Some(first) = segs.first().copied() {
                        if let Some(mod_file) = local_mods.get(first) {
                            if segs.len() == 1 {
                                target = Some(mod_file.clone());
                            } else {
                                let base2 = module_dir_for_file(Path::new(mod_file));
                                if let Some(t2) = resolve_module_file(&base2, &segs[1..], all_paths)
                                {
                                    target = Some(t2);
                                } else {
                                    target = Some(mod_file.clone());
                                }
                            }
                        }
                    }
                }

                if let Some(t) = target {
                    if seen.insert(("use".to_string(), t.clone())) {
                        out.push(("use".to_string(), t, weight));
                    }
                }
            }

            continue;
        }
    }

    out
}

// -----------------------------------------------------------------------------
// TypeScript/TSX module graph (imports/re-exports + tsconfig path aliases + JSX edges)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct TsAliasConfig {
    /// Repo-relative baseUrl (when present).
    base_url: Option<PathBuf>,
    /// `compilerOptions.paths` rules.
    paths: Vec<TsPathAlias>,
}

#[derive(Debug, Clone)]
struct TsPathAlias {
    /// e.g. "@/*" or "@components/*"
    key: String,
    /// Repo-relative target patterns, e.g. ["src/*"]
    targets: Vec<String>,
}

fn ts_load_alias_config(repo_root: &Path) -> TsAliasConfig {
    let mut cfg = TsAliasConfig::default();

    // Prefer common tsconfig files in the repo root.
    let mut candidates: Vec<PathBuf> = vec![
        repo_root.join("tsconfig.json"),
        repo_root.join("tsconfig.base.json"),
        repo_root.join("tsconfig.app.json"),
        repo_root.join("tsconfig.paths.json"),
    ];

    // Also consider any `tsconfig*.json` files in the root directory (best-effort).
    if let Ok(rd) = fs::read_dir(repo_root) {
        for ent in rd.flatten() {
            let p = ent.path();
            if !p.is_file() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.starts_with("tsconfig") || !name.ends_with(".json") {
                continue;
            }
            // Avoid duplicates.
            if !candidates.iter().any(|x| x == &p) {
                candidates.push(p);
            }
        }
    }

    candidates.sort();
    candidates.dedup();

    let mut visited: HashSet<PathBuf> = HashSet::new();
    for p in candidates {
        if !p.is_file() {
            continue;
        }
        let extracted = ts_extract_aliases_from_tsconfig(repo_root, &p, 0, &mut visited);
        cfg = ts_merge_alias_config(cfg, extracted);
    }

    // Practical fallback for Vite-style repos where `@/` is used but TS paths are missing.
    // If `src/` exists, treat `@/*` as `src/*`.
    let has_at = cfg
        .paths
        .iter()
        .any(|a| a.key.starts_with("@/") || a.key.starts_with("@/*") || a.key == "@");
    if !has_at && repo_root.join("src").is_dir() {
        cfg.paths.push(TsPathAlias {
            key: "@/*".to_string(),
            targets: vec!["src/*".to_string()],
        });
    }

    cfg
}

fn ts_merge_alias_config(parent: TsAliasConfig, child: TsAliasConfig) -> TsAliasConfig {
    // Child overrides base_url when present.
    let base_url = child.base_url.or(parent.base_url);

    // Merge paths by key: child overrides parent entries.
    let mut map: HashMap<String, TsPathAlias> = HashMap::new();
    for a in parent.paths {
        map.insert(a.key.clone(), a);
    }
    for a in child.paths {
        map.insert(a.key.clone(), a);
    }

    let mut paths: Vec<TsPathAlias> = map.into_values().collect();
    paths.sort_by(|a, b| a.key.cmp(&b.key));

    TsAliasConfig { base_url, paths }
}

fn ts_extract_aliases_from_tsconfig(
    repo_root: &Path,
    config_path: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
) -> TsAliasConfig {
    if depth > 6 {
        return TsAliasConfig::default();
    }
    let Ok(abs) = config_path.canonicalize() else {
        return TsAliasConfig::default();
    };
    if !visited.insert(abs.clone()) {
        return TsAliasConfig::default();
    }

    let src = match fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(_) => return TsAliasConfig::default(),
    };
    let v: Value = match serde_json::from_str(&src) {
        Ok(v) => v,
        Err(_) => return TsAliasConfig::default(),
    };

    // Handle `extends` first (parent), then overlay current.
    let mut out = TsAliasConfig::default();
    if let Some(ext) = v.get("extends").and_then(|x| x.as_str()) {
        // Ignore package-based extends like `@tsconfig/node16/tsconfig.json`.
        if ext.starts_with('.') || ext.starts_with('/') {
            let base_dir = config_path.parent().unwrap_or_else(|| Path::new(""));
            let mut p = base_dir.join(ext);
            if p.extension().is_none() {
                p.set_extension("json");
            }
            if p.is_file() {
                let parent = ts_extract_aliases_from_tsconfig(repo_root, &p, depth + 1, visited);
                out = ts_merge_alias_config(out, parent);
            }
        } else {
            // Also accept non-@ relative-ish paths like "../tsconfig.base.json".
            // If it doesn't look like a package name, try resolving relative to config dir.
            if !ext.contains('@') {
                let base_dir = config_path.parent().unwrap_or_else(|| Path::new(""));
                let mut p = base_dir.join(ext);
                if p.extension().is_none() {
                    p.set_extension("json");
                }
                if p.is_file() {
                    let parent =
                        ts_extract_aliases_from_tsconfig(repo_root, &p, depth + 1, visited);
                    out = ts_merge_alias_config(out, parent);
                }
            }
        }
    }

    let base_dir = config_path.parent().unwrap_or_else(|| Path::new(""));

    // baseUrl
    if let Some(bu) = v
        .get("compilerOptions")
        .and_then(|x| x.get("baseUrl"))
        .and_then(|x| x.as_str())
    {
        let bu = bu.trim();
        if !bu.is_empty() {
            let abs_bu = base_dir.join(bu);
            if let Ok(rel) = abs_bu.strip_prefix(repo_root) {
                out.base_url = Some(ts_join_and_normalize(Path::new(""), &norm_path(rel)));
            }
        }
    }

    // paths
    if let Some(obj) = v
        .get("compilerOptions")
        .and_then(|x| x.get("paths"))
        .and_then(|x| x.as_object())
    {
        let mut rules: Vec<TsPathAlias> = Vec::new();
        for (k, vv) in obj {
            let key = k.trim().to_string();
            if key.is_empty() {
                continue;
            }
            let arr = vv.as_array().cloned().unwrap_or_default();
            let mut targets: Vec<String> = Vec::new();

            // Base for targets: baseUrl if present, else config dir.
            let target_base = if let Some(bu) = &out.base_url {
                repo_root.join(bu)
            } else {
                base_dir.to_path_buf()
            };

            for tv in arr {
                let Some(t) = tv.as_str() else {
                    continue;
                };
                let t = t.trim();
                if t.is_empty() {
                    continue;
                }
                if t.starts_with("http://") || t.starts_with("https://") {
                    continue;
                }

                // Make repo-relative by joining against target_base.
                let abs_t = target_base.join(t);
                if let Ok(rel) = abs_t.strip_prefix(repo_root) {
                    // Normalize to repo-style forward slashes.
                    let rel_s = norm_path(rel);
                    let rel_norm = norm_path(&ts_join_and_normalize(Path::new(""), &rel_s));
                    if !rel_norm.is_empty() {
                        targets.push(rel_norm);
                    }
                }
            }

            if !targets.is_empty() {
                targets.sort();
                targets.dedup();
                rules.push(TsPathAlias { key, targets });
            }
        }

        // Merge extracted rules into out.
        for r in rules {
            // Override existing key if present.
            if let Some(pos) = out.paths.iter().position(|x| x.key == r.key) {
                out.paths[pos] = r;
            } else {
                out.paths.push(r);
            }
        }
    }

    out
}

fn ts_module_targets(
    src: &str,
    file_path: &Path,
    all_paths: &HashSet<String>,
    aliases: &TsAliasConfig,
) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = Vec::new();
    for (spec, weight) in ts_import_specifiers(src) {
        let targets = ts_resolve_module_specifier_any(file_path, &spec, all_paths, aliases);
        for t in targets {
            out.push((t, weight));
        }
    }
    out
}

fn ts_import_specifiers(src: &str) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = Vec::new();
    // Rough line-based parsing. We handle:
    // - import ... from "...";
    // - export ... from "...";
    // - import("...") dynamic
    // - require("...")
    for line in src.lines() {
        let s = line.trim();
        if s.starts_with("import ") || s.starts_with("export ") {
            if let Some(spec) = ts_extract_first_string_literal(s) {
                out.push((spec, 1.0));
            }
            continue;
        }
        if s.contains("import(") {
            if let Some(spec) = ts_extract_first_string_literal(s) {
                out.push((spec, 0.65));
            }
            continue;
        }
        if s.contains("require(") {
            if let Some(spec) = ts_extract_first_string_literal(s) {
                out.push((spec, 0.55));
            }
            continue;
        }
    }
    out
}

fn ts_extract_first_string_literal(line: &str) -> Option<String> {
    // Find first occurrence of '"' or '\'' and extract until matching quote.
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' || c == '\'' {
            let quote = c;
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == quote {
                    return Some(line[start..j].to_string());
                }
                if cj == '\\' {
                    j += 1;
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

fn ts_strip_spec_fragments(spec: &str) -> &str {
    // Strip query/hash fragments used by bundlers (e.g. "./x?raw", "./y#foo").
    spec.split(['?', '#']).next().unwrap_or(spec)
}

fn ts_resolve_module_specifier_any(
    file_path: &Path,
    spec: &str,
    all_paths: &HashSet<String>,
    aliases: &TsAliasConfig,
) -> Vec<String> {
    let spec0 = ts_strip_spec_fragments(spec.trim());
    if spec0.is_empty() {
        return Vec::new();
    }

    // Apply path aliases first (if any match).
    let mut alias_candidates: Vec<PathBuf> = ts_apply_paths_aliases(spec0, aliases);

    // Also try raw (relative) resolution.
    let base_dir = file_path.parent().unwrap_or_else(|| Path::new(""));
    if spec0.starts_with('.') || spec0.starts_with('/') {
        alias_candidates.push(ts_join_and_normalize(base_dir, spec0));
    }

    let mut out: Vec<String> = Vec::new();
    for base in alias_candidates {
        for p in ts_resolve_base_path(&base, all_paths) {
            if all_paths.contains(&p) {
                out.push(p);
            }
        }
    }

    out.sort();
    out.dedup();
    if out.len() > 32 {
        out.truncate(32);
    }
    out
}

fn ts_apply_paths_aliases(spec: &str, aliases: &TsAliasConfig) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for rule in &aliases.paths {
        if let Some(star) = ts_match_star(&rule.key, spec) {
            for tgt in &rule.targets {
                // Replace '*' with captured segment
                let resolved = if tgt.contains('*') {
                    tgt.replace('*', &star)
                } else {
                    tgt.to_string()
                };
                out.push(ts_join_and_normalize(Path::new(""), &resolved));
            }
        }
    }
    out
}

fn ts_match_star(pattern: &str, s: &str) -> Option<String> {
    // Very small glob: exactly one '*' in pattern.
    let Some((pre, post)) = pattern.split_once('*') else {
        return if pattern == s {
            Some("".to_string())
        } else {
            None
        };
    };
    if !s.starts_with(pre) {
        return None;
    }
    if !s.ends_with(post) {
        return None;
    }
    let mid = &s[pre.len()..s.len() - post.len()];
    Some(mid.to_string())
}

fn ts_resolve_base_path(base: &Path, all_paths: &HashSet<String>) -> Vec<String> {
    let base_s = norm_path(base);
    let mut candidates: Vec<String> = Vec::new();

    // If base already ends with an extension, try it directly.
    if base_s.ends_with(".ts")
        || base_s.ends_with(".tsx")
        || base_s.ends_with(".js")
        || base_s.ends_with(".jsx")
    {
        candidates.push(base_s.clone());
        return candidates;
    }

    // Try common extensions.
    for ext in ["ts", "tsx", "js", "jsx"] {
        candidates.push(format!("{base_s}.{ext}"));
    }

    // Try index.* in a directory.
    let idx_base = format!("{base_s}/index");
    for ext in ["ts", "tsx", "js", "jsx"] {
        candidates.push(format!("{idx_base}.{ext}"));
    }

    // Also allow import of directory via package.json "main" or "module" etc (best-effort).
    // We only look for a package.json file if it exists in all_paths.
    let pkg = format!("{base_s}/package.json");
    if all_paths.contains(&pkg) {
        // If package.json is indexed (rare), do nothing special.
    }

    // Finally, include the base itself if it matches an indexed path (some repos index without ext).
    if all_paths.contains(&base_s) {
        candidates.push(base_s);
    }

    candidates
}

fn ts_join_and_normalize(base_dir: &Path, spec: &str) -> PathBuf {
    // Join base_dir + spec and normalize `.` / `..`.
    let mut joined = if spec.starts_with('/') {
        // Treat absolute as repo-relative (strip leading '/').
        let s = spec.trim_start_matches('/');
        PathBuf::from(s)
    } else {
        base_dir.join(spec)
    };

    // Normalize components.
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(c.as_os_str()),
        }
    }
    joined = out;

    // Normalize separators.
    PathBuf::from(norm_path(&joined))
}

// -----------------------------------------------------------------------------
// JSX edges: try to resolve TSX JSX tags to imported local components.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TsImportKind {
    Default,
    Named,
    Namespace,
}

#[derive(Debug, Clone)]
struct TsImportBinding {
    local: String,
    imported: Option<String>,
    specifier: String,
    kind: TsImportKind,
}

fn ts_gather_import_statements(src: &str) -> Vec<String> {
    // Very rough: collect contiguous lines starting with import/export.
    // Handles multi-line imports by accumulating until ';' or end.
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_stmt = false;

    for line in src.lines() {
        let s = line.trim();
        let start = s.starts_with("import ") || s.starts_with("export ");
        if start {
            if in_stmt && !buf.is_empty() {
                out.push(buf.clone());
                buf.clear();
            }
            in_stmt = true;
        }
        if in_stmt {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(s);
            if s.contains(';') {
                out.push(buf.clone());
                buf.clear();
                in_stmt = false;
            }
        }
    }
    if in_stmt && !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn ts_parse_import_bindings(stmt: &str) -> Vec<TsImportBinding> {
    // Parse bindings from statements like:
    //   import Foo from "./x";
    //   import { A, B as C } from "./y";
    //   import * as Icons from "./icons";
    //   export { A } from "./z";
    // Returns bindings with local names.

    let s = stmt.trim();
    let spec = match ts_extract_first_string_literal(s) {
        Some(x) => x,
        None => return Vec::new(),
    };

    // Strip leading "import" / "export".
    let mut head = s;
    if head.starts_with("import") {
        head = head.trim_start_matches("import").trim();
    } else if head.starts_with("export") {
        head = head.trim_start_matches("export").trim();
    }

    // Remove trailing "from ..." and anything after.
    if let Some((lhs, _rhs)) = head.split_once(" from ") {
        head = lhs.trim();
    }

    // Handle `export * from` (no local binding)
    if head.starts_with('*') {
        return Vec::new();
    }

    let mut out: Vec<TsImportBinding> = Vec::new();

    // Namespace import: `* as X`
    if head.starts_with('*') {
        if let Some(pos) = head.find("as") {
            let local = head[pos + 2..].trim().trim_end_matches(';').to_string();
            if !local.is_empty() {
                out.push(TsImportBinding {
                    local,
                    imported: None,
                    specifier: spec,
                    kind: TsImportKind::Namespace,
                });
            }
        }
        return out;
    }

    // Default import: `Foo` (possibly followed by `, { ... }`)
    if !head.starts_with('{') {
        // Up to ',' or end
        let first = head.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            out.push(TsImportBinding {
                local: first.to_string(),
                imported: None,
                specifier: spec.clone(),
                kind: TsImportKind::Default,
            });
        }

        // If there are named imports after comma, parse them too.
        if let Some((_, rest)) = head.split_once(',') {
            let rest = rest.trim();
            if rest.starts_with('{') {
                out.extend(ts_parse_named_imports(rest, &spec));
            }
        }
        return out;
    }

    // Named imports: `{ A, B as C }`
    out.extend(ts_parse_named_imports(head, &spec));
    out
}

fn ts_parse_named_imports(braced: &str, spec: &str) -> Vec<TsImportBinding> {
    let mut out: Vec<TsImportBinding> = Vec::new();
    let mut s = braced.trim();
    if let Some(start) = s.find('{') {
        s = &s[start + 1..];
    }
    if let Some(end) = s.rfind('}') {
        s = &s[..end];
    }
    for part in s.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        // `A as B`
        if let Some((a, b)) = p.split_once(" as ") {
            let imported = a.trim();
            let local = b.trim();
            if !local.is_empty() {
                out.push(TsImportBinding {
                    local: local.to_string(),
                    imported: Some(imported.to_string()),
                    specifier: spec.to_string(),
                    kind: TsImportKind::Named,
                });
            }
        } else {
            let name = p.trim();
            if !name.is_empty() {
                out.push(TsImportBinding {
                    local: name.to_string(),
                    imported: Some(name.to_string()),
                    specifier: spec.to_string(),
                    kind: TsImportKind::Named,
                });
            }
        }
    }
    out
}

fn ts_extract_jsx_tags(src: &str) -> Vec<String> {
    let mut out: HashSet<String> = HashSet::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;

    while i + 2 < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let c1 = bytes[i + 1] as char;
        if c1 == '/' || c1 == '!' || c1 == '?' {
            i += 1;
            continue;
        }
        if !c1.is_ascii_uppercase() {
            i += 1;
            continue;
        }

        let mut j = i + 1;
        while j < bytes.len() {
            let cj = bytes[j] as char;
            if cj.is_ascii_alphanumeric() || cj == '_' || cj == '.' {
                j += 1;
                continue;
            }
            break;
        }

        if j > i + 1 {
            let name = String::from_utf8_lossy(&bytes[i + 1..j]).to_string();
            let nm = name.trim();
            // Avoid TSX generic patterns like `<T>` and keep tags reasonable.
            if nm.len() >= 2 && nm.len() <= 80 {
                out.insert(nm.to_string());
            }
        }

        i = j;
    }

    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    if v.len() > 200 {
        v.truncate(200);
    }
    v
}

fn parse_kind(s: &str) -> Option<FragKind> {
    match s {
        "Function" => Some(FragKind::Function),
        "Method" => Some(FragKind::Method),
        "Test" => Some(FragKind::Test),
        "Struct" => Some(FragKind::Struct),
        "Enum" => Some(FragKind::Enum),
        "Trait" => Some(FragKind::Trait),
        "Impl" => Some(FragKind::Impl),
        "Mod" => Some(FragKind::Mod),
        "Const" => Some(FragKind::Const),
        "Static" => Some(FragKind::Static),
        "TypeAlias" => Some(FragKind::TypeAlias),
        "Macro" => Some(FragKind::Macro),
        "ApiSummary" => Some(FragKind::ApiSummary),
        "Other" => Some(FragKind::Other),
        _ => None,
    }
}
