# E117 — sparse causal search on real Prune

This experiment is the first transfer of the E104–E116 sparse-causal-search line from the deterministic synthetic substrate into Prune's real Context Engine.

It deliberately stays at Stage A: no model calls, no patch generation and no SWE-bench test execution. `ce pack --format json` is the measured system.

## Inputs

- A built `ce` binary from this branch.
- A Prune index (`index.sqlite` + HNSW directory) for the repository represented by the task set.
- At least 24 Prune eval JSONL tasks. Each task should contain `task`/`prompt`/`problem_statement`, `expect_paths`, and optionally `expect_symbols`.
- The frozen explicit base strategy: `experiments/E117-balanced-base.json`.

The existing `ce tasks import-swe-bench` command can convert SWE-bench-like instances into this task format.

## Confirmation run

```bash
cd prune
cargo build --release -p ce-cli

python3 scripts/run_e117_sparse_causal_prune.py \
  --ce target/release/ce \
  --db /ABS/PATH/TO/index.sqlite \
  --hnsw-dir /ABS/PATH/TO/hnsw \
  --tasks /ABS/PATH/TO/tasks.jsonl \
  --base-strategy ../experiments/E117-balanced-base.json \
  --budget-tokens 12000 \
  --out ../experiments/E117-sparse-causal-prune.json \
  --confirm
```

The confirmation seed is fixed at `1172036`. Task partitioning is deterministic by task-text hash. Evaluation tasks never enter fitting, gate calibration, candidate selection or causal probing.

## What is compared

`raw`: a frozen ridge ranker trained on 24 one-step strategy mutations measured only on the train split.

`causal`: the same ranker and identical candidate schedule. When the ranker's top-1/top-2 margin falls below the preregistered calibration gate, only its top four candidates are measured on one rotating probe shard. The candidate with best measured probe-shard gain is selected.

Both policies get 12 closed-loop generations from the same starting StrategyConfig. Only their final strategies are scored on the held-out evaluation split.

## Interpretation boundary

A positive E117 result means sparse causal sensing helped choose Prune context-strategy mutations on the selected real task set. It is not evidence of recursive LLM self-improvement and is not an end-to-end SWE-bench resolution result.
