-- 002_add_crate.sql
-- Adds crate-aware metadata to the `files` table.
--
-- We store a best-effort Rust crate name for each indexed Rust file.
-- This is used to bias symbol resolution and edge weighting toward same-crate definitions.

ALTER TABLE files ADD COLUMN crate_name TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_files_crate_name ON files(crate_name);
