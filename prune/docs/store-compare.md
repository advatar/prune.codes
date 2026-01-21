# Store Comparison

| Dimension | SQLite (default) | SurrealDB (embedded) |
| --- | --- | --- |
| Storage | `index.sqlite` + HNSW dump | SurrealKV directory (or mem) |
| Vector search | HNSW index | HNSW index in SurrealDB |
| Full-text search | SQLite FTS | SurrealDB full-text index |
| Graph edges | Edge table in SQLite | Relation table in SurrealDB |
| Hybrid search | Client-side fusion | Client-side RRF fusion (optional store-side later) |
| Setup | No extra feature flags | Build with `--features surreal` |
| Best for | Fast default indexing/search | One-store workflows (graph + vector + FTS) |

## Recommendations
- Use SQLite unless you need SurrealDB graph queries or want a single embedded store for vectors + FTS.
- SurrealKV is safe for Prune because the index is derived and can be rebuilt at any time.
