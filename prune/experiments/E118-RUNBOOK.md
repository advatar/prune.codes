# E118 local runbook

E118 uses SWE-bench Multilingual Stage-A retrieval evaluation only. It makes zero model calls.

## Why E118 supersedes the unrun E117 SWE-bench idea

SWE-bench instances are bound to `repo` + `base_commit`. A single shared Prune index across multiple SWE-bench instances would be invalid. E118 therefore creates and caches a separate Prune index for every selected instance at the exact `base_commit`.

## Prerequisites

From `prune/`:

```bash
cargo build --release -p ce-cli
python3 -m pip install datasets numpy
```

The runner fetches public SWE-bench repositories from GitHub and the official dataset from Hugging Face. No GitHub Actions are used.

## Confirmation run

```bash
cd prune
python3 scripts/run_e118_swebench_sparse_causal_prune.py \
  --ce ./target/release/ce \
  --cache .e118-cache \
  --base experiments/E118-base-strategy.json \
  --budget-tokens 12000 \
  --confirm
```

The first run is expensive in wall-clock/I/O because exact historical repository states must be fetched and indexed. The `.e118-cache` directory makes subsequent pack evaluations reuse those indexes.

If successful it writes exactly one immutable result:

`experiments/E118-swebench-sparse-causal-prune.json`

Do not rerun with changed thresholds or inspect evaluation-repository outcomes and then alter the preregistered design. If the result is negative or inconclusive, preserve it and start a new experiment.

## Scientific boundary

A positive E118 result means only that sparse causal probing improved Stage-A Prune context-strategy search on held-out SWE-bench repositories. It does **not** establish SWE-bench issue resolution, improved coding-agent capability, recursive self-improvement, or multi-generation compounding.

The next positive-result experiment is Stage B: freeze the two selected strategies and compare them using the same fixed coding agent and the official SWE-bench harness on fresh repositories.
