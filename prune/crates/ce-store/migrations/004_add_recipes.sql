-- 004_add_recipes.sql
-- Repair recipe memory

CREATE TABLE IF NOT EXISTS recipes (
  recipe_id INTEGER PRIMARY KEY,
  fingerprint TEXT NOT NULL,
  fingerprint_hash TEXT NOT NULL,
  tokens TEXT NOT NULL,
  failure_excerpt TEXT NOT NULL,
  pack_summary TEXT NOT NULL,
  patch_meta TEXT NOT NULL,
  tags TEXT,
  success_tokens INTEGER,
  iterations INTEGER,
  created_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_recipes_fingerprint_hash ON recipes(fingerprint_hash);
