# E118 local runbook

E118 uses SWE-bench Multilingual Stage-A retrieval evaluation only. It makes zero model calls.

## Why E118 supersedes the unrun E117 SWE-bench idea

SWE-bench instances are bound to `repo` + `base_commit`. A single shared Prune index across multiple SWE-bench instances would be invalid. E118 therefore creates and caches a separate Prune index for every selected instance at the exact `base_commit`.

## Prerequisites

From `prune/`:

```bash
cargo build --release -p ce-cli
python3 -m venv .venv-e118
.venv-e118/bin/python -m pip install --upgrade pip
.venv-e118/bin/python -m pip install -r experiments/E118-requirements.txt
```

The runner fetches public SWE-bench repositories from GitHub and the official dataset from Hugging Face. No GitHub Actions are used.

## Qualification-only run

This verifies the pinned dataset identity, selection manifest, split counts, plan
digest, base-strategy digest, and binary availability while making zero Prune
pack calls:

```bash
cd prune
.venv-e118/bin/python scripts/run_e118_swebench_sparse_causal_prune.py \
  --ce ./target/release/ce \
  --cache .e118-cache \
  --base experiments/E118-base-strategy.json \
  --budget-tokens 12000
```

The expected frozen manifest digest is
`ccfbe787573dd51c688a92945d923f8d95c73e4811a78f35f9a8c6b1710075ce`.
The qualification-only path never evaluates a strategy.

## Confirmation run

```bash
cd prune
.venv-e118/bin/python scripts/run_e118_swebench_sparse_causal_prune.py \
  --ce ./target/release/ce \
  --cache .e118-cache \
  --base experiments/E118-base-strategy.json \
  --budget-tokens 12000 \
  --confirm
```

The first run is expensive in wall-clock/I/O because exact historical repository states must be fetched and indexed. The `.e118-cache` directory makes subsequent pack evaluations reuse those indexes.

Index reuse is fail-closed on the exact repository remote, checkout HEAD,
cleanliness, HNSW tree digest, SQLite integrity, and a digest of every logical
application-table value. `ce pack` refreshes `meta.embeddings.updated_at_ms`, so
that single volatile timestamp is explicitly excluded from the logical digest;
the original post-index SQLite byte digest remains recorded. Every binding is
validated again after the final pack access.

If successful it exclusively creates an immutable result and digest receipt:

`experiments/E118-swebench-sparse-causal-prune.json`

`experiments/E118-swebench-sparse-causal-prune.json.sha256`

## Independent verification

The verifier reads frozen evidence and Git objects only. It does not rerun search,
Prune indexing, or Prune packing:

```bash
cd prune
.venv-e118/bin/python scripts/verify_e118_swebench_sparse_causal_prune.py \
  --result experiments/E118-swebench-sparse-causal-prune.json \
  --repo-root ..

.venv-e118/bin/python scripts/test_e118_verifier_corruption.py \
  --result experiments/E118-swebench-sparse-causal-prune.json \
  --repo-root ..
```

Do not rerun with changed thresholds or inspect evaluation-repository outcomes and then alter the preregistered design. If the result is negative or inconclusive, preserve it and start a new experiment.

## Scientific boundary

A positive E118 result means only that sparse causal probing improved Stage-A Prune context-strategy search on held-out SWE-bench repositories. It does **not** establish SWE-bench issue resolution, improved coding-agent capability, recursive self-improvement, or multi-generation compounding.

The next positive-result experiment is Stage B: freeze the two selected strategies and compare them using the same fixed coding agent and the official SWE-bench harness on fresh repositories.
