
# Prioritized vNext roadmap for Prune (AST slicing + subgraph selection + eval-driven evolution)

> Implementation status (2026-08): the roadmap below is implemented in the canonical `prune/` engine. Uniform AST policies cover Rust, TypeScript/TSX, and Swift/SwiftUI; connected prize-collecting selection is on by default with missing-link degradation reports; evolution uses crossover and Pareto fronts; Stage B orchestrates checkout through official harness evaluation; automatic profiles and repository decision/golden-path memory are available through CLI and MCP.

This is a practical “do it in order” plan that maximizes impact per unit effort.

## Phase 1 — AST slicing that actually reduces tokens without losing meaning
**Objective:** get “signature + types + call sites + relevant branches” instead of whole-file dumping.

1) **AST skeletonizer per language pack**
   - Keep: exports/public API, type declarations, function signatures, doc comments
   - Drop: most bodies unless referenced or error-touched
   - Add a “body include policy”: none / referenced-only / top-K relevant blocks

2) **Branch-aware slicing**
   - If task references a symbol or error span, slice to the minimal enclosing node(s)
   - Pull adjacent helper definitions only if referenced

3) **Cross-file type slice**
   - If you include a function, also include the minimal set of type defs it depends on (interfaces/type aliases) rather than the whole types module

**Success metric (Stage A):** token reduction with unchanged “unresolved reference risk”.

---

## Phase 2 — Graph subgraph selection (connected, minimal, and explainable)
**Objective:** choose a *connected* evidence set that explains the task, not just “top-K chunks”.

1) **Subgraph candidate generation**
   - Seed nodes: touched files, mentioned symbols, error file/line hints, entrypoints
   - Expand with weighted edges: import/use/refs/JSX-usage/tsconfig-alias edges

2) **Budgeted subgraph solver**
   - Approximate Steiner tree / prize-collecting walk:
     - minimize: tokens + redundancy
     - maximize: coverage score + connectivity
   - Output: “why included” provenance per fragment

3) **Gating policy**
   - If the solver can’t get a connected subgraph under budget, degrade gracefully:
     - keep the best-connected component + a short “missing links” summary

**Success metric (Stage A):** fewer irrelevant fragments; higher “coverage of touched defs”.

---

## Phase 3 — Eval-driven evolution (DGM-style) over *context strategies*, not tools
**Objective:** automatically discover the best strategy combinations.

1) **SWE-bench runner integration**
   - One command runs:
     - Stage A pack metrics
     - Stage B harness evaluation (optional patch agent)
   - You already outlined the right shape: dataset → checkout base_commit → index/pack → optional agent → harness eval.

2) **Strategy genome**
   - Encode strategy params:
     - pruning aggressiveness
     - edge weights/radii
     - AST skeletonization settings
     - hint extraction weights
     - external reference caps
   - Implement mutation + crossover

3) **Two-tier search**
   - Tier 1: run thousands of Stage A only (fast)
   - Tier 2: shortlist and run Stage B (expensive)

4) **Pareto selection**
   - Promote strategies that dominate on:
     - resolved rate
     - tokens
     - latency
     - missing-def risk

**Success metric:** “best-known” configs per language + per repo archetype.

---

## Phase 4 — Make it product-grade
1) **Strategy registry + auto-selection**
   - “bugfix”, “feature inception”, “refactor”, “integration”
2) **Repo-specific memory (decisions + golden paths)**
3) **Full provenance output**
   - every included fragment has a “why” string

---

## Where Inception Mode fits in this roadmap
Inception Mode is a “Phase 4 quality multiplier,” but it benefits immediately from:
- Phase 1 (AST skeletonization) to keep onboarding packs tiny
- Phase 2 (subgraph selection) to keep “golden paths” relevant
- Phase 3 (evolution) to tune inception profiles for different repo types

---
