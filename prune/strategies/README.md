# Strategy examples

These are example **partial** `StrategyConfig` files.

Because `StrategyConfig` is annotated with `#[serde(default)]`, you only need to specify the fields you want to override.

## Add a strategy to your DB

```bash
ce strategy add --db .ce/index.sqlite --name balanced --config strategies/balanced.toml
ce strategy add --db .ce/index.sqlite --name cheap --config strategies/cheap.toml
ce strategy add --db .ce/index.sqlite --name compaction --config strategies/compaction.toml
ce strategy add --db .ce/index.sqlite --name compaction_symbols --config strategies/compaction_symbols.toml
ce strategy add --db .ce/index.sqlite --name summary_first_large_repo --config strategies/summary_first_large_repo.toml
```

## Use a stored strategy

```bash
ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw --task "..." --strategy-id <ID>
```

## Override one setting at runtime

```bash
ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw --task "..." --strategy-id <ID> --budget-chars 12000

# Or enforce a token budget
ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw --task "..." --strategy-id <ID> --budget-tokens 3000

# Choose tokenizer used for token budgeting/counts
ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw --task "..." --strategy-id <ID> \
  --budget-tokens 3000 --tokenizer o200k_base
```

### Note on `body_snippet_mode`

`body_snippet_mode` is intentionally stringly-typed and feature-gated by substrings
so you can evolve configs easily. In particular:

- include `signals` to enable file:line slicing
- include `symbols` to enable symbol-focused slicing (task tokens ∩ fragment refs)
- include `ast` to enable Rust AST-based pruning (statement-aware excerpts)
- include `query_grep` to enable broad grep slicing

## Evolve a strategy (random mutation + evaluation)

```bash
ce strategy evolve --db .ce/index.sqlite --tasks ./eval/tasks.jsonl --base-strategy-file strategies/balanced.toml
```
