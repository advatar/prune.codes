You’re thinking about it the right way: once you have **many pruning strategies**, SWE-bench becomes less “a single score” and more “an evaluation harness you drive with an experiment manager.”

The trick is to **separate what you’re evaluating**:

- **Context strategy quality** (fast to measure, helps you search a large space)
- **End-to-end issue resolution** (slow/expensive, but it’s the real scoreboard)

SWE-bench supports both styles of evaluation because it has:
- a **standard dataset format** (instances include `repo`, `base_commit`, `problem_statement`, and a gold `patch`)  [oai_citation:0‡SWEbench](https://www.swebench.com/SWE-bench/guides/datasets/)  
- a **Docker-based harness** that applies your patch and runs tests to decide “resolved”  [oai_citation:1‡SWEbench](https://www.swebench.com/SWE-bench/guides/evaluation/)  
- explicit tooling for **RAG dataset creation + retrieval eval** (useful for context-only evaluation)  [oai_citation:2‡SWEbench](https://www.swebench.com/SWE-bench/guides/create_rag_datasets/)  

Below is a concrete, scalable evaluation plan for “strategies individually + combinations” using SWE-bench.

---

## 1) Pick the right SWE-bench variant for your language packs

You currently have strong **TypeScript** and **Rust** packs (and Swift, but SWE-bench doesn’t cover Swift).

So for *strategy evaluation that actually matches your pack strengths*, use:

- **SWE-bench Multilingual** (300 tasks across 42 repos and 9 languages including **TypeScript** and **Rust**)  [oai_citation:3‡SWEbench](https://www.swebench.com/multilingual.html)  
  This is the best fit to evaluate *your* context work right now.

For “industry standard” comparisons:
- **SWE-bench Verified** is a 500-instance subset verified as solvable by engineers  [oai_citation:4‡SWEbench](https://www.swebench.com/SWE-bench/faq/)  
- **SWE-bench Lite** is designed for faster iteration (the site describes 300 tasks + a dev set)  [oai_citation:5‡SWEbench](https://www.swebench.com/lite.html)  

---

## 2) Use a 2-stage evaluation ladder (this is how you survive “1000 strategies”)

### Stage A — Fast, context-only evaluation (no LLM, no tests)
Goal: rapidly rank strategies and combinations *before* spending money/time on Docker + models.

You can do this using the dataset’s gold patch as “oracle truth” (for evaluation only): dataset instances include the `patch` field with the gold solution patch.  [oai_citation:6‡SWEbench](https://www.swebench.com/SWE-bench/guides/datasets/)

**What you measure per instance:**
- **File recall @K**: did the pack include the files the gold patch touches?
- **Hunk/span coverage**: does the pack include text overlapping changed hunks?
- **Symbol coverage** (if you extract defs/refs): do included fragments cover referenced symbols in the changed regions?
- **Token cost**: pack tokens (and “deferred fetch” tokens)
- **Pruning correctness proxy**:
  - `unbound_symbol_count` (symbols referenced in pack text but with no defs or deferred candidates)

This stage lets you test thousands of variants cheaply.

**Why SWE-bench helps here**
SWE-bench has official tooling to create RAG-style datasets and evaluate retrieval outputs (BM25 etc.)  [oai_citation:7‡SWEbench](https://www.swebench.com/SWE-bench/guides/create_rag_datasets/) — you can either:
- reuse their retrieval-eval flow conceptually, or
- compute your own metrics (often easier since you retrieve *fragments*, not files)

### Stage B — Slow, end-to-end SWE-bench harness evaluation (patch + tests)
Goal: validate the top strategies on what SWE-bench actually reports: “resolved”.

SWE-bench evaluation applies your generated patch and runs tests in Docker.  [oai_citation:8‡SWEbench](https://www.swebench.com/SWE-bench/guides/evaluation/)  
You supply a **predictions JSONL** with (at least):
```json
{
  "instance_id": "...",
  "model_name_or_path": "...",
  "model_patch": "diff patch string"
}
```  
 [oai_citation:9‡SWEbench](https://www.swebench.com/SWE-bench/guides/evaluation/)

Then run:
```bash
python -m swebench.harness.run_evaluation \
  --dataset_name <dataset> \
  --predictions_path <predictions.jsonl> \
  --max_workers 8 \
  --run_id my_run
```
 [oai_citation:10‡SWEbench](https://www.swebench.com/SWE-bench/guides/evaluation/)

---

## 3) “Evaluate separately” means: ablations with fixed everything else

To understand each pruning component’s contribution, create **ablation families**:

### Define a baseline strategy (S0)
Example: “balanced”
- lexical+semantic retrieval
- small import/graph expansion
- basic compaction
- no special constraints

### Then isolate one change per run:
- S1: S0 + “structured signals”
- S2: S0 + “no-unbound-names support closure”
- S3: S0 + “connected subgraph selector”
- S4: S0 + “UI skeletonization”
- S5: S0 + “recipe memory”

**Rule:** everything else stays fixed:
- same dataset split
- same LLM model (if Stage B)
- same max steps / budget
- same random seed policy

This gives you interpretable deltas.

---

## 4) “Evaluate combinations” without exploding compute

The combination space grows exponentially. Don’t do full factorial on SWE-bench harness.

Instead use one of these:

### Option 1: Fractional factorial (good for discrete toggles)
If you have toggles like:
- signals on/off
- support closure on/off
- subgraph on/off
- skeletonization on/off
You can do a fractional design to estimate main effects and some interactions with far fewer runs.

### Option 2: Successive halving / Hyperband (best general method)
This works incredibly well for strategy search:

1) Generate N candidates (random combos / mutated configs)
2) Evaluate all N on a small subset (e.g., 30 tasks)
3) Keep top ~25%
4) Evaluate survivors on 100 tasks
5) Keep top ~25%
6) Evaluate finalists on full set (or Verified/Multilingual full)

Stage A metrics are perfect for the early rounds; Stage B is for finalists.

### Option 3: DGM-style evolution (fits your roadmap)
Treat strategy configs as genomes (“genes” are your knobs), fitness is:
- + resolved_rate (Stage B when you can)
- + file/hunk recall (Stage A)
- − token_cost
- − unbound_symbol_count

Then:
- mutate a few genes each generation
- keep elites
- occasional crossover between two good configs

This gives you principled “search in combo space”.

---

## 5) How to structure “strategy combinations” so they’re meaningful

There are two kinds of combinations:

### A) “Static combos” (one config governs everything)
Example:
- retrieval weights
- graph hop limits
- compaction thresholds
- support-closure caps

These are easy to evaluate because every task uses the same pipeline.

### B) “Policy combos” (gated strategies)
Example meta-policy:
- If you find `path:line` signals → use aggressive slice-first strategy
- Else if low-confidence retrieval → use summary-first strategy
- Else → use balanced strategy

These often win in practice, but you must evaluate them **as their own strategy** because the gating logic changes results.

---

## 6) What SWE-bench datasets give you for free in evaluation design

### Multilingual is explicitly designed to be SWE-bench compatible
The Multilingual benchmark states it’s “fully compatible with SWE-bench” so users can evaluate without changing infrastructure.  [oai_citation:11‡SWEbench](https://www.swebench.com/multilingual.html)  
So you can use the same harness and workflows.

### Task instances contain:
- `repo`, `base_commit`, `problem_statement`, and gold `patch`  [oai_citation:12‡SWEbench](https://www.swebench.com/SWE-bench/guides/datasets/)  
This is perfect for Stage A metrics (oracle localization / coverage scoring).

---

## 7) Practical “experiment bookkeeping” (so results stay sane)

### Use `model_name_or_path` as your strategy label
Since SWE-bench expects that field in predictions, encode:
- base model
- strategy id
- dataset
- run id

Example:
- `gpt-5.2-pro+prune_s03_subgraph+multilingual_test+run42`

### Store a run manifest
For each run:
- strategy config file (full)
- git commit hash of Prune
- dataset + split
- seed settings
- timeouts/budgets
- pack metrics summary (Stage A)
- SWE-bench resolved metrics (Stage B)

SWE-bench reports resolution-rate style metrics (instances resolved/unresolved/etc.).  [oai_citation:13‡SWEbench](https://www.swebench.com/SWE-bench/faq/)

---

## 8) A minimal, recommended workflow

1) **Start with SWE-bench Multilingual**, filter to TypeScript + Rust tasks first (because those match your packs).  [oai_citation:14‡SWEbench](https://www.swebench.com/multilingual.html)  
2) Run **Stage A** on ~300 tasks with 200–1000 strategy candidates (fast).
3) Take the top ~10–20 candidates by a Pareto rule:
   - high gold-file recall + low token cost + low unbound symbols
4) Run **Stage B** on those candidates using SWE-bench harness (Docker) and compare resolved rates.  [oai_citation:15‡SWEbench](https://www.swebench.com/SWE-bench/guides/evaluation/)  
5) Promote winners into your default strategy set and/or evolve further.

---

## If you want the next step as a concrete change request
I can write a full Epic + sub-issues for adding a `ce eval swebench` runner that:
- downloads SWE-bench datasets
- checks out `base_commit`
- runs `ce index` + `ce pack` for Stage A metrics
- (optionally) calls an agent to produce patches → writes predictions JSONL
- invokes `swebench.harness.run_evaluation` for Stage B

That would make “evaluate separately + evaluate combos” a one-command workflow on your side.
