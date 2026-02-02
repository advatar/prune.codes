CREATE TABLE IF NOT EXISTS memories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL UNIQUE,
  tags TEXT,
  source TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  embedding BLOB
);

CREATE INDEX IF NOT EXISTS idx_memories_project_id ON memories(project_id);
CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at);
CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
  content,
  tags
);
