CREATE TABLE IF NOT EXISTS repository_memory (
  memory_id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK(kind IN ('decision', 'golden_path')),
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  tokens TEXT NOT NULL,
  path TEXT,
  tags TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repository_memory_kind ON repository_memory(kind);
CREATE INDEX IF NOT EXISTS idx_repository_memory_path ON repository_memory(path);
