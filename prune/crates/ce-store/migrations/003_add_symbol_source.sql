-- 003_add_symbol_source.sql
-- Adds a `source` tag to the `symbols` table so we can distinguish extracted symbols
-- from generated aliases (module-qualified, crate-qualified, etc.).
--
-- This makes it possible to rebuild/replace only generated aliases without touching
-- extracted symbol rows.

ALTER TABLE symbols ADD COLUMN source TEXT NOT NULL DEFAULT 'extracted';

CREATE INDEX IF NOT EXISTS idx_symbols_source ON symbols(source);
