#!/usr/bin/env python3
"""E118: sparse causal Prune strategy search on SWE-bench Multilingual.

Zero model calls. Each SWE-bench instance is checked out and indexed at its exact
base_commit. Repository identities, not solver calls, define train/probe/eval
independence. Gold patches are parsed only to obtain changed paths for Stage-A
retrieval scoring; patch contents never enter Prune prompts or policy features.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
from collections import defaultdict
from pathlib import Path
from typing import Any

import numpy as np

DATASET = "SWE-bench/SWE-bench_Multilingual"
SEED = 1182036
GENES = ("lexical_k", "semantic_k", "hybrid_alpha", "graph_seed_k", "edge_radius",
         "neighbors_k", "refs_per_seed", "candidate_pool_limit")
BOUNDS = {
    "lexical_k": (2, 80), "semantic_k": (2, 80), "hybrid_alpha": (0.0, 1.0),
    "graph_seed_k": (1, 40), "edge_radius": (0, 4), "neighbors_k": (0, 20),
    "refs_per_seed": (0, 32), "candidate_pool_limit": (32, 640),
}
STEPS = {
    "lexical_k": (2, 4, 8), "semantic_k": (2, 4, 8), "hybrid_alpha": (.05, .1, .2),
    "graph_seed_k": (1, 2, 4), "edge_radius": (1,), "neighbors_k": (1, 2, 4),
    "refs_per_seed": (1, 2, 4), "candidate_pool_limit": (16, 32, 64),
}


def sh(cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, check=True)


def patch_paths(patch: str) -> list[str]:
    paths = []
    for m in re.finditer(r"^diff --git a/(.+?) b/(.+?)$", patch or "", re.M):
        p = m.group(2)
        if p != "/dev/null" and p not in paths:
            paths.append(p)
    return paths


def infer_language(row: dict[str, Any]) -> str | None:
    paths = patch_paths(str(row.get("patch") or ""))
    rs = sum(p.endswith(".rs") for p in paths)
    ts = sum(p.endswith((".ts", ".tsx")) for p in paths)
    if rs and rs >= ts:
        return "Rust"
    if ts:
        return "TypeScript"
    return None


def load_swebench() -> list[dict[str, Any]]:
    try:
        from datasets import load_dataset
    except ImportError as exc:
        raise SystemExit("pip install datasets before E118") from exc
    ds = load_dataset(DATASET, split="test")
    rows = []
    for row in ds:
        row = dict(row)
        lang = infer_language(row)
        if lang:
            row["_language"] = lang
            row["_expect_paths"] = patch_paths(str(row.get("patch") or ""))
            rows.append(row)
    return rows


def select_and_split(rows: list[dict[str, Any]], cap=6):
    by_repo = defaultdict(list)
    for row in rows:
        by_repo[(row["_language"], row["repo"])].append(row)
    chosen = []
    split_repos = {"train": [], "probe": [], "evaluation": []}
    for lang in ("Rust", "TypeScript"):
        repos = sorted([repo for (l, repo) in by_repo if l == lang],
                       key=lambda r: hashlib.sha256((str(SEED)+r).encode()).hexdigest())
        if len(repos) < 3:
            raise SystemExit(f"need >=3 {lang} repositories, found {len(repos)}")
        # Approximately 50/25/25, but guarantee each language in all splits.
        n = len(repos)
        n_train = max(1, n // 2)
        n_probe = max(1, (n - n_train) // 2)
        if n_train + n_probe >= n:
            n_train = n - 2
            n_probe = 1
        groups = {
            "train": repos[:n_train],
            "probe": repos[n_train:n_train+n_probe],
            "evaluation": repos[n_train+n_probe:],
        }
        for split, rps in groups.items():
            split_repos[split].extend(rps)
            for repo in rps:
                items = sorted(by_repo[(lang, repo)], key=lambda x: hashlib.sha256(str(x["instance_id"]).encode()).hexdigest())[:cap]
                for row in items:
                    row = dict(row)
                    row["_split"] = split
                    chosen.append(row)
    if len(chosen) < 30 or sum(len(v) for v in split_repos.values()) < 9:
        raise SystemExit(f"insufficient selected corpus: {len(chosen)} instances, {split_repos}")
    return chosen, split_repos


def ensure_instance_index(row, cache: Path, ce: str):
    iid = str(row["instance_id"])
    root = cache / iid
    repo_dir, idx = root / "repo", root / "index"
    db, hnsw = idx / "index.sqlite", idx / "hnsw"
    marker = root / "READY.json"
    if marker.exists() and db.exists():
        return repo_dir, db, hnsw
    root.mkdir(parents=True, exist_ok=True)
    if not (repo_dir / ".git").exists():
        repo_dir.mkdir(parents=True, exist_ok=True)
        sh(["git", "init", "-q"], repo_dir)
        sh(["git", "remote", "add", "origin", f"https://github.com/{row['repo']}.git"], repo_dir)
    sh(["git", "fetch", "--depth", "1", "origin", str(row["base_commit"])], repo_dir)
    sh(["git", "checkout", "-q", "--detach", "FETCH_HEAD"], repo_dir)
    idx.mkdir(parents=True, exist_ok=True)
    hnsw.mkdir(parents=True, exist_ok=True)
    sh([ce, "index", "--repo", str(repo_dir), "--db", str(db), "--hnsw-dir", str(hnsw), "--full"])
    marker.write_text(json.dumps({"instance_id": iid, "repo": row["repo"], "base_commit": row["base_commit"]}, sort_keys=True)+"\n")
    return repo_dir, db, hnsw


def norm(p):
    return str(p).replace("\\", "/").lstrip("./")


def score_pack(pack, expected_paths, budget):
    got = {norm(i.get("path", "")) for i in (pack.get("items") or [])}
    exp = [norm(p) for p in expected_paths]
    recall = (sum(any(g == e or g.endswith("/"+e) or e.endswith("/"+g) for g in got) for e in exp) / len(exp)) if exp else 1.0
    metrics = pack.get("metrics") or {}
    conn = float(metrics.get("connectivity_score") or 0.0)
    unbound = float(metrics.get("unbound_symbol_count") or len(pack.get("unresolved_symbols") or []))
    tokens = float(pack.get("used_tokens") or metrics.get("pack_tokens_total") or 0.0)
    eff = 1.0 - min(tokens / max(float(budget), 1.0), 1.0)
    utility = .70*recall + .10*max(0.0, min(conn, 1.0)) + .10*eff - .10*min(unbound/10.0, 1.0)
    return {"utility": utility, "path_recall": recall, "connectivity": conn, "unbound": unbound, "tokens": tokens}


class Evaluator:
    def __init__(self, ce, cache, budget):
        self.ce, self.cache_dir, self.budget = ce, Path(cache), budget
        self.cache = {}
        self.pack_calls = 0

    def one(self, cfg, row):
        ck = hashlib.sha256(json.dumps(cfg, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        key = (ck, row["instance_id"])
        if key in self.cache:
            return self.cache[key]
        _, db, hnsw = ensure_instance_index(row, self.cache_dir, self.ce)
        cp = sh([self.ce, "pack", "--db", str(db), "--hnsw-dir", str(hnsw),
                 "--task", str(row["problem_statement"]), "--strategy-json", json.dumps(cfg, separators=(",", ":")),
                 "--budget-tokens", str(self.budget), "--format", "json"])
        result = score_pack(json.loads(cp.stdout), row["_expect_paths"], self.budget)
        self.cache[key] = result
        self.pack_calls += 1
        return result

    def aggregate(self, cfg, rows):
        vals = [self.one(cfg, r) for r in rows]
        return {k: float(np.mean([v[k] for v in vals])) for k in vals[0]}


def vec(cfg):
    return np.asarray([(float(cfg[g])-BOUNDS[g][0])/(BOUNDS[g][1]-BOUNDS[g][0]) for g in GENES])


def mutate(cfg, rng):
    c = copy.deepcopy(cfg)
    g = str(rng.choice(GENES)); step = float(rng.choice(STEPS[g])) * (-1 if rng.random() < .5 else 1)
    lo, hi = BOUNDS[g]; v = float(c[g]) + step
    if g != "hybrid_alpha": v = int(round(v))
    c[g] = max(lo, min(hi, v)); return c


def feature(parent, child):
    x, y = vec(parent), vec(child); d = y-x
    return np.r_[x, d, x*d]


def fit_ridge(X, y, lam=1.0):
    X=np.asarray(X,float); y=np.asarray(y,float); mu=X.mean(0); sd=X.std(0); sd[sd<1e-9]=1
    Z=(X-mu)/sd; beta=np.linalg.solve(Z.T@Z+lam*np.eye(Z.shape[1]), Z.T@y)
    return mu,sd,beta


def pred(model, feats):
    mu,sd,beta=model; X=np.asarray(feats,float); return ((X-mu)/sd)@beta


def train_model(base, ev, train, rng):
    bu = ev.aggregate(base, train)["utility"]; X=[]; y=[]; seen=set()
    while len(X)<24:
        c=mutate(base,rng); k=json.dumps(c,sort_keys=True)
        if k in seen: continue
        seen.add(k); X.append(feature(base,c)); y.append(ev.aggregate(c,train)["utility"]-bu)
    model=fit_ridge(X,y); margins=[]
    for _ in range(16):
        sib=[mutate(base,rng) for _ in range(12)]; s=np.sort(pred(model,[feature(base,c) for c in sib])); margins.append(float(s[-1]-s[-2]))
    return model, float(np.median(margins))


def shards(rows, n=8):
    out=[[] for _ in range(n)]
    for r in rows: out[hashlib.sha256(str(r["instance_id"]).encode()).digest()[1] % n].append(r)
    return [x for x in out if x]


def trajectory(kind, base, ev, model, gate, probe, seed):
    rng=np.random.default_rng(seed); cur=copy.deepcopy(base); ps=shards(probe); start_calls=ev.pack_calls; meta=[]
    for gen in range(10):
        sib=[mutate(cur,rng) for _ in range(12)]; scores=pred(model,[feature(cur,c) for c in sib]); order=np.argsort(scores)[::-1]; pick=int(order[0]); triggered=False
        if kind=="causal" and float(scores[order[0]]-scores[order[1]]) < gate:
            triggered=True; inds=order[:4]; shard=ps[gen % len(ps)]; pu=ev.aggregate(cur,shard)["utility"]
            gains=[ev.aggregate(sib[int(i)],shard)["utility"]-pu for i in inds]; pick=int(inds[int(np.argmax(gains))])
        cur=sib[pick]; meta.append({"generation":gen,"triggered":triggered,"raw_rank":int(np.where(order==pick)[0][0])+1})
    return cur,{"pack_evaluations":ev.pack_calls-start_calls,"trajectory":meta}


def repo_means(cfg, ev, rows):
    by=defaultdict(list)
    for r in rows: by[r["repo"]].append(r)
    return {repo:ev.aggregate(cfg,rs)["utility"] for repo,rs in by.items()}


def main():
    ap=argparse.ArgumentParser(); ap.add_argument("--ce",default="ce"); ap.add_argument("--cache",type=Path,default=Path(".e118-cache")); ap.add_argument("--base",type=Path,default=Path("experiments/E118-base-strategy.json")); ap.add_argument("--budget-tokens",type=int,default=12000); ap.add_argument("--out",type=Path,default=Path("experiments/E118-swebench-sparse-causal-prune.json")); ap.add_argument("--confirm",action="store_true"); args=ap.parse_args()
    rows=load_swebench(); selected,repos=select_and_split(rows); groups={s:[r for r in selected if r["_split"]==s] for s in ("train","probe","evaluation")}
    base=json.loads(args.base.read_text()); missing=[g for g in GENES if g not in base]
    if missing: raise SystemExit(f"base missing genes {missing}")
    ev=Evaluator(args.ce,args.cache,args.budget_tokens); model,gate=train_model(base,ev,groups["train"],np.random.default_rng(SEED))
    raw,rm=trajectory("raw",base,ev,model,gate,groups["probe"],SEED+1); causal,cm=trajectory("causal",base,ev,model,gate,groups["probe"],SEED+1)
    b=ev.aggregate(base,groups["evaluation"]); r=ev.aggregate(raw,groups["evaluation"]); c=ev.aggregate(causal,groups["evaluation"])
    raw_gain=r["utility"]-b["utility"]; causal_gain=c["utility"]-b["utility"]; delta=causal_gain-raw_gain
    br=repo_means(base,ev,groups["evaluation"]); rr=repo_means(raw,ev,groups["evaluation"]); cr=repo_means(causal,ev,groups["evaluation"])
    wins=sum(cr[k]>rr[k] for k in cr); total=len(cr); naive=10*5*len(groups["probe"]); ratio=cm["pack_evaluations"]/max(1,naive)
    if delta>=.01 and wins>total/2 and ratio<=.35: decision="SWE_BENCH_SPARSE_CAUSAL_PROMISING"
    elif delta>0 and wins>=total/2 and ratio<=.35: decision="SWE_BENCH_SPARSE_CAUSAL_WEAK_SIGNAL"
    elif delta<=-.01: decision="SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED"
    else: decision="SWE_BENCH_SPARSE_CAUSAL_INCONCLUSIVE"
    result={"schema":"prune.e118-result.v1","experiment_id":"E118-swebench-sparse-causal-prune","seed":SEED,"model_calls":0,"decision":decision,"selected_instances":len(selected),"repository_split":repos,"instance_counts":{k:len(v) for k,v in groups.items()},"raw_margin_gate":gate,"base_evaluation":b,"raw_evaluation":r,"causal_evaluation":c,"raw_gain":raw_gain,"causal_gain":causal_gain,"causal_minus_raw":delta,"evaluation_repo_base":br,"evaluation_repo_raw":rr,"evaluation_repo_causal":cr,"causal_repo_wins":wins,"evaluation_repo_count":total,"raw_meta":rm,"causal_meta":cm,"probe_cost_ratio":ratio,"actual_pack_calls":ev.pack_calls,"dataset":DATASET,"claim_boundary":"Stage-A context-selection methodology result only; no SWE-bench issue-resolution or RSI claim."}
    print(json.dumps(result,indent=2,sort_keys=True))
    if args.confirm:
        if args.out.exists(): raise SystemExit(f"immutable result exists: {args.out}")
        args.out.parent.mkdir(parents=True,exist_ok=True); args.out.write_text(json.dumps(result,indent=2,sort_keys=True)+"\n")

if __name__=="__main__": main()
