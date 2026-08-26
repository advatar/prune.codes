#!/usr/bin/env python3
"""E117: sparse causal strategy search on the real Prune Context Engine.

No model calls. Shells out to `ce pack --format json`, partitions a Prune eval
JSONL into train/probe/evaluation tasks, freezes a raw pairwise ranker on train
outcomes, then compares raw-only closed-loop mutation selection with
uncertainty-gated top-4 probing on one rotating probe shard.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any

import numpy as np

SEED = 1172036
GENES = (
    "lexical_k", "semantic_k", "hybrid_alpha", "graph_seed_k", "edge_radius",
    "neighbors_k", "refs_per_seed", "support_max_defs", "candidate_pool_limit",
)
BOUNDS = {
    "lexical_k": (2, 40), "semantic_k": (2, 40), "hybrid_alpha": (0.0, 1.0),
    "graph_seed_k": (1, 24), "edge_radius": (0, 4), "neighbors_k": (0, 16),
    "refs_per_seed": (0, 24), "support_max_defs": (0, 48),
    "candidate_pool_limit": (32, 512),
}
STEPS = {
    "lexical_k": (2, 4, 8), "semantic_k": (2, 4, 8), "hybrid_alpha": (.05, .1, .2),
    "graph_seed_k": (1, 2, 4), "edge_radius": (1,), "neighbors_k": (1, 2, 4),
    "refs_per_seed": (1, 2, 4), "support_max_defs": (2, 4, 8),
    "candidate_pool_limit": (16, 32, 64),
}


def load_strategy(path: Path) -> dict[str, Any]:
    if path.suffix.lower() != ".json":
        raise SystemExit("E117 currently requires a JSON StrategyConfig file")
    return json.loads(path.read_text())


def load_tasks(path: Path) -> list[dict[str, Any]]:
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if len(rows) < 24:
        raise SystemExit("E117 requires >=24 tasks for disjoint train/probe/evaluation splits")
    return rows


def task_text(task: dict[str, Any]) -> str:
    for key in ("task", "prompt", "problem_statement"):
        if task.get(key):
            return str(task[key])
    raise ValueError("task has no task/prompt/problem_statement")


def partition(tasks):
    buckets = [[], [], []]
    for task in tasks:
        h = hashlib.sha256(task_text(task).encode()).digest()[0] % 8
        buckets[0 if h < 4 else 1 if h < 6 else 2].append(task)
    if min(map(len, buckets)) < 4:
        raise SystemExit(f"task partition too small: {[len(x) for x in buckets]}")
    return buckets


def norm_path(value: str) -> str:
    return value.replace("\\", "/").lstrip("./")


def pack_score(pack, task, budget_tokens):
    items = pack.get("items") or []
    paths = {norm_path(str(item.get("path", ""))) for item in items}
    expected_paths = [norm_path(str(x)) for x in (task.get("expect_paths") or [])]
    if expected_paths:
        hits = sum(any(p == e or p.endswith("/" + e) or e.endswith("/" + p) for p in paths)
                   for e in expected_paths)
        path_recall = hits / len(expected_paths)
    else:
        path_recall = 1.0

    covered = set(map(str, pack.get("covers_symbols") or []))
    covered |= {str(item["symbol"]) for item in items if item.get("symbol")}
    expected_symbols = list(map(str, task.get("expect_symbols") or []))
    symbol_recall = (sum(s in covered for s in expected_symbols) / len(expected_symbols)
                     if expected_symbols else 1.0)

    metrics = pack.get("metrics") or {}
    connectivity = float(metrics.get("connectivity_score") or 0.0)
    unbound = float(metrics.get("unbound_symbol_count") or len(pack.get("unresolved_symbols") or []))
    tokens = float(pack.get("used_tokens") or metrics.get("pack_tokens_total") or 0.0)
    token_eff = 1.0 - min(tokens / max(1.0, float(budget_tokens)), 1.0)
    utility = (.55 * path_recall + .20 * symbol_recall +
               .10 * max(0.0, min(connectivity, 1.0)) -
               .10 * min(unbound / 10.0, 1.0) + .05 * token_eff)
    return {"utility": utility, "path_recall": path_recall,
            "symbol_recall": symbol_recall, "connectivity": connectivity,
            "unbound": unbound, "tokens": tokens}


class Evaluator:
    def __init__(self, ce, db, hnsw, budget_tokens):
        self.ce, self.db, self.hnsw, self.budget = ce, db, hnsw, budget_tokens
        self.cache = {}
        self.pack_calls = 0

    def one(self, cfg, task):
        cfg_key = hashlib.sha256(json.dumps(cfg, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        task_key = hashlib.sha256(task_text(task).encode()).hexdigest()
        key = (cfg_key, task_key)
        if key in self.cache:
            return self.cache[key]
        cmd = [self.ce, "pack", "--db", self.db, "--hnsw-dir", self.hnsw,
               "--task", task_text(task), "--strategy-json", json.dumps(cfg, separators=(",", ":")),
               "--budget-tokens", str(self.budget), "--format", "json"]
        cp = subprocess.run(cmd, check=True, text=True, capture_output=True)
        result = pack_score(json.loads(cp.stdout), task, self.budget)
        self.pack_calls += 1
        self.cache[key] = result
        return result

    def aggregate(self, cfg, tasks):
        rows = [self.one(cfg, task) for task in tasks]
        return {key: float(np.mean([row[key] for row in rows])) for key in rows[0]}


def vector(cfg):
    vals = []
    for gene in GENES:
        lo, hi = BOUNDS[gene]
        vals.append((float(cfg[gene]) - lo) / (hi - lo))
    return np.asarray(vals, float)


def mutate(cfg, rng):
    child = copy.deepcopy(cfg)
    gene = str(rng.choice(GENES))
    step = float(rng.choice(STEPS[gene])) * (-1 if rng.random() < .5 else 1)
    lo, hi = BOUNDS[gene]
    value = float(child[gene]) + step
    if gene != "hybrid_alpha":
        value = int(round(value))
    child[gene] = max(lo, min(hi, value))
    return child


def raw_feature(parent, child):
    x, y = vector(parent), vector(child)
    delta = y - x
    return np.r_[x, delta, x * delta]


def fit_ridge(X, y, lam=1.0):
    X = np.asarray(X, float)
    y = np.asarray(y, float)
    mu = X.mean(0)
    sd = X.std(0)
    sd[sd < 1e-9] = 1.0
    Z = (X - mu) / sd
    beta = np.linalg.solve(Z.T @ Z + lam * np.eye(Z.shape[1]), Z.T @ y)
    return mu, sd, beta


def predict(model, X):
    mu, sd, beta = model
    return ((np.asarray(X, float) - mu) / sd) @ beta


def bootstrap(base, evaluator, train, rng):
    base_u = evaluator.aggregate(base, train)["utility"]
    X, y, seen = [], [], set()
    while len(X) < 24:
        child = mutate(base, rng)
        key = json.dumps(child, sort_keys=True)
        if key in seen:
            continue
        seen.add(key)
        utility = evaluator.aggregate(child, train)["utility"]
        X.append(raw_feature(base, child))
        y.append(utility - base_u)
    model = fit_ridge(X, y)
    margins = []
    for _ in range(12):
        siblings = [mutate(base, rng) for _ in range(12)]
        scores = np.sort(predict(model, [raw_feature(base, c) for c in siblings]))
        margins.append(float(scores[-1] - scores[-2]))
    return model, float(np.median(margins))


def probe_shards(probe):
    shards = [[] for _ in range(8)]
    for task in probe:
        idx = hashlib.sha256(task_text(task).encode()).digest()[1] % 8
        shards[idx].append(task)
    return [shard if shard else probe for shard in shards]


def run_policy(name, base, evaluator, model, gate, probe, seed):
    rng = np.random.default_rng(seed)
    current = copy.deepcopy(base)
    shards = probe_shards(probe)
    triggers = 0
    probe_calls0 = evaluator.pack_calls
    trajectory = []
    for generation in range(12):
        siblings = [mutate(current, rng) for _ in range(12)]
        scores = predict(model, [raw_feature(current, c) for c in siblings])
        order = np.argsort(scores)[::-1]
        pick = int(order[0])
        triggered = False
        if name == "causal" and float(scores[order[0]] - scores[order[1]]) < gate:
            triggered = True
            triggers += 1
            inds = order[:4]
            shard = shards[generation % 8]
            parent_u = evaluator.aggregate(current, shard)["utility"]
            gains = [evaluator.aggregate(siblings[int(i)], shard)["utility"] - parent_u for i in inds]
            pick = int(inds[int(np.argmax(gains))])
        current = siblings[pick]
        trajectory.append({"generation": generation, "triggered": triggered,
                           "picked_raw_rank": int(np.where(order == pick)[0][0]) + 1})
    return current, {"triggers": triggers,
                     "probe_pack_evaluations": evaluator.pack_calls - probe_calls0,
                     "trajectory": trajectory}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ce", default="ce")
    ap.add_argument("--db", required=True)
    ap.add_argument("--hnsw-dir", required=True)
    ap.add_argument("--tasks", type=Path, required=True)
    ap.add_argument("--base-strategy", type=Path, required=True)
    ap.add_argument("--budget-tokens", type=int, default=12000)
    ap.add_argument("--out", type=Path, default=Path("experiments/E117-sparse-causal-prune.json"))
    ap.add_argument("--confirm", action="store_true")
    args = ap.parse_args()

    tasks = load_tasks(args.tasks)
    train, probe, evaluation = partition(tasks)
    base = load_strategy(args.base_strategy)
    missing = [gene for gene in GENES if gene not in base]
    if missing:
        raise SystemExit(f"base StrategyConfig missing mutable genes: {missing}")

    evaluator = Evaluator(args.ce, args.db, args.hnsw_dir, args.budget_tokens)
    model, gate = bootstrap(base, evaluator, train, np.random.default_rng(SEED))
    raw_final, raw_meta = run_policy("raw", base, evaluator, model, gate, probe, SEED + 1)
    causal_final, causal_meta = run_policy("causal", base, evaluator, model, gate, probe, SEED + 1)

    base_eval = evaluator.aggregate(base, evaluation)
    raw_eval = evaluator.aggregate(raw_final, evaluation)
    causal_eval = evaluator.aggregate(causal_final, evaluation)
    raw_gain = raw_eval["utility"] - base_eval["utility"]
    causal_gain = causal_eval["utility"] - base_eval["utility"]
    full_equiv = 12 * 4 * len(probe)
    ratio = causal_meta["probe_pack_evaluations"] / max(1, full_equiv)

    if causal_gain >= raw_gain + .01 and ratio <= .35:
        decision = "SPARSE_CAUSAL_PRUNE_PROMISING"
    elif causal_gain > raw_gain and ratio <= .35:
        decision = "SPARSE_CAUSAL_PRUNE_WEAK_SIGNAL"
    elif causal_gain < raw_gain - .01:
        decision = "SPARSE_CAUSAL_PRUNE_NOT_SUPPORTED"
    else:
        decision = "SPARSE_CAUSAL_PRUNE_INCONCLUSIVE"

    result = {
        "schema": "prune.e117-result.v1", "experiment_id": "E117-sparse-causal-prune",
        "seed": SEED, "model_calls": 0, "decision": decision,
        "task_counts": {"train": len(train), "probe": len(probe), "evaluation": len(evaluation)},
        "raw_margin_gate": gate, "base_evaluation": base_eval, "raw_evaluation": raw_eval,
        "causal_evaluation": causal_eval, "raw_gain": raw_gain, "causal_gain": causal_gain,
        "causal_minus_raw": causal_gain - raw_gain, "raw_meta": raw_meta, "causal_meta": causal_meta,
        "full_top4_all_probe_shards_equivalent": full_equiv, "probe_cost_ratio": ratio,
        "base_strategy": base, "raw_final_strategy": raw_final, "causal_final_strategy": causal_final,
        "claim_boundary": "Real Prune Context Engine methodology result only; no LLM calls and no SWE-bench patch/test resolution claim."
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    if args.confirm:
        if args.out.exists():
            raise SystemExit(f"immutable result already exists: {args.out}")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
