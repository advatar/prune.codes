-- 001_init.sql
-- Embedded SQLite schema for Context Engine
-- Notes:
-- - Use `PRAGMA foreign_keys = ON;` in the application.
-- - fragments_fts is maintained via triggers (external content).

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');

CREATE TABLE IF NOT EXISTS files (
  file_id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  language TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  mtime_ms INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);

CREATE TABLE IF NOT EXISTS fragments (
  rowid INTEGER PRIMARY KEY,
  frag_id TEXT UNIQUE NOT NULL,      -- content-addressed id (blake3 hex)
  ast_hash TEXT NOT NULL,            -- structural signature (tree sexp hash)
  file_id INTEGER NOT NULL,
  path TEXT NOT NULL,                -- denormalized for convenience
  kind TEXT NOT NULL,
  symbol TEXT,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  start_line INTEGER NOT NULL,
  start_col INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_col INTEGER NOT NULL,
  signature TEXT NOT NULL,
  body TEXT NOT NULL,
  doc TEXT NOT NULL,
  retrieval_text TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  FOREIGN KEY(file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_fragments_file_id ON fragments(file_id);
CREATE INDEX IF NOT EXISTS idx_fragments_symbol ON fragments(symbol);
CREATE INDEX IF NOT EXISTS idx_fragments_kind ON fragments(kind);

-- FTS5 for lexical search (external content).
-- Store retrieval_text + path + symbol; frag_id is kept unindexed for returning ids.
CREATE VIRTUAL TABLE IF NOT EXISTS fragments_fts USING fts5(
  frag_id UNINDEXED,
  path,
  symbol,
  retrieval_text,
  content='fragments',
  content_rowid='rowid',
  tokenize = 'unicode61 tokenchars "_-"'
);

-- Maintain FTS index via triggers.
CREATE TRIGGER IF NOT EXISTS fragments_ai AFTER INSERT ON fragments BEGIN
  INSERT INTO fragments_fts(rowid, frag_id, path, symbol, retrieval_text)
  VALUES (new.rowid, new.frag_id, new.path, new.symbol, new.retrieval_text);
END;

CREATE TRIGGER IF NOT EXISTS fragments_ad AFTER DELETE ON fragments BEGIN
  INSERT INTO fragments_fts(fragments_fts, rowid, frag_id, path, symbol, retrieval_text)
  VALUES ('delete', old.rowid, old.frag_id, old.path, old.symbol, old.retrieval_text);
END;

CREATE TRIGGER IF NOT EXISTS fragments_au AFTER UPDATE ON fragments BEGIN
  INSERT INTO fragments_fts(fragments_fts, rowid, frag_id, path, symbol, retrieval_text)
  VALUES ('delete', old.rowid, old.frag_id, old.path, old.symbol, old.retrieval_text);

  INSERT INTO fragments_fts(rowid, frag_id, path, symbol, retrieval_text)
  VALUES (new.rowid, new.frag_id, new.path, new.symbol, new.retrieval_text);
END;

-- Embeddings stored as raw f32 bytes (little-endian) for in-process ANN index building.
CREATE TABLE IF NOT EXISTS embeddings (
  rowid INTEGER PRIMARY KEY,         -- same as fragments.rowid
  model TEXT NOT NULL,
  dim INTEGER NOT NULL,
  vec BLOB NOT NULL,
  created_at_ms INTEGER NOT NULL,
  FOREIGN KEY(rowid) REFERENCES fragments(rowid) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_embeddings_model ON embeddings(model);

-- Symbol definitions: best-effort “go to definition” without an LSP.
CREATE TABLE IF NOT EXISTS symbols (
  symbol TEXT NOT NULL,
  frag_rowid INTEGER NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY(symbol, frag_rowid),
  FOREIGN KEY(frag_rowid) REFERENCES fragments(rowid) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_symbols_symbol ON symbols(symbol);

-- Unresolved references inside a fragment: used for cheap graph-ish expansion.
CREATE TABLE IF NOT EXISTS refs (
  from_rowid INTEGER NOT NULL,
  ref_text TEXT NOT NULL,
  PRIMARY KEY(from_rowid, ref_text),
  FOREIGN KEY(from_rowid) REFERENCES fragments(rowid) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_refs_ref_text ON refs(ref_text);

-- Resolved edges between fragments (optional; can be populated incrementally).
CREATE TABLE IF NOT EXISTS edges (
  from_rowid INTEGER NOT NULL,
  to_rowid INTEGER NOT NULL,
  edge_type TEXT NOT NULL,           -- e.g. 'defines', 'calls', 'imports', 'refers'
  weight REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY(from_rowid, to_rowid, edge_type),
  FOREIGN KEY(from_rowid) REFERENCES fragments(rowid) ON DELETE CASCADE,
  FOREIGN KEY(to_rowid) REFERENCES fragments(rowid) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_rowid);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_rowid);

-- Strategy configs (DGM-style evolution target): store JSON/TOML blobs here.
CREATE TABLE IF NOT EXISTS strategies (
  strategy_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  config_json TEXT NOT NULL,
  parent_id TEXT,
  score REAL,
  created_at_ms INTEGER NOT NULL
);
