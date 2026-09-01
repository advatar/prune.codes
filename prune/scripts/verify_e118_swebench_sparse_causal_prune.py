#!/usr/bin/env python3
"""Independent evidence-only verifier for E118.

This module intentionally does not import the E118 runner and never calls Prune,
checks out a SWE-bench repository, or reruns search.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import re
import subprocess
from collections import defaultdict
from pathlib import Path
from typing import Any

import numpy as np

DATASET = "SWE-bench/SWE-bench_Multilingual"
DATASET_REVISION = "846e647b9f33c0b51b739d005d13d85493c9af09"
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
SCORE_KEYS = ("utility", "path_recall", "connectivity", "unbound", "tokens")


class VerificationError(RuntimeError):
    """The frozen E118 evidence is inconsistent or corrupt."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


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


def close(actual: float, expected: float, label: str, tolerance: float = 1e-10) -> None:
    require(math.isfinite(float(actual)), f"{label} is non-finite")
    require(
        math.isclose(float(actual), float(expected), rel_tol=tolerance, abs_tol=tolerance),
        f"{label} mismatch: {actual} != {expected}",
    )


def close_mapping(actual: dict[str, Any], expected: dict[str, Any], label: str) -> None:
    require(set(actual) == set(expected), f"{label} keys mismatch")
    for key in expected:
        close(float(actual[key]), float(expected[key]), f"{label}.{key}")


def git_blob(repo: Path, commit: str, path: str) -> bytes:
    result = subprocess.run(
        ["git", "show", f"{commit}:{path}"], cwd=repo, capture_output=True, check=False,
    )
    require(result.returncode == 0, f"source blob unavailable: {commit}:{path}")
    return result.stdout


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
    return {
        "utility": float(utility), "path_recall": float(recall),
        "connectivity": connectivity, "unbound": unbound, "tokens": tokens,
    }


def validate_config(config: dict[str, Any], budget: int, label: str) -> None:
    require(all(gene in config for gene in GENES), f"{label} missing genes")
    for gene in GENES:
        lower, upper = BOUNDS[gene]
        require(lower <= float(config[gene]) <= upper, f"{label}.{gene} out of range")
    require(int(config.get("budget_tokens", -1)) == budget, f"{label} budget drift")


def vector(config: dict[str, Any]) -> np.ndarray:
    return np.asarray([
        (float(config[gene]) - BOUNDS[gene][0]) / (BOUNDS[gene][1] - BOUNDS[gene][0])
        for gene in GENES
    ])


def feature(parent: dict[str, Any], child: dict[str, Any]) -> np.ndarray:
    parent_vector = vector(parent)
    child_vector = vector(child)
    delta = child_vector - parent_vector
    return np.r_[parent_vector, delta, parent_vector * delta]


def mutation_spec(rng: np.random.Generator) -> dict[str, Any]:
    gene = str(rng.choice(GENES))
    magnitude = float(rng.choice(STEPS[gene]))
    direction = -1 if rng.random() < 0.5 else 1
    return {"gene": gene, "signed_step": magnitude * direction}


def apply_mutation(config: dict[str, Any], spec: dict[str, Any]) -> dict[str, Any]:
    child = copy.deepcopy(config)
    gene = spec["gene"]
    require(gene in GENES, f"unknown mutation gene: {gene}")
    require(abs(float(spec["signed_step"])) in STEPS[gene], f"invalid mutation step: {spec}")
    lower, upper = BOUNDS[gene]
    value = float(child[gene]) + float(spec["signed_step"])
    if gene != "hybrid_alpha":
        value = int(round(value))
    child[gene] = max(lower, min(upper, value))
    return child


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


def mean_scores(pack_keys: list[str], packs: dict[str, dict[str, Any]]) -> dict[str, float]:
    require(bool(pack_keys), "empty pack-key aggregate")
    require(len(pack_keys) == len(set(pack_keys)), "duplicate pack key within aggregate")
    records = [packs[key]["score"] for key in pack_keys]
    return {key: float(np.mean([record[key] for record in records])) for key in SCORE_KEYS}


def require_pack_group(
    pack_keys: list[str], packs: dict[str, dict[str, Any]], config: dict[str, Any],
    expected_instance_ids: set[str], label: str,
) -> None:
    require(
        {packs[key]["identity"]["instance_id"] for key in pack_keys}
        == expected_instance_ids,
        f"{label} task set mismatch",
    )
    expected_config_digest = digest_json(config)
    require(
        all(
            packs[key]["identity"]["config_sha256"] == expected_config_digest
            for key in pack_keys
        ),
        f"{label} config mismatch",
    )


def split_shard_ids(probe_manifest: list[dict[str, Any]], count: int = 8) -> list[list[str]]:
    shards: list[list[str]] = [[] for _ in range(count)]
    for row in probe_manifest:
        instance_id = row["instance_id"]
        shards[hashlib.sha256(instance_id.encode()).digest()[1] % count].append(instance_id)
    return [shard for shard in shards if shard]


def classify(delta: float, wins: int, repositories: int, probe_cost_ratio: float) -> str:
    if delta >= 0.01 and wins > repositories / 2 and probe_cost_ratio <= 0.35:
        return "SWE_BENCH_SPARSE_CAUSAL_PROMISING"
    if delta > 0 and wins >= repositories / 2 and probe_cost_ratio <= 0.35:
        return "SWE_BENCH_SPARSE_CAUSAL_WEAK_SIGNAL"
    if delta <= -0.01:
        return "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED"
    return "SWE_BENCH_SPARSE_CAUSAL_INCONCLUSIVE"


def verify_result(
    result: dict[str, Any], *, repo_root: Path, plan_path: Path, base_path: Path,
) -> dict[str, Any]:
    require(result.get("schema") == "prune.e118-result.v2", "result schema mismatch")
    require(
        result.get("experiment_id") == "E118-swebench-sparse-causal-prune",
        "experiment ID mismatch",
    )
    require(digest_file(plan_path) == PLAN_SHA256, "local plan digest mismatch")
    require(digest_file(base_path) == BASE_SHA256, "local base digest mismatch")
    protocol = result["protocol"]
    require(protocol["seed"] == SEED, "seed mismatch")
    require(protocol["model_calls"] == 0, "model-call count must be zero")
    require(protocol["generations"] == 10, "generation count mismatch")
    require(protocol["siblings_per_generation"] == 12, "sibling count mismatch")
    require(protocol["bootstrap_mutations"] == 24, "bootstrap count mismatch")
    require(protocol["probe_finalists"] == 4, "probe-finalist count mismatch")
    require(protocol["probe_shards"] == 8, "probe-shard count mismatch")
    require(protocol["genes"] == list(GENES), "gene order mismatch")
    require(protocol["bounds"] == {gene: list(bounds) for gene, bounds in BOUNDS.items()}, "bounds mismatch")
    require(protocol["steps"] == {gene: list(steps) for gene, steps in STEPS.items()}, "steps mismatch")
    budget = int(protocol["budget_tokens"])

    scope = result["scope"]
    require(digest_json(scope) == result["scope_sha256"], "scope digest mismatch")
    require(scope["plan_sha256"] == PLAN_SHA256, "scope plan digest mismatch")
    require(scope["base_sha256"] == BASE_SHA256, "scope base digest mismatch")
    require(scope["dataset_revision"] == DATASET_REVISION, "scope dataset mismatch")
    provenance = result["provenance"]
    commit = provenance["source_commit"]
    require(re.fullmatch(r"[0-9a-f]{40}", commit) is not None, "invalid source commit")
    require(scope["source_commit"] == commit, "scope/source commit mismatch")
    require(provenance["source_worktree_clean"] is True, "source was not clean")
    for name, source_file in provenance["source_files"].items():
        blob = git_blob(repo_root, commit, source_file["path"])
        require(digest_bytes(blob) == source_file["sha256"], f"source digest mismatch: {name}")
    require(provenance["source_files"]["plan"]["sha256"] == PLAN_SHA256, "source plan mismatch")
    require(provenance["source_files"]["base_strategy"]["sha256"] == BASE_SHA256, "source base mismatch")
    require(
        provenance["source_files"]["confirmation_interruption"]["sha256"]
        == scope["interruption_sha256"],
        "interruption evidence digest mismatch",
    )

    dataset = result["dataset"]
    require(dataset["name"] == DATASET, "dataset name mismatch")
    require(dataset["revision"] == DATASET_REVISION, "dataset revision mismatch")
    require(dataset["split"] == "test", "dataset split mismatch")
    require(dataset["rows"] == 300, "dataset row count mismatch")
    require(dataset["datasets_fingerprint"] == DATASET_FINGERPRINT, "dataset fingerprint mismatch")
    require(dataset["parquet_sha256"] == DATASET_PARQUET_SHA256, "dataset parquet digest mismatch")

    manifest_wrapper = result["manifest"]
    manifest = manifest_wrapper["selected_instances"]
    require(digest_json(manifest) == manifest_wrapper["sha256"], "manifest digest mismatch")
    require(scope["manifest_sha256"] == manifest_wrapper["sha256"], "scope manifest mismatch")
    require(len(manifest) == manifest_wrapper["selected_instance_count"] == 57, "selected count mismatch")
    require(manifest_wrapper["eligible_instance_count"] == 64, "eligible count mismatch")
    manifest_by_id = {row["instance_id"]: row for row in manifest}
    require(len(manifest_by_id) == len(manifest), "duplicate selected instance")
    repository_membership: dict[str, set[str]] = defaultdict(set)
    calculated_counts = {"train": 0, "probe": 0, "evaluation": 0}
    for row in manifest:
        require(re.fullmatch(r"[0-9a-f]{40}", row["base_commit"]) is not None, "invalid base commit")
        require(row["split"] in calculated_counts, "invalid split")
        require(row["language"] in {"Rust", "TypeScript"}, "invalid language")
        require(digest_json(row["expected_paths"]) == row["expected_paths_sha256"], "truth-path digest mismatch")
        require(row["pack_task_sha256"] == row["problem_statement_sha256"], "pack task is not problem statement")
        require(row["pack_task_sha256"] != row["gold_patch_sha256"], "gold patch used as pack task")
        calculated_counts[row["split"]] += 1
        repository_membership[row["repo"]].add(row["split"])
    require(calculated_counts == manifest_wrapper["instance_counts"], "instance split counts mismatch")
    require(all(len(splits) == 1 for splits in repository_membership.values()), "repository split overlap")
    flattened_repos = {
        repo: split for split, repos in manifest_wrapper["repository_split"].items()
        for repo in repos
    }
    require(len(flattened_repos) == 13, "repository count mismatch")
    require(
        all(flattened_repos[row["repo"]] == row["split"] for row in manifest),
        "repository membership mismatch",
    )
    require(
        {key: len(value) for key, value in manifest_wrapper["repository_split"].items()}
        == manifest_wrapper["repository_counts"],
        "repository split counts mismatch",
    )
    anti_leakage = result["anti_leakage"]
    require(anti_leakage["gold_patch_stored"] is False, "gold patch stored")
    require(
        anti_leakage["pack_task_matches_problem_statement_for_all_instances"] is True,
        "pack-task leakage assertion failed",
    )
    require(
        anti_leakage["evaluation_repo_checkout_or_pack_before_trajectories"] is False,
        "evaluation repository accessed before trajectories",
    )

    bindings = {binding["instance_id"]: binding for binding in result["index_bindings"]}
    require(len(bindings) == len(manifest), "index-binding count mismatch")
    ce_sha256 = scope["ce_sha256"]
    for instance_id, row in manifest_by_id.items():
        require(instance_id in bindings, f"missing index binding: {instance_id}")
        binding = bindings[instance_id]
        require(binding["repo"] == row["repo"], f"index repo mismatch: {instance_id}")
        require(binding["base_commit"] == row["base_commit"], f"index base mismatch: {instance_id}")
        require(binding["checkout_head"] == row["base_commit"], f"checkout HEAD mismatch: {instance_id}")
        require(binding["remote_url"] == f"https://github.com/{row['repo']}.git", f"remote mismatch: {instance_id}")
        require(binding["ce_sha256"] == ce_sha256, f"index ce mismatch: {instance_id}")
        index_identity = {
            "schema": "prune.e118-index-identity.v1", "instance_id": instance_id,
            "repo": row["repo"], "base_commit": row["base_commit"],
            "ce_sha256": ce_sha256,
        }
        require(digest_json(index_identity) == binding["cache_key"], f"index cache key mismatch: {instance_id}")
        require(re.fullmatch(r"[0-9a-f]{64}", binding["database_sha256"]) is not None, "invalid DB digest")
        require(re.fullmatch(r"[0-9a-f]{64}", binding["hnsw_sha256"]) is not None, "invalid HNSW digest")

    packs = {record["pack_key"]: record for record in result["pack_records"]}
    require(len(packs) == len(result["pack_records"]), "duplicate pack record")
    for pack_key, record in packs.items():
        identity = record["identity"]
        require(digest_json(identity) == pack_key, f"pack identity digest mismatch: {pack_key}")
        instance_id = identity["instance_id"]
        require(instance_id in manifest_by_id, f"unknown pack instance: {instance_id}")
        row = manifest_by_id[instance_id]
        binding = bindings[instance_id]
        require(identity["repo"] == row["repo"], f"pack repo mismatch: {pack_key}")
        require(identity["base_commit"] == row["base_commit"], f"pack base mismatch: {pack_key}")
        require(identity["problem_statement_sha256"] == row["problem_statement_sha256"], f"pack task mismatch: {pack_key}")
        require(identity["budget_tokens"] == budget, f"pack budget mismatch: {pack_key}")
        require(identity["ce_sha256"] == ce_sha256, f"pack ce mismatch: {pack_key}")
        require(identity["index_cache_key"] == binding["cache_key"], f"pack index mismatch: {pack_key}")
        require(identity["database_sha256"] == binding["database_sha256"], f"pack DB mismatch: {pack_key}")
        require(identity["hnsw_sha256"] == binding["hnsw_sha256"], f"pack HNSW mismatch: {pack_key}")
        validate_config(record["config"], budget, f"pack {pack_key} config")
        require(digest_json(record["config"]) == identity["config_sha256"], f"pack config digest mismatch: {pack_key}")
        score = score_measurement(record["measurement"], row["expected_paths"], budget)
        close_mapping(record["score"], score, f"pack {pack_key} score")

    access_log = result["access_log"]
    boundary = anti_leakage["trajectory_complete_access_ordinal"]
    first_seen: set[str] = set()
    for ordinal, event in enumerate(access_log):
        require(event["ordinal"] == ordinal, f"access ordinal mismatch: {ordinal}")
        pack_key = event["pack_key"]
        require(pack_key in packs, f"access references missing pack: {pack_key}")
        row = manifest_by_id[event["instance_id"]]
        require(event["split"] == row["split"], f"access split mismatch: {ordinal}")
        require(event["pack_task_sha256"] == row["problem_statement_sha256"], f"access task mismatch: {ordinal}")
        required = pack_key not in first_seen
        require(event["incremental_pack_required"] is required, f"incremental-cost mismatch: {ordinal}")
        first_seen.add(pack_key)
        if event["phase"] == "train":
            require(row["split"] == "train" and ordinal < boundary, "non-train data used for fitting")
        elif event["phase"] == "probe":
            require(row["split"] == "probe" and ordinal < boundary, "invalid probe access")
        elif event["phase"] == "evaluation":
            require(row["split"] == "evaluation" and ordinal >= boundary, "early evaluation access")
        else:
            raise VerificationError(f"unknown access phase: {event['phase']}")
    require(set(packs) == first_seen, "pack records are not exactly the logically used records")

    base = result["final_strategies"]["base"]["config"]
    validate_config(base, budget, "base")
    require(digest_json(base) == result["final_strategies"]["base"]["sha256"], "base digest mismatch")
    bootstrap = result["bootstrap"]
    require(bootstrap["base"]["config"] == base, "bootstrap base mismatch")
    close_mapping(
        bootstrap["base"]["aggregate"], mean_scores(bootstrap["base"]["pack_keys"], packs),
        "bootstrap base aggregate",
    )
    train_ids = {row["instance_id"] for row in manifest if row["split"] == "train"}
    require_pack_group(
        bootstrap["base"]["pack_keys"], packs, base, train_ids, "bootstrap base",
    )
    candidates = bootstrap["candidates"]
    require(len(candidates) == 24, "bootstrap candidate count mismatch")
    rng = np.random.default_rng(SEED)
    expected_candidates: list[tuple[dict[str, Any], dict[str, Any]]] = []
    seen_configs: set[str] = set()
    attempts = 0
    while len(expected_candidates) < 24:
        attempts += 1
        spec = mutation_spec(rng)
        child = apply_mutation(base, spec)
        child_digest = digest_json(child)
        if child_digest in seen_configs:
            continue
        seen_configs.add(child_digest)
        expected_candidates.append((spec, child))
    require(attempts == bootstrap["unique_candidate_attempts"], "bootstrap attempt count mismatch")
    features: list[np.ndarray] = []
    outcomes: list[float] = []
    for index, (candidate, expected) in enumerate(zip(candidates, expected_candidates)):
        spec, child = expected
        require(candidate["candidate_index"] == index, f"bootstrap index mismatch: {index}")
        require(candidate["mutation"] == spec, f"bootstrap mutation mismatch: {index}")
        require(candidate["config"] == child, f"bootstrap child mismatch: {index}")
        require(digest_json(child) == candidate["config_sha256"], f"bootstrap digest mismatch: {index}")
        aggregate = mean_scores(candidate["pack_keys"], packs)
        require_pack_group(
            candidate["pack_keys"], packs, child, train_ids,
            f"bootstrap candidate {index}",
        )
        close_mapping(candidate["aggregate"], aggregate, f"bootstrap aggregate {index}")
        gain = aggregate["utility"] - bootstrap["base"]["aggregate"]["utility"]
        close(candidate["gain_vs_base"], gain, f"bootstrap gain {index}")
        features.append(feature(base, child))
        outcomes.append(gain)
    model = fit_ridge(features, outcomes)
    ridge = bootstrap["ridge"]
    require(ridge["regularization"] == 1.0, "ridge regularization mismatch")
    for label, actual, expected in (
        ("ridge mean", ridge["mean"], model[0]),
        ("ridge standard deviation", ridge["standard_deviation"], model[1]),
        ("ridge beta", ridge["beta"], model[2]),
    ):
        require(np.allclose(np.asarray(actual), expected, rtol=1e-10, atol=1e-10), f"{label} mismatch")
    margins: list[float] = []
    calibrations = bootstrap["gate_calibrations"]
    require(len(calibrations) == 16, "gate calibration count mismatch")
    for index, calibration in enumerate(calibrations):
        specs = [mutation_spec(rng) for _ in range(12)]
        children = [apply_mutation(base, spec) for spec in specs]
        scores = predict(model, [feature(base, child) for child in children])
        order = np.argsort(scores)[::-1]
        margin = float(scores[order[0]] - scores[order[1]])
        require(calibration["calibration_index"] == index, f"calibration index mismatch: {index}")
        require(calibration["mutations"] == specs, f"calibration schedule mismatch: {index}")
        require(calibration["candidate_configs"] == children, f"calibration children mismatch: {index}")
        require(np.allclose(calibration["predicted_scores"], scores, rtol=1e-10, atol=1e-10), f"calibration scores mismatch: {index}")
        close(calibration["margin"], margin, f"calibration margin {index}")
        margins.append(margin)
    gate = float(np.median(margins))
    close(bootstrap["raw_margin_gate"], gate, "raw margin gate")

    schedule_rng = np.random.default_rng(SEED + 1)
    expected_schedule = [[mutation_spec(schedule_rng) for _ in range(12)] for _ in range(10)]
    schedule = result["candidate_opportunity_schedule"]
    require(schedule == expected_schedule, "candidate opportunity schedule mismatch")
    probe_manifest = [row for row in manifest if row["split"] == "probe"]
    probe_shards = split_shard_ids(probe_manifest)

    def verify_trajectory(kind: str) -> dict[str, Any]:
        evidence = result[f"{kind}_trajectory"]
        require(evidence["kind"] == kind, f"{kind} trajectory label mismatch")
        require(len(evidence["generations"]) == 10, f"{kind} generation count mismatch")
        current = copy.deepcopy(base)
        for generation_index, generation in enumerate(evidence["generations"]):
            require(generation["generation"] == generation_index, f"{kind} generation index mismatch")
            require(generation["parent_config"] == current, f"{kind} parent mismatch: {generation_index}")
            require(generation["mutations"] == schedule[generation_index], f"{kind} arm schedule mismatch: {generation_index}")
            children = [apply_mutation(current, spec) for spec in schedule[generation_index]]
            require(generation["candidate_configs"] == children, f"{kind} children mismatch: {generation_index}")
            scores = predict(model, [feature(current, child) for child in children])
            require(np.allclose(generation["predicted_scores"], scores, rtol=1e-10, atol=1e-10), f"{kind} scores mismatch: {generation_index}")
            order = np.argsort(scores)[::-1]
            require(generation["raw_order"] == [int(value) for value in order], f"{kind} raw order mismatch: {generation_index}")
            margin = float(scores[order[0]] - scores[order[1]])
            close(generation["raw_margin"], margin, f"{kind} margin {generation_index}")
            expected_trigger = kind == "causal" and margin < gate
            require(generation["triggered"] is expected_trigger, f"{kind} trigger mismatch: {generation_index}")
            if not expected_trigger:
                require(generation["probe"] is None, f"unexpected {kind} probe: {generation_index}")
                selected = int(order[0])
            else:
                probe = generation["probe"]
                expected_shard = probe_shards[generation_index % len(probe_shards)]
                require(probe["shard_instance_ids"] == expected_shard, f"probe shard mismatch: {generation_index}")
                parent_aggregate = mean_scores(probe["parent_pack_keys"], packs)
                require_pack_group(
                    probe["parent_pack_keys"], packs, current, set(expected_shard),
                    f"probe parent {generation_index}",
                )
                close_mapping(probe["parent_aggregate"], parent_aggregate, f"probe parent {generation_index}")
                finalists = probe["finalists"]
                expected_indices = [int(value) for value in order[:4]]
                require([item["candidate_index"] for item in finalists] == expected_indices, f"probe finalist order mismatch: {generation_index}")
                gains: list[float] = []
                for finalist in finalists:
                    aggregate = mean_scores(finalist["pack_keys"], packs)
                    require_pack_group(
                        finalist["pack_keys"], packs,
                        children[finalist["candidate_index"]], set(expected_shard),
                        f"probe candidate {generation_index}/{finalist['candidate_index']}",
                    )
                    close_mapping(finalist["aggregate"], aggregate, f"probe candidate {generation_index}/{finalist['candidate_index']}")
                    gain = aggregate["utility"] - parent_aggregate["utility"]
                    close(finalist["probe_gain"], gain, f"probe gain {generation_index}/{finalist['candidate_index']}")
                    gains.append(gain)
                selected = expected_indices[int(np.argmax(gains))]
            require(generation["selected_index"] == selected, f"{kind} selection mismatch: {generation_index}")
            require(generation["selected_config"] == children[selected], f"{kind} selected config mismatch: {generation_index}")
            require(generation["selected_raw_rank"] == int(np.where(order == selected)[0][0]) + 1, f"{kind} selected rank mismatch: {generation_index}")
            current = children[selected]
        require(evidence["final_config"] == current, f"{kind} final config mismatch")
        require(evidence["final_config_sha256"] == digest_json(current), f"{kind} final digest mismatch")
        require(result["final_strategies"][kind]["config"] == current, f"{kind} frozen strategy mismatch")
        require(result["final_strategies"][kind]["sha256"] == digest_json(current), f"{kind} strategy digest mismatch")
        return current

    raw_config = verify_trajectory("raw")
    causal_config = verify_trajectory("causal")

    evaluation_manifest = {
        row["instance_id"]: row for row in manifest if row["split"] == "evaluation"
    }
    recomputed_aggregates: dict[str, dict[str, float]] = {}
    recomputed_repositories: dict[str, dict[str, float]] = {}
    for arm, config in (("base", base), ("raw", raw_config), ("causal", causal_config)):
        arm_evidence = result["evaluation"][arm]
        per_instance = arm_evidence["per_instance"]
        require(set(per_instance) == set(evaluation_manifest), f"{arm} evaluation task set mismatch")
        by_repo: dict[str, list[float]] = defaultdict(list)
        scores = []
        for instance_id, row in evaluation_manifest.items():
            record = per_instance[instance_id]
            pack = packs[record["pack_key"]]
            require(pack["identity"]["config_sha256"] == digest_json(config), f"{arm} evaluation config mismatch: {instance_id}")
            close_mapping(record["score"], pack["score"], f"{arm} instance score {instance_id}")
            scores.append(pack["score"])
            by_repo[row["repo"]].append(float(pack["score"]["utility"]))
        aggregate = {key: float(np.mean([score[key] for score in scores])) for key in SCORE_KEYS}
        repo_means = {repo: float(np.mean(values)) for repo, values in sorted(by_repo.items())}
        close_mapping(arm_evidence["aggregate"], aggregate, f"{arm} evaluation aggregate")
        require(set(arm_evidence["per_repository_utility"]) == set(repo_means), f"{arm} repo keys mismatch")
        for repo, value in repo_means.items():
            close(arm_evidence["per_repository_utility"][repo], value, f"{arm} repo {repo}")
        recomputed_aggregates[arm] = aggregate
        recomputed_repositories[arm] = repo_means
    raw_gain = recomputed_aggregates["raw"]["utility"] - recomputed_aggregates["base"]["utility"]
    causal_gain = recomputed_aggregates["causal"]["utility"] - recomputed_aggregates["base"]["utility"]
    delta = causal_gain - raw_gain
    wins = sum(
        recomputed_repositories["causal"][repo] > recomputed_repositories["raw"][repo]
        for repo in recomputed_repositories["causal"]
    )
    repository_count = len(recomputed_repositories["causal"])
    close(result["evaluation"]["raw_gain"], raw_gain, "raw gain")
    close(result["evaluation"]["causal_gain"], causal_gain, "causal gain")
    close(result["evaluation"]["causal_minus_raw"], delta, "causal-minus-raw")
    require(result["evaluation"]["causal_repository_wins"] == wins, "repository wins mismatch")
    require(result["evaluation"]["evaluation_repository_count"] == repository_count, "evaluation repository count mismatch")

    cost = result["cost"]
    require(cost["logical_pack_accesses"] == len(access_log), "logical pack count mismatch")
    require(cost["unique_pack_evaluations"] == len(first_seen), "unique pack count mismatch")
    require(cost["pack_executions_this_process"] == sum(event["source"] == "executed" for event in access_log), "executed pack count mismatch")
    require(cost["disk_pack_cache_reuses"] == sum(event["source"] == "disk" for event in access_log), "disk cache count mismatch")
    require(cost["memory_pack_cache_reuses"] == sum(event["source"] == "memory" for event in access_log), "memory cache count mismatch")
    require(cost["index_creations_this_process"] == sum(binding["execution_mode"] == "created" for binding in bindings.values()), "index creation count mismatch")
    require(cost["index_reuses_this_process"] == sum(binding["execution_mode"] == "reused" for binding in bindings.values()), "index reuse count mismatch")
    require(cost["index_bindings"] == len(bindings), "index binding cost mismatch")
    probe_unique = sum(
        event["phase"] == "probe" and event["incremental_pack_required"]
        for event in access_log
    )
    naive_probe = 10 * 5 * calculated_counts["probe"]
    probe_ratio = probe_unique / naive_probe
    require(cost["probe_pack_evaluations"] == probe_unique, "probe pack count mismatch")
    require(cost["naive_probe_pack_evaluations"] == naive_probe, "naive probe count mismatch")
    close(cost["probe_cost_ratio"], probe_ratio, "probe cost ratio")
    require(cost["bootstrap_measured_candidates"] == 24, "bootstrap cost mismatch")
    require(cost["gate_calibration_predicted_candidates"] == 192, "gate prediction cost mismatch")
    require(cost["raw_trajectory_predicted_candidates"] == 120, "raw candidate cost mismatch")
    require(cost["causal_trajectory_predicted_candidates"] == 120, "causal candidate cost mismatch")
    require(cost["invalid_candidates"] == 0, "invalid candidates were hidden")
    require(cost["recorded_failures"] == len(result["failures"]), "failure count mismatch")
    require(
        any(
            failure.get("schema") == "prune.e118-confirmation-interruption.v1"
            and failure.get("source_commit")
            == "ae6b560e254e1feba2c4d1ebfc9f1427c6fccc0a"
            for failure in result["failures"]
        ),
        "first confirmation interruption is not preserved",
    )
    close(cost["pack_wall_seconds"], sum(float(pack["pack_duration_seconds"]) for pack in packs.values()), "pack wall time")
    close(cost["index_wall_seconds"], sum(float(binding["index_duration_seconds"]) for binding in bindings.values()), "index wall time")
    decision = classify(delta, wins, repository_count, probe_ratio)
    require(result["decision"] == decision, "decision-rule classification mismatch")
    require(len(result["transitions"]) == 20, "transition count mismatch")
    for transition in result["transitions"]:
        evidence = result[f"{transition['arm']}_trajectory"]["generations"][transition["generation"]]
        selected = evidence["selected_index"]
        require(transition["x_t"] == evidence["parent_config"], "transition state mismatch")
        require(transition["candidate_delta_x"] == evidence["mutations"][selected], "transition mutation mismatch")
        close(transition["raw_predicted_gain"], evidence["predicted_scores"][selected], "transition prediction")
    return {
        "verification": "VERIFIED",
        "decision": decision,
        "selected_instances": len(manifest),
        "repositories": len(repository_membership),
        "raw_gain": raw_gain,
        "causal_gain": causal_gain,
        "causal_minus_raw": delta,
        "causal_repository_wins": wins,
        "evaluation_repository_count": repository_count,
        "probe_cost_ratio": probe_ratio,
        "unique_pack_evaluations": len(first_seen),
    }


def verify_path(
    result_path: Path, *, repo_root: Path, plan_path: Path, base_path: Path,
    check_receipt: bool = True,
) -> dict[str, Any]:
    raw = result_path.read_bytes()
    if check_receipt:
        receipt_path = result_path.with_suffix(result_path.suffix + ".sha256")
        require(receipt_path.is_file(), f"missing result receipt: {receipt_path}")
        receipt = json.loads(receipt_path.read_text())
        require(receipt["schema"] == "prune.e118-result-receipt.v1", "receipt schema mismatch")
        require(receipt["result_file"] == result_path.name, "receipt file mismatch")
        require(receipt["result_sha256"] == digest_bytes(raw), "result receipt digest mismatch")
    result = json.loads(raw)
    summary = verify_result(
        result, repo_root=repo_root.resolve(), plan_path=plan_path.resolve(),
        base_path=base_path.resolve(),
    )
    if check_receipt:
        require(receipt["source_commit"] == result["provenance"]["source_commit"], "receipt source mismatch")
        require(receipt["scope_sha256"] == result["scope_sha256"], "receipt scope mismatch")
    summary["result_sha256"] = digest_bytes(raw)
    return summary


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--result", type=Path,
        default=Path("experiments/E118-swebench-sparse-causal-prune.json"),
    )
    parser.add_argument("--repo-root", type=Path, default=Path(".."))
    parser.add_argument(
        "--plan", type=Path,
        default=Path("experiments/E118-swebench-sparse-causal-prune-plan.json"),
    )
    parser.add_argument(
        "--base", type=Path, default=Path("experiments/E118-base-strategy.json"),
    )
    parser.add_argument("--skip-receipt-check", action="store_true")
    args = parser.parse_args()
    try:
        summary = verify_path(
            args.result, repo_root=args.repo_root, plan_path=args.plan,
            base_path=args.base, check_receipt=not args.skip_receipt_check,
        )
    except (KeyError, TypeError, ValueError, VerificationError) as error:
        raise SystemExit(f"E118 VERIFICATION FAILED: {error}") from error
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
