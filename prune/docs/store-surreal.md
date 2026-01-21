# SurrealDB Store (Embedded)

## Overview
- Optional embedded backend that stores fragments, embeddings, and graph edges in one store.
- SQLite remains the default backend; SurrealDB is opt-in via `--store surreal`.
- Persistent engine defaults to SurrealKV; the mem engine is intended for tests.

## Enable
1) Build with Surreal support:
   - `cargo build -p ce-cli --features surreal`
2) Index a repo:
   - `ce index --repo . --store surreal --surreal-path .ce/surreal`
3) Search/pack:
   - `ce search --store surreal --surreal-path .ce/surreal --query "..."`
   - `ce pack --store surreal --surreal-path .ce/surreal --task "..."`
4) MCP server:
   - `ce mcp serve --repo . --store surreal --surreal-path .ce/surreal`

## Key Options
- `--surreal-engine surrealkv|mem`: persistent vs in-memory.
- `--surreal-versioned`: enable SurrealKV versioned mode.
- `--surreal-ns`, `--surreal-db`: namespace/database selection.
- `--embedding-dim`: override embedding dimension.
- `--fts on|off`: enable or disable full-text search.
- `--hybrid rrf|client`: hybrid retrieval mode (client-side RRF fusion).

## Reset/Rebuild
- The Surreal index is derived data. To reset, delete the `--surreal-path` directory and re-run indexing.
- SurrealKV is beta, but it is safe here because the data can be rebuilt from source.
