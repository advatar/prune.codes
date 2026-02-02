# TypeScript/TSX (React) support in Prune

This document summarizes the current TypeScript/TSX/JS/JSX support in Prune and
how to run it. It aligns with `prune/README.md` and the current CLI behavior.

---

## What Prune supports today

Language adapters:
- Rust (`.rs`)
- Swift (`.swift`)
- TypeScript/JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`)

TS/TSX/JS/JSX behavior:
- AST-based fragmentation via tree-sitter-typescript.
- Fragment kinds include functions, classes, interfaces, type aliases, enums, and
  arrow-function exports (useful for React components).
- `export default` is handled: declarations are fragmented, expressions are
  captured as a file-level fragment.
- ApiSummary fragments include TS/TSX refs from import/export headers.

Graph edges (SQLite store):
- `ts_import` / `ts_imported_by` edges between file-level ApiSummary fragments.
- tsconfig `baseUrl` + `paths` resolution (with a practical `@/* -> src/*` fallback).
- JSX tag usage edges: `jsx_uses` / `jsx_used_by`.

Surreal store (embedded):
- TS import resolution is limited to relative/absolute paths (no tsconfig/JSX edges yet).

Packing:
- `tsx_skeleton` is a supported `body_snippet_mode` for TSX/JSX skeletonization.

---

## Build and run

From the Prune repo root:

```bash
cd prune
cargo build -p ce-cli
```

Index a repo:

```bash
./target/debug/ce index --repo /path/to/repo --db /path/to/repo/.ce/index.sqlite --hnsw-dir /path/to/repo/.ce/hnsw
```

Search:

```bash
./target/debug/ce search --db /path/to/repo/.ce/index.sqlite --hnsw-dir /path/to/repo/.ce/hnsw \
  --query "Button component props" --k 12
```

Pack:

```bash
./target/debug/ce pack --db /path/to/repo/.ce/index.sqlite --hnsw-dir /path/to/repo/.ce/hnsw \
  --task "Fix type error in App.tsx around useEffect dependency array" --budget-tokens 3000
```

Optional: SurrealDB store

```bash
cargo build -p ce-cli --features surreal
./target/debug/ce index --store surreal --surreal-path .ce/surreal --repo .
```

---

## Strategy presets

See `prune/strategies/README.md` for built-in presets:
- `balanced`
- `cheap`
- `high_recall`
- `compaction`
- `compaction_symbols`
- `summary_first_large_repo`

---

## Suggested next improvements (TS/React)

- Add tsconfig/JSX edges to the Surreal store path (match SQLite behavior).
- Improve TS symbol resolution for ref→def edges.
- Extend TSX skeletonization with framework-aware heuristics (routes, pages, hooks).
