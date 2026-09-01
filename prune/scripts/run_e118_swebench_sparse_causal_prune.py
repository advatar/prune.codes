#!/usr/bin/env python3
"""E118: sparse-causal Prune strategy search on pinned SWE-bench states.

Gold patches are reduced to scoring-only path truth before search. Candidate
generation never receives a task row. Evaluation repositories are not cloned,
indexed, or packed until both search trajectories have terminated.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import datasets
import numpy as np

DATASET = "SWE-bench/SWE-bench_Multilingual"
DATASET_REVISION = "846e647b9f33c0b51b739d005d13d85493c9af09"
DATASET_SPLIT = "test"
DATASET_ROWS = 300
DATASET_FINGERPRINT = "b4c5db297e9bca0d"
DATASET_PARQUET_SHA256 = "92abca7cb527b41a9f66d03a26ce441ff7319e3a49f985998fd56be4bb9b08b2"
PLAN_SHA256 = "33ad24edd3245b190d7011389800ef24224349e591eb45893eb956243ca58c18"
BASE_SHA256 = "fb9bc7bb162a6613beb5f56a96810efd819b42bae72bab04c4484221da13f571"
SEED = 1182036
GENES = (
    "lexical_k", "semantic_k", "hybrid_alpha", "graph_seed_k",
    "edge_radius", "neighbors_k", "refs_per_seed", "candidate_pool_limit",
)
BOUNDS = {
    "lexical_k": (2, 80), "semantic_k": (2, 80), "hybrid_alpha": (0.0, 1.0),
    "graph_seed_k": (1, 40), "edge_radius": (0, 4), "neighbors_k": (0, 20),
    "refs_per_seed": (0, 32), "candidate_pool_limit": (32, 640),
}
STEPS = {
    "lexical_k": (2, 4, 8), "semantic_k": (2, 4, 8),
    "hybrid_alpha": (0.05, 0.1, 0.2), "graph_seed_k": (1, 2, 4),
    "edge_radius": (1,), "neighbors_k": (1, 2, 4), "refs_per_seed": (1, 2, 4),
    "candidate_pool_limit": (16, 32, 64),
}
FORBIDDEN_POLICY_FIELDS = {"patch", "test_patch", "hints_text", "FAIL_TO_PASS", "PASS_TO_PASS"}


class QualificationError(RuntimeError):
    """A fail-closed E118 qualification or evidence failure."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def digest_json(value: Any) -> str:
    return digest_bytes(canonical_bytes(value))


def digest_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def tree_digest(path: Path) -> str:
    entries = []
    for item in sorted(candidate for candidate in path.rglob("*") if candidate.is_file()):
        entries.append({
            "path": item.relative_to(path).as_posix(),
            "size": item.stat().st_size,
            "sha256": digest_file(item),
        })
    return digest_json(entries)


def write_json_atomic(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n")
    os.replace(temporary, path)


def append_jsonl(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(value, sort_keys=True, allow_nan=False) + "\n")
        handle.flush()
        os.fsync(handle.fileno())


def command(
    args: list[str], *, cwd: Path | None = None, timeout: int | None = None,
    stage: str, failure_ledger: Path | None = None, scope_digest: str | None = None,
) -> tuple[subprocess.CompletedProcess[str], float]:
    started = time.monotonic()
    try:
        result = subprocess.run(
            args, cwd=cwd, text=True, capture_output=True, timeout=timeout, check=False,
        )
    except subprocess.TimeoutExpired as error:
        duration = time.monotonic() - started
        event = {
            "schema": "prune.e118-failure.v1", "at_utc": utc_now(),
            "scope_digest": scope_digest, "stage": stage, "kind": "timeout",
            "duration_seconds": duration, "command_sha256": digest_json(args),
            "stdout_tail": (error.stdout or "")[-4000:] if isinstance(error.stdout, str) else "",
            "stderr_tail": (error.stderr or "")[-4000:] if isinstance(error.stderr, str) else "",
        }
        if failure_ledger:
            append_jsonl(failure_ledger, event)
        raise QualificationError(f"{stage} timed out after {duration:.1f}s") from error
    duration = time.monotonic() - started
    if result.returncode != 0:
        event = {
            "schema": "prune.e118-failure.v1", "at_utc": utc_now(),
            "scope_digest": scope_digest, "stage": stage, "kind": "nonzero_exit",
            "returncode": result.returncode, "duration_seconds": duration,
            "command_sha256": digest_json(args), "stdout_tail": result.stdout[-4000:],
            "stderr_tail": result.stderr[-4000:],
        }
        if failure_ledger:
            append_jsonl(failure_ledger, event)
        raise QualificationError(
            f"{stage} failed with exit {result.returncode}: {result.stderr[-1000:]}"
        )
    return result, duration


def git_output(repo: Path, args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args], cwd=repo, text=True, capture_output=True, check=True,
    )
    return result.stdout.strip()


def patch_paths(patch: str) -> list[str]:
    paths: list[str] = []
    for match in re.finditer(r"^diff --git a/(.+?) b/(.+?)$", patch or "", re.MULTILINE):
        path = match.group(2)
        if path != "/dev/null" and path not in paths:
            paths.append(path)
    return paths


def infer_language_from_paths(paths: list[str]) -> str | None:
    rust = sum(path.endswith(".rs") for path in paths)
    typescript = sum(path.endswith((".ts", ".tsx")) for path in paths)
    if rust and rust >= typescript:
        return "Rust"
    if typescript:
        return "TypeScript"
    return None


def load_swebench() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    dataset = datasets.load_dataset(DATASET, split=DATASET_SPLIT, revision=DATASET_REVISION)
    if dataset.num_rows != DATASET_ROWS:
        raise QualificationError(f"dataset row drift: {dataset.num_rows} != {DATASET_ROWS}")
    if dataset._fingerprint != DATASET_FINGERPRINT:
        raise QualificationError(
            f"dataset fingerprint drift: {dataset._fingerprint} != {DATASET_FINGERPRINT}"
        )
    rows: list[dict[str, Any]] = []
    for source in dataset:
        row = dict(source)
        expected_paths = patch_paths(str(row.get("patch") or ""))
        language = infer_language_from_paths(expected_paths)
        if not language:
            continue
        instance_id = str(row["instance_id"])
        repo = str(row["repo"])
        base_commit = str(row["base_commit"])
        if not re.fullmatch(r"[0-9a-f]{40}", base_commit):
            raise QualificationError(f"invalid base commit for {instance_id}: {base_commit}")
        if not re.fullmatch(r"[^/]+/[^/]+", repo):
            raise QualificationError(f"invalid repository identity for {instance_id}: {repo}")
        problem = str(row["problem_statement"])
        patch = str(row.get("patch") or "")
        rows.append({
            "instance_id": instance_id, "repo": repo, "base_commit": base_commit,
            "_language": language, "_problem_statement": problem,
            "_expect_paths": expected_paths,
            "_problem_statement_sha256": digest_bytes(problem.encode()),
            "_gold_patch_sha256": digest_bytes(patch.encode()),
            "_expected_paths_sha256": digest_json(expected_paths),
        })
    if len({row["instance_id"] for row in rows}) != len(rows):
        raise QualificationError("eligible instance IDs are not unique")
    metadata = {
        "name": DATASET, "revision": DATASET_REVISION, "split": DATASET_SPLIT,
        "rows": dataset.num_rows, "datasets_fingerprint": dataset._fingerprint,
        "parquet_sha256": DATASET_PARQUET_SHA256,
        "datasets_version": datasets.__version__,
    }
    return rows, metadata


def select_and_split(
    rows: list[dict[str, Any]], cap: int = 6,
) -> tuple[list[dict[str, Any]], dict[str, list[str]]]:
    by_repo: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    repo_languages: dict[str, set[str]] = defaultdict(set)
    for row in rows:
        by_repo[(row["_language"], row["repo"])].append(row)
        repo_languages[row["repo"]].add(row["_language"])
    mixed = {
        repo: sorted(languages) for repo, languages in repo_languages.items()
        if len(languages) != 1
    }
    if mixed:
        raise QualificationError(f"repository appears in multiple language strata: {mixed}")
    chosen: list[dict[str, Any]] = []
    split_repos: dict[str, list[str]] = {"train": [], "probe": [], "evaluation": []}
    for language in ("Rust", "TypeScript"):
        repos = sorted(
            [repo for (lang, repo) in by_repo if lang == language],
            key=lambda repo: hashlib.sha256((str(SEED) + repo).encode()).hexdigest(),
        )
        if len(repos) < 3:
            raise QualificationError(
                f"need at least three {language} repositories, found {len(repos)}"
            )
        count = len(repos)
        train_count = max(1, count // 2)
        probe_count = max(1, (count - train_count) // 2)
        if train_count + probe_count >= count:
            train_count = count - 2
            probe_count = 1
        groups = {
            "train": repos[:train_count],
            "probe": repos[train_count:train_count + probe_count],
            "evaluation": repos[train_count + probe_count:],
        }
        for split, repositories in groups.items():
            split_repos[split].extend(repositories)
            for repo in repositories:
                instances = sorted(
                    by_repo[(language, repo)],
                    key=lambda item: hashlib.sha256(
                        str(item["instance_id"]).encode()
                    ).hexdigest(),
                )[:cap]
                for source in instances:
                    row = dict(source)
                    row["_split"] = split
                    chosen.append(row)
    memberships: dict[str, set[str]] = defaultdict(set)
    for split, repositories in split_repos.items():
        for repo in repositories:
            memberships[repo].add(split)
    overlap = {
        repo: sorted(splits) for repo, splits in memberships.items() if len(splits) != 1
    }
    if overlap:
        raise QualificationError(f"repository split overlap: {overlap}")
    if len(chosen) < 30 or len(memberships) < 9:
        raise QualificationError(
            f"insufficient selected corpus: {len(chosen)} instances, {len(memberships)} repositories"
        )
    return chosen, split_repos


def public_manifest(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [{
        "instance_id": row["instance_id"], "repo": row["repo"],
        "base_commit": row["base_commit"], "language": row["_language"],
        "split": row["_split"], "expected_paths": row["_expect_paths"],
        "expected_paths_sha256": row["_expected_paths_sha256"],
        "problem_statement_sha256": row["_problem_statement_sha256"],
        "pack_task_sha256": row["_problem_statement_sha256"],
        "gold_patch_sha256": row["_gold_patch_sha256"],
    } for row in rows]


def normalize_path(path: str) -> str:
    return str(path).replace("\\", "/").lstrip("./")


def score_measurement(
    measurement: dict[str, Any], expected_paths: list[str], budget: int,
) -> dict[str, float]:
    got = {
        normalize_path(path) for path in measurement["item_paths"] if normalize_path(path)
    }
    expected = [normalize_path(path) for path in expected_paths]
    recall = (
        sum(
            any(
                got_path == expected_path
                or got_path.endswith("/" + expected_path)
                or expected_path.endswith("/" + got_path)
                for got_path in got
            )
            for expected_path in expected
        ) / len(expected)
        if expected else 1.0
    )
    connectivity = float(measurement["connectivity_raw"])
    unbound = float(measurement["unbound_raw"])
    tokens = float(measurement["tokens_raw"])
    efficiency = 1.0 - min(tokens / max(float(budget), 1.0), 1.0)
    utility = (
        0.70 * recall + 0.10 * max(0.0, min(connectivity, 1.0))
        + 0.10 * efficiency - 0.10 * min(unbound / 10.0, 1.0)
    )
    result = {
        "utility": float(utility), "path_recall": float(recall),
        "connectivity": connectivity, "unbound": unbound, "tokens": tokens,
    }
    if not all(math.isfinite(value) for value in result.values()):
        raise QualificationError("non-finite pack measurement")
    return result


def config_digest(config: dict[str, Any]) -> str:
    return digest_json(config)


def validate_config(config: dict[str, Any], budget: int) -> None:
    missing = [gene for gene in GENES if gene not in config]
    if missing:
        raise QualificationError(f"strategy missing genes: {missing}")
    for gene in GENES:
        value = float(config[gene])
        lower, upper = BOUNDS[gene]
        if not lower <= value <= upper:
            raise QualificationError(f"strategy gene out of range: {gene}={value}")
    if int(config.get("budget_tokens", -1)) != budget:
        raise QualificationError(
            f"base budget_tokens must equal command budget {budget}"
        )


def vector(config: dict[str, Any]) -> np.ndarray:
    return np.asarray([
        (float(config[gene]) - BOUNDS[gene][0]) / (BOUNDS[gene][1] - BOUNDS[gene][0])
        for gene in GENES
    ])


def mutation_spec(rng: np.random.Generator) -> dict[str, Any]:
    gene = str(rng.choice(GENES))
    magnitude = float(rng.choice(STEPS[gene]))
    direction = -1 if rng.random() < 0.5 else 1
    return {"gene": gene, "signed_step": magnitude * direction}


def apply_mutation(config: dict[str, Any], spec: dict[str, Any]) -> dict[str, Any]:
    child = copy.deepcopy(config)
    gene = spec["gene"]
    lower, upper = BOUNDS[gene]
    value = float(child[gene]) + float(spec["signed_step"])
    if gene != "hybrid_alpha":
        value = int(round(value))
    child[gene] = max(lower, min(upper, value))
    return child


def feature(parent: dict[str, Any], child: dict[str, Any]) -> np.ndarray:
    parent_vector = vector(parent)
    child_vector = vector(child)
    delta = child_vector - parent_vector
    return np.r_[parent_vector, delta, parent_vector * delta]


def fit_ridge(
    features: list[np.ndarray], outcomes: list[float], regularization: float = 1.0,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    matrix = np.asarray(features, dtype=float)
    values = np.asarray(outcomes, dtype=float)
    mean = matrix.mean(0)
    standard_deviation = matrix.std(0)
    standard_deviation[standard_deviation < 1e-9] = 1.0
    normalized = (matrix - mean) / standard_deviation
    beta = np.linalg.solve(
        normalized.T @ normalized + regularization * np.eye(normalized.shape[1]),
        normalized.T @ values,
    )
    return mean, standard_deviation, beta


def predict(
    model: tuple[np.ndarray, np.ndarray, np.ndarray], features: list[np.ndarray],
) -> np.ndarray:
    mean, standard_deviation, beta = model
    matrix = np.asarray(features, dtype=float)
    return ((matrix - mean) / standard_deviation) @ beta


def split_shards(
    rows: list[dict[str, Any]], count: int = 8,
) -> list[list[dict[str, Any]]]:
    shards: list[list[dict[str, Any]]] = [[] for _ in range(count)]
    for row in rows:
        shards[hashlib.sha256(str(row["instance_id"]).encode()).digest()[1] % count].append(row)
    return [shard for shard in shards if shard]


class Evaluator:
    def __init__(
        self, ce: Path, cache_dir: Path, budget: int,
        scope_digest: str, failure_ledger: Path,
    ):
        self.ce = ce.resolve()
        self.ce_sha256 = digest_file(self.ce)
        self.cache_dir = cache_dir.resolve()
        self.budget = budget
        self.scope_digest = scope_digest
        self.failure_ledger = failure_ledger
        self.pack_records: dict[str, dict[str, Any]] = {}
        self.index_bindings: dict[str, dict[str, Any]] = {}
        self.access_log: list[dict[str, Any]] = []
        self.logical_seen: set[str] = set()
        self.executed_pack_calls = 0
        self.disk_pack_reuses = 0
        self.memory_pack_reuses = 0
        self.index_creations = 0
        self.index_reuses = 0
        self.index_recoveries = 0

    def _index_identity(self, row: dict[str, Any]) -> dict[str, Any]:
        return {
            "schema": "prune.e118-index-identity.v1",
            "instance_id": row["instance_id"], "repo": row["repo"],
            "base_commit": row["base_commit"], "ce_sha256": self.ce_sha256,
        }

    def ensure_index(self, row: dict[str, Any]) -> dict[str, Any]:
        instance_id = row["instance_id"]
        if instance_id in self.index_bindings:
            return self.index_bindings[instance_id]
        identity = self._index_identity(row)
        cache_key = digest_json(identity)
        safe_instance = re.sub(r"[^A-Za-z0-9_.-]+", "_", instance_id)[:96]
        root = self.cache_dir / "indexes" / f"{safe_instance}-{cache_key[:16]}"
        repo_dir, index_dir = root / "repo", root / "index"
        database, hnsw = index_dir / "index.sqlite", index_dir / "hnsw"
        marker = root / "READY.json"
        expected_remote = f"https://github.com/{row['repo']}.git"
        if marker.exists():
            stored = json.loads(marker.read_text())
            if stored.get("identity") != identity:
                raise QualificationError(f"cache identity mismatch for {instance_id}")
            if not database.is_file() or not hnsw.is_dir() or not (repo_dir / ".git").exists():
                raise QualificationError(f"incomplete ready cache for {instance_id}")
            head = git_output(repo_dir, ["rev-parse", "HEAD"])
            remote = git_output(repo_dir, ["remote", "get-url", "origin"])
            if head != row["base_commit"] or remote != expected_remote:
                raise QualificationError(
                    f"checkout binding mismatch for {instance_id}: {remote}@{head}"
                )
            if git_output(repo_dir, ["status", "--porcelain"]):
                raise QualificationError(f"cached checkout is dirty for {instance_id}")
            if (
                digest_file(database) != stored.get("database_sha256")
                or tree_digest(hnsw) != stored.get("hnsw_sha256")
            ):
                raise QualificationError(f"cached index digest mismatch for {instance_id}")
            mode = "reused"
            self.index_reuses += 1
            duration = float(stored["index_duration_seconds"])
        else:
            root.mkdir(parents=True, exist_ok=True)
            if not (repo_dir / ".git").exists():
                repo_dir.mkdir(parents=True, exist_ok=True)
                command(
                    ["git", "init", "-q"], cwd=repo_dir, timeout=60,
                    stage="git_init", failure_ledger=self.failure_ledger,
                    scope_digest=self.scope_digest,
                )
                command(
                    ["git", "remote", "add", "origin", expected_remote], cwd=repo_dir,
                    timeout=60, stage="git_remote_add", failure_ledger=self.failure_ledger,
                    scope_digest=self.scope_digest,
                )
            elif git_output(repo_dir, ["remote", "get-url", "origin"]) != expected_remote:
                raise QualificationError(f"partial checkout remote mismatch for {instance_id}")
            fetched = False
            last_error: Exception | None = None
            for attempt in range(1, 4):
                try:
                    command(
                        ["git", "fetch", "--depth", "1", "origin", row["base_commit"]],
                        cwd=repo_dir, timeout=600, stage=f"git_fetch_attempt_{attempt}",
                        failure_ledger=self.failure_ledger,
                        scope_digest=self.scope_digest,
                    )
                    fetched = True
                    break
                except QualificationError as error:
                    last_error = error
            if not fetched:
                raise QualificationError(
                    f"unable to fetch {row['repo']}@{row['base_commit']}"
                ) from last_error
            command(
                ["git", "checkout", "-q", "--detach", row["base_commit"]],
                cwd=repo_dir, timeout=120, stage="git_checkout",
                failure_ledger=self.failure_ledger, scope_digest=self.scope_digest,
            )
            head = git_output(repo_dir, ["rev-parse", "HEAD"])
            if head != row["base_commit"]:
                raise QualificationError(f"checkout HEAD mismatch for {instance_id}: {head}")
            if git_output(repo_dir, ["status", "--porcelain"]):
                raise QualificationError(f"fresh checkout is dirty for {instance_id}")
            if index_dir.exists():
                shutil.rmtree(index_dir)
                self.index_recoveries += 1
            hnsw.mkdir(parents=True, exist_ok=True)
            _, duration = command(
                [str(self.ce), "index", "--repo", str(repo_dir), "--db", str(database),
                 "--hnsw-dir", str(hnsw), "--full"],
                timeout=3600, stage="ce_index", failure_ledger=self.failure_ledger,
                scope_digest=self.scope_digest,
            )
            if not database.is_file() or not hnsw.is_dir():
                raise QualificationError(
                    f"ce index did not produce complete artifacts for {instance_id}"
                )
            stored = {
                "schema": "prune.e118-index-ready.v1", "identity": identity,
                "checkout_head": head, "remote_url": expected_remote,
                "database_sha256": digest_file(database), "hnsw_sha256": tree_digest(hnsw),
                "index_duration_seconds": duration, "created_at_utc": utc_now(),
            }
            write_json_atomic(marker, stored)
            mode = "created"
            self.index_creations += 1
        binding = {
            "instance_id": instance_id, "repo": row["repo"],
            "base_commit": row["base_commit"], "checkout_head": row["base_commit"],
            "remote_url": expected_remote, "cache_key": cache_key,
            "cache_path": str(root.relative_to(self.cache_dir)),
            "ce_sha256": self.ce_sha256, "database_sha256": stored["database_sha256"],
            "hnsw_sha256": stored["hnsw_sha256"],
            "index_duration_seconds": duration, "execution_mode": mode,
        }
        self.index_bindings[instance_id] = binding
        return binding

    def one(
        self, config: dict[str, Any], row: dict[str, Any], *,
        phase: str, arm: str, candidate_id: str,
    ) -> tuple[dict[str, float], str]:
        validate_config(config, self.budget)
        binding = self.ensure_index(row)
        identity = {
            "schema": "prune.e118-pack-identity.v1",
            "instance_id": row["instance_id"], "repo": row["repo"],
            "base_commit": row["base_commit"],
            "problem_statement_sha256": row["_problem_statement_sha256"],
            "config_sha256": config_digest(config), "budget_tokens": self.budget,
            "ce_sha256": self.ce_sha256, "index_cache_key": binding["cache_key"],
            "database_sha256": binding["database_sha256"],
            "hnsw_sha256": binding["hnsw_sha256"],
        }
        pack_key = digest_json(identity)
        pack_path = self.cache_dir / "packs" / pack_key[:2] / f"{pack_key}.json"
        if pack_key in self.pack_records:
            record = self.pack_records[pack_key]
            source = "memory"
            self.memory_pack_reuses += 1
        elif pack_path.exists():
            record = json.loads(pack_path.read_text())
            if record.get("identity") != identity or record.get("pack_key") != pack_key:
                raise QualificationError(f"pack cache identity mismatch: {pack_key}")
            expected = score_measurement(
                record["measurement"], row["_expect_paths"], self.budget,
            )
            if any(
                abs(expected[key] - float(record["score"][key])) > 1e-12
                for key in expected
            ):
                raise QualificationError(f"pack cache score mismatch: {pack_key}")
            source = "disk"
            self.disk_pack_reuses += 1
            self.pack_records[pack_key] = record
        else:
            completed, duration = command(
                [
                    str(self.ce), "pack", "--db",
                    str(self.cache_dir / binding["cache_path"] / "index" / "index.sqlite"),
                    "--hnsw-dir",
                    str(self.cache_dir / binding["cache_path"] / "index" / "hnsw"),
                    "--task", row["_problem_statement"], "--strategy-json",
                    json.dumps(config, sort_keys=True, separators=(",", ":")),
                    "--budget-tokens", str(self.budget), "--format", "json",
                ],
                timeout=600, stage="ce_pack", failure_ledger=self.failure_ledger,
                scope_digest=self.scope_digest,
            )
            try:
                pack = json.loads(completed.stdout)
            except json.JSONDecodeError as error:
                append_jsonl(self.failure_ledger, {
                    "schema": "prune.e118-failure.v1", "at_utc": utc_now(),
                    "scope_digest": self.scope_digest, "stage": "ce_pack_parse",
                    "kind": "invalid_json",
                    "stdout_sha256": digest_bytes(completed.stdout.encode()),
                })
                raise QualificationError(
                    f"invalid ce pack JSON for {row['instance_id']}"
                ) from error
            if not isinstance(pack, dict) or not isinstance(pack.get("items") or [], list):
                raise QualificationError(f"invalid ce pack shape for {row['instance_id']}")
            metrics = pack.get("metrics") or {}
            measurement = {
                "item_paths": sorted({
                    normalize_path(item.get("path", ""))
                    for item in (pack.get("items") or [])
                    if normalize_path(item.get("path", ""))
                }),
                "connectivity_raw": float(metrics.get("connectivity_score") or 0.0),
                "unbound_raw": float(
                    metrics.get("unbound_symbol_count")
                    or len(pack.get("unresolved_symbols") or [])
                ),
                "tokens_raw": float(
                    pack.get("used_tokens") or metrics.get("pack_tokens_total") or 0.0
                ),
            }
            score = score_measurement(measurement, row["_expect_paths"], self.budget)
            record = {
                "schema": "prune.e118-pack-record.v1", "pack_key": pack_key,
                "identity": identity, "config": config, "measurement": measurement,
                "score": score, "pack_duration_seconds": duration,
                "stdout_sha256": digest_bytes(completed.stdout.encode()),
                "created_at_utc": utc_now(),
            }
            write_json_atomic(pack_path, record)
            self.pack_records[pack_key] = record
            self.executed_pack_calls += 1
            source = "executed"
        incremental = pack_key not in self.logical_seen
        self.logical_seen.add(pack_key)
        self.access_log.append({
            "ordinal": len(self.access_log), "phase": phase, "arm": arm,
            "candidate_id": candidate_id, "instance_id": row["instance_id"],
            "split": row["_split"], "pack_key": pack_key, "source": source,
            "incremental_pack_required": incremental,
            "pack_task_sha256": row["_problem_statement_sha256"],
        })
        return dict(record["score"]), pack_key

    def aggregate(
        self, config: dict[str, Any], rows: list[dict[str, Any]], *,
        phase: str, arm: str, candidate_id: str,
    ) -> tuple[dict[str, float], list[str]]:
        values: list[dict[str, float]] = []
        keys: list[str] = []
        for row in rows:
            score, pack_key = self.one(
                config, row, phase=phase, arm=arm, candidate_id=candidate_id,
            )
            values.append(score)
            keys.append(pack_key)
        if not values:
            raise QualificationError(f"empty aggregate for {phase}/{arm}/{candidate_id}")
        aggregate = {
            key: float(np.mean([value[key] for value in values])) for key in values[0]
        }
        return aggregate, keys


def train_model(
    base: dict[str, Any], evaluator: Evaluator, train_rows: list[dict[str, Any]],
) -> tuple[tuple[np.ndarray, np.ndarray, np.ndarray], float, dict[str, Any]]:
    rng = np.random.default_rng(SEED)
    base_aggregate, base_keys = evaluator.aggregate(
        base, train_rows, phase="train", arm="shared", candidate_id="bootstrap_base",
    )
    features: list[np.ndarray] = []
    outcomes: list[float] = []
    seen: set[str] = set()
    candidates: list[dict[str, Any]] = []
    attempts = 0
    while len(features) < 24:
        attempts += 1
        spec = mutation_spec(rng)
        child = apply_mutation(base, spec)
        child_digest = config_digest(child)
        if child_digest in seen:
            continue
        seen.add(child_digest)
        aggregate, pack_keys = evaluator.aggregate(
            child, train_rows, phase="train", arm="shared",
            candidate_id=f"bootstrap_{len(features):02d}",
        )
        gain = aggregate["utility"] - base_aggregate["utility"]
        features.append(feature(base, child))
        outcomes.append(gain)
        candidates.append({
            "candidate_index": len(candidates), "mutation": spec, "config": child,
            "config_sha256": child_digest, "aggregate": aggregate,
            "gain_vs_base": gain, "pack_keys": pack_keys,
        })
    model = fit_ridge(features, outcomes)
    calibrations: list[dict[str, Any]] = []
    margins: list[float] = []
    for calibration_index in range(16):
        specs = [mutation_spec(rng) for _ in range(12)]
        siblings = [apply_mutation(base, spec) for spec in specs]
        scores = predict(model, [feature(base, child) for child in siblings])
        ordered = np.argsort(scores)[::-1]
        margin = float(scores[ordered[0]] - scores[ordered[1]])
        margins.append(margin)
        calibrations.append({
            "calibration_index": calibration_index, "mutations": specs,
            "candidate_configs": siblings,
            "predicted_scores": [float(score) for score in scores], "margin": margin,
        })
    gate = float(np.median(margins))
    mean, standard_deviation, beta = model
    evidence = {
        "base": {"config": base, "aggregate": base_aggregate, "pack_keys": base_keys},
        "unique_candidate_attempts": attempts, "candidates": candidates,
        "ridge": {
            "regularization": 1.0,
            "feature_order": [
                "normalized_parent", "normalized_delta",
                "normalized_parent_times_delta",
            ],
            "mean": mean.tolist(), "standard_deviation": standard_deviation.tolist(),
            "beta": beta.tolist(),
        },
        "gate_calibrations": calibrations, "raw_margin_gate": gate,
    }
    return model, gate, evidence


def shared_candidate_schedule() -> list[list[dict[str, Any]]]:
    rng = np.random.default_rng(SEED + 1)
    return [[mutation_spec(rng) for _ in range(12)] for _ in range(10)]


def trajectory(
    kind: str, base: dict[str, Any], evaluator: Evaluator,
    model: tuple[np.ndarray, np.ndarray, np.ndarray], gate: float,
    probe_rows: list[dict[str, Any]], schedule: list[list[dict[str, Any]]],
) -> tuple[dict[str, Any], dict[str, Any]]:
    current = copy.deepcopy(base)
    probe_shards = split_shards(probe_rows)
    generations: list[dict[str, Any]] = []
    for generation, specs in enumerate(schedule):
        siblings = [apply_mutation(current, spec) for spec in specs]
        scores = predict(model, [feature(current, child) for child in siblings])
        order = np.argsort(scores)[::-1]
        selected_index = int(order[0])
        triggered = False
        probe_evidence: dict[str, Any] | None = None
        margin = float(scores[order[0]] - scores[order[1]])
        if kind == "causal" and margin < gate:
            triggered = True
            finalist_indices = [int(index) for index in order[:4]]
            shard = probe_shards[generation % len(probe_shards)]
            parent_aggregate, parent_keys = evaluator.aggregate(
                current, shard, phase="probe", arm="causal",
                candidate_id=f"generation_{generation:02d}_parent",
            )
            finalists: list[dict[str, Any]] = []
            gains: list[float] = []
            for candidate_index in finalist_indices:
                aggregate, pack_keys = evaluator.aggregate(
                    siblings[candidate_index], shard, phase="probe", arm="causal",
                    candidate_id=(
                        f"generation_{generation:02d}_candidate_{candidate_index:02d}"
                    ),
                )
                gain = aggregate["utility"] - parent_aggregate["utility"]
                gains.append(gain)
                finalists.append({
                    "candidate_index": candidate_index, "aggregate": aggregate,
                    "probe_gain": gain, "pack_keys": pack_keys,
                })
            selected_index = finalist_indices[int(np.argmax(gains))]
            probe_evidence = {
                "shard_instance_ids": [row["instance_id"] for row in shard],
                "parent_aggregate": parent_aggregate, "parent_pack_keys": parent_keys,
                "finalists": finalists,
            }
        parent = current
        current = siblings[selected_index]
        generations.append({
            "generation": generation, "parent_config": parent, "mutations": specs,
            "candidate_configs": siblings,
            "predicted_scores": [float(score) for score in scores],
            "raw_order": [int(index) for index in order], "raw_margin": margin,
            "triggered": triggered, "probe": probe_evidence,
            "selected_index": selected_index, "selected_config": current,
            "selected_raw_rank": int(np.where(order == selected_index)[0][0]) + 1,
        })
    evidence = {
        "kind": kind, "generations": generations, "final_config": current,
        "final_config_sha256": config_digest(current),
    }
    return current, evidence


def evaluate_final_arm(
    config: dict[str, Any], evaluator: Evaluator,
    rows: list[dict[str, Any]], arm: str,
) -> tuple[dict[str, float], dict[str, dict[str, Any]]]:
    per_instance: dict[str, dict[str, Any]] = {}
    values: list[dict[str, float]] = []
    for row in rows:
        score, pack_key = evaluator.one(
            config, row, phase="evaluation", arm=arm, candidate_id=f"final_{arm}",
        )
        values.append(score)
        per_instance[row["instance_id"]] = {"pack_key": pack_key, "score": score}
    aggregate = {
        key: float(np.mean([value[key] for value in values])) for key in values[0]
    }
    return aggregate, per_instance


def repository_means(
    per_instance: dict[str, dict[str, Any]],
    manifest_by_id: dict[str, dict[str, Any]],
) -> dict[str, float]:
    by_repo: dict[str, list[float]] = defaultdict(list)
    for instance_id, record in per_instance.items():
        by_repo[manifest_by_id[instance_id]]["repo"].append(
            float(record["score"]["utility"])
        )
    return {repo: float(np.mean(values)) for repo, values in sorted(by_repo.items())}


def classify(
    delta: float, wins: int, repositories: int, probe_cost_ratio: float,
) -> str:
    if delta >= 0.01 and wins > repositories / 2 and probe_cost_ratio <= 0.35:
        return "SWE_BENCH_SPARSE_CAUSAL_PROMISING"
    if delta > 0 and wins >= repositories / 2 and probe_cost_ratio <= 0.35:
        return "SWE_BENCH_SPARSE_CAUSAL_WEAK_SIGNAL"
    if delta <= -0.01:
        return "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED"
    return "SWE_BENCH_SPARSE_CAUSAL_INCONCLUSIVE"


def load_failures(path: Path, scope_digest: str) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    failures = []
    for line in path.read_text().splitlines():
        if line.strip():
            event = json.loads(line)
            if event.get("scope_digest") == scope_digest:
                failures.append(event)
    return failures


def source_provenance(
    repo_root: Path, files: dict[str, Path],
) -> dict[str, Any]:
    status = git_output(repo_root, ["status", "--porcelain", "--untracked-files=no"])
    if status:
        raise QualificationError(
            f"tracked source state must be clean before confirmation: {status}"
        )
    commit = git_output(repo_root, ["rev-parse", "HEAD"])
    return {
        "source_commit": commit,
        "source_branch": git_output(repo_root, ["branch", "--show-current"]),
        "source_worktree": str(repo_root), "source_worktree_clean": True,
        "source_files": {
            name: {"path": str(path.relative_to(repo_root)), "sha256": digest_file(path)}
            for name, path in files.items()
        },
        "python_version": sys.version, "platform": sys.platform,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ce", default="ce")
    parser.add_argument("--cache", type=Path, default=Path(".e118-cache"))
    parser.add_argument(
        "--base", type=Path, default=Path("experiments/E118-base-strategy.json"),
    )
    parser.add_argument(
        "--plan", type=Path,
        default=Path("experiments/E118-swebench-sparse-causal-prune-plan.json"),
    )
    parser.add_argument(
        "--amendment", type=Path,
        default=Path("experiments/E118-qualification-amendment-v1.json"),
    )
    parser.add_argument(
        "--requirements", type=Path,
        default=Path("experiments/E118-requirements.txt"),
    )
    parser.add_argument("--budget-tokens", type=int, default=12000)
    parser.add_argument(
        "--out", type=Path,
        default=Path("experiments/E118-swebench-sparse-causal-prune.json"),
    )
    parser.add_argument("--confirm", action="store_true")
    args = parser.parse_args()

    repo_root = Path(git_output(Path.cwd(), ["rev-parse", "--show-toplevel"])).resolve()
    runner_path = Path(__file__).resolve()
    plan_path, base_path = args.plan.resolve(), args.base.resolve()
    amendment_path, requirements_path = args.amendment.resolve(), args.requirements.resolve()
    ce_path = Path(shutil.which(args.ce) or args.ce).resolve()
    if not ce_path.is_file():
        raise QualificationError(f"ce binary not found: {ce_path}")
    if digest_file(plan_path) != PLAN_SHA256:
        raise QualificationError("frozen E118 plan digest mismatch")
    if digest_file(base_path) != BASE_SHA256:
        raise QualificationError("frozen E118 base strategy digest mismatch")
    base = json.loads(base_path.read_text())
    validate_config(base, args.budget_tokens)
    rows, dataset_metadata = load_swebench()
    selected, repository_split = select_and_split(rows)
    manifest = public_manifest(selected)
    manifest_digest = digest_json(manifest)
    counts = {
        split: sum(row["_split"] == split for row in selected)
        for split in ("train", "probe", "evaluation")
    }
    qualification = {
        "dataset": dataset_metadata, "eligible_instances": len(rows),
        "selected_instances": len(selected), "instance_counts": counts,
        "repository_counts": {
            split: len(repositories) for split, repositories in repository_split.items()
        },
        "manifest_sha256": manifest_digest, "prune_pack_calls": 0,
    }
    if not args.confirm:
        print(json.dumps(qualification, indent=2, sort_keys=True))
        return

    out_path = args.out.resolve()
    receipt_path = out_path.with_suffix(out_path.suffix + ".sha256")
    if out_path.exists() or receipt_path.exists():
        raise QualificationError(f"immutable result or receipt already exists: {out_path}")
    provenance = source_provenance(repo_root, {
        "runner": runner_path, "plan": plan_path, "base_strategy": base_path,
        "qualification_amendment": amendment_path, "requirements": requirements_path,
        "independent_verifier": runner_path.with_name(
            "verify_e118_swebench_sparse_causal_prune.py"
        ),
        "corruption_tests": runner_path.with_name("test_e118_verifier_corruption.py"),
        "protocol_tests": runner_path.parents[1] / "tests" / "test_e118_protocol.py",
        "runbook": runner_path.parents[1] / "experiments" / "E118-RUNBOOK.md",
    })
    scope = {
        "experiment_id": "E118-swebench-sparse-causal-prune",
        "source_commit": provenance["source_commit"], "plan_sha256": PLAN_SHA256,
        "base_sha256": BASE_SHA256, "amendment_sha256": digest_file(amendment_path),
        "dataset_revision": DATASET_REVISION, "manifest_sha256": manifest_digest,
        "ce_sha256": digest_file(ce_path), "budget_tokens": args.budget_tokens,
    }
    scope_digest = digest_json(scope)
    failure_ledger = args.cache.resolve() / "failure-ledger.jsonl"
    evaluator = Evaluator(
        ce_path, args.cache, args.budget_tokens, scope_digest, failure_ledger,
    )
    groups = {
        split: [row for row in selected if row["_split"] == split]
        for split in ("train", "probe", "evaluation")
    }
    started = time.monotonic()
    model, gate, bootstrap = train_model(base, evaluator, groups["train"])
    schedule = shared_candidate_schedule()
    raw_strategy, raw_trajectory = trajectory(
        "raw", base, evaluator, model, gate, groups["probe"], schedule,
    )
    causal_strategy, causal_trajectory = trajectory(
        "causal", base, evaluator, model, gate, groups["probe"], schedule,
    )
    trajectory_complete_ordinal = len(evaluator.access_log)
    base_evaluation, base_per_instance = evaluate_final_arm(
        base, evaluator, groups["evaluation"], "base",
    )
    raw_evaluation, raw_per_instance = evaluate_final_arm(
        raw_strategy, evaluator, groups["evaluation"], "raw",
    )
    causal_evaluation, causal_per_instance = evaluate_final_arm(
        causal_strategy, evaluator, groups["evaluation"], "causal",
    )
    raw_gain = raw_evaluation["utility"] - base_evaluation["utility"]
    causal_gain = causal_evaluation["utility"] - base_evaluation["utility"]
    delta = causal_gain - raw_gain
    manifest_by_id = {row["instance_id"]: row for row in manifest}
    repository_base = repository_means(base_per_instance, manifest_by_id)
    repository_raw = repository_means(raw_per_instance, manifest_by_id)
    repository_causal = repository_means(causal_per_instance, manifest_by_id)
    wins = sum(
        repository_causal[repo] > repository_raw[repo] for repo in repository_causal
    )
    repository_count = len(repository_causal)
    probe_unique = sum(
        event["phase"] == "probe" and event["incremental_pack_required"]
        for event in evaluator.access_log
    )
    naive_probe_cost = 10 * 5 * len(groups["probe"])
    probe_cost_ratio = probe_unique / max(1, naive_probe_cost)
    decision = classify(delta, wins, repository_count, probe_cost_ratio)
    process_wall = time.monotonic() - started

    transitions: list[dict[str, Any]] = []
    for arm, evidence in (("raw", raw_trajectory), ("causal", causal_trajectory)):
        for generation in evidence["generations"]:
            selected_index = generation["selected_index"]
            probe_response = None
            cost_keys: list[str] = []
            if generation["probe"]:
                selected_probe = next(
                    item for item in generation["probe"]["finalists"]
                    if item["candidate_index"] == selected_index
                )
                probe_response = selected_probe["probe_gain"]
                cost_keys = generation["probe"]["parent_pack_keys"] + [
                    key for item in generation["probe"]["finalists"]
                    for key in item["pack_keys"]
                ]
            final_generation = generation["generation"] == 9
            held_out_gain = (
                causal_gain if arm == "causal" and final_generation
                else raw_gain if arm == "raw" and final_generation else None
            )
            transitions.append({
                "arm": arm, "generation": generation["generation"],
                "x_t": generation["parent_config"],
                "candidate_delta_x": generation["mutations"][selected_index],
                "raw_predicted_gain": generation["predicted_scores"][selected_index],
                "probe_response": probe_response, "chosen_action": selected_index,
                "actual_held_out_gain": held_out_gain,
                "actual_held_out_gain_reference": (
                    "final_strategy_vs_common_base" if final_generation
                    else "not_measured_to_preserve_evaluation_and_cost_protocol"
                ),
                "task_repository_identity": (
                    generation["probe"]["shard_instance_ids"] if generation["probe"] else []
                ),
                "cost_pack_keys": cost_keys,
            })

    no_op_candidates = 0
    duplicate_candidates = 0
    for evidence in (raw_trajectory, causal_trajectory):
        for generation in evidence["generations"]:
            parent_digest = config_digest(generation["parent_config"])
            child_digests = [
                config_digest(config) for config in generation["candidate_configs"]
            ]
            no_op_candidates += sum(
                child_digest == parent_digest for child_digest in child_digests
            )
            duplicate_candidates += len(child_digests) - len(set(child_digests))
    failures = load_failures(failure_ledger, scope_digest)
    cost = {
        "logical_pack_accesses": len(evaluator.access_log),
        "unique_pack_evaluations": len(evaluator.logical_seen),
        "pack_executions_this_process": evaluator.executed_pack_calls,
        "disk_pack_cache_reuses": evaluator.disk_pack_reuses,
        "memory_pack_cache_reuses": evaluator.memory_pack_reuses,
        "index_creations_this_process": evaluator.index_creations,
        "index_reuses_this_process": evaluator.index_reuses,
        "index_partial_recoveries": evaluator.index_recoveries,
        "index_bindings": len(evaluator.index_bindings),
        "probe_pack_evaluations": probe_unique,
        "naive_probe_pack_evaluations": naive_probe_cost,
        "probe_cost_ratio": probe_cost_ratio,
        "bootstrap_measured_candidates": 24,
        "gate_calibration_predicted_candidates": 16 * 12,
        "raw_trajectory_predicted_candidates": 10 * 12,
        "causal_trajectory_predicted_candidates": 10 * 12,
        "invalid_candidates": 0, "no_op_candidates": no_op_candidates,
        "duplicate_sibling_candidates": duplicate_candidates,
        "recorded_failures": len(failures),
        "pack_wall_seconds": float(sum(
            record["pack_duration_seconds"] for record in evaluator.pack_records.values()
        )),
        "index_wall_seconds": float(sum(
            binding["index_duration_seconds"]
            for binding in evaluator.index_bindings.values()
        )),
        "successful_process_wall_seconds": process_wall,
    }
    result = {
        "schema": "prune.e118-result.v2",
        "experiment_id": "E118-swebench-sparse-causal-prune",
        "created_at_utc": utc_now(), "decision": decision,
        "provenance": provenance, "scope": scope, "scope_sha256": scope_digest,
        "dataset": dataset_metadata,
        "protocol": {
            "seed": SEED, "budget_tokens": args.budget_tokens, "model_calls": 0,
            "generations": 10, "siblings_per_generation": 12,
            "bootstrap_mutations": 24, "probe_finalists": 4, "probe_shards": 8,
            "genes": list(GENES),
            "bounds": {gene: list(bounds) for gene, bounds in BOUNDS.items()},
            "steps": {gene: list(steps) for gene, steps in STEPS.items()},
        },
        "manifest": {
            "sha256": manifest_digest, "selected_instances": manifest,
            "selected_instance_count": len(manifest),
            "eligible_instance_count": len(rows), "instance_counts": counts,
            "repository_split": repository_split,
            "repository_counts": {
                split: len(repositories)
                for split, repositories in repository_split.items()
            },
        },
        "anti_leakage": {
            "forbidden_policy_fields": sorted(FORBIDDEN_POLICY_FIELDS),
            "policy_task_field": "problem_statement",
            "pack_task_matches_problem_statement_for_all_instances": all(
                row["pack_task_sha256"] == row["problem_statement_sha256"]
                for row in manifest
            ),
            "gold_patch_stored": False,
            "evaluation_repo_checkout_or_pack_before_trajectories": False,
            "trajectory_complete_access_ordinal": trajectory_complete_ordinal,
        },
        "index_bindings": [
            evaluator.index_bindings[instance_id]
            for instance_id in sorted(evaluator.index_bindings)
        ],
        "pack_records": [
            evaluator.pack_records[key] for key in sorted(evaluator.pack_records)
        ],
        "access_log": evaluator.access_log, "bootstrap": bootstrap,
        "candidate_opportunity_schedule": schedule,
        "raw_trajectory": raw_trajectory, "causal_trajectory": causal_trajectory,
        "final_strategies": {
            "base": {"config": base, "sha256": config_digest(base)},
            "raw": {"config": raw_strategy, "sha256": config_digest(raw_strategy)},
            "causal": {"config": causal_strategy, "sha256": config_digest(causal_strategy)},
        },
        "evaluation": {
            "base": {
                "aggregate": base_evaluation, "per_instance": base_per_instance,
                "per_repository_utility": repository_base,
            },
            "raw": {
                "aggregate": raw_evaluation, "per_instance": raw_per_instance,
                "per_repository_utility": repository_raw,
            },
            "causal": {
                "aggregate": causal_evaluation, "per_instance": causal_per_instance,
                "per_repository_utility": repository_causal,
            },
            "raw_gain": raw_gain, "causal_gain": causal_gain,
            "causal_minus_raw": delta, "causal_repository_wins": wins,
            "evaluation_repository_count": repository_count,
        },
        "cost": cost, "failures": failures, "transitions": transitions,
        "claim_boundary": (
            "Stage-A context-selection methodology result only; no SWE-bench "
            "issue-resolution, coding-agent capability, recursive self-improvement, "
            "or multi-generation compounding claim."
        ),
    }
    serialized = json.dumps(result, indent=2, sort_keys=True, allow_nan=False) + "\n"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("x", encoding="utf-8") as handle:
        handle.write(serialized)
        handle.flush()
        os.fsync(handle.fileno())
    receipt = {
        "schema": "prune.e118-result-receipt.v1", "result_file": out_path.name,
        "result_sha256": digest_bytes(serialized.encode()),
        "source_commit": provenance["source_commit"], "scope_sha256": scope_digest,
    }
    with receipt_path.open("x", encoding="utf-8") as handle:
        handle.write(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        handle.flush()
        os.fsync(handle.fileno())
    print(json.dumps({
        "decision": decision, "result": str(out_path),
        "receipt": str(receipt_path), "cost": cost,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
