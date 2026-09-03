#!/usr/bin/env python3
"""Independent, stdlib-only verifier for the migrated frozen E118a evidence.

This module intentionally does not import ``paritylab`` or the production
E118a analyzer.  Permutations are constructed from each parent-local causal
vector, so no value can cross a parent boundary.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
from collections import defaultdict
from pathlib import Path
from statistics import fmean
from typing import Any, Sequence


IDENTITY_FIELDS = (
    "repository",
    "parent_id",
    "candidate_id",
    "parent_config_sha256",
    "candidate_config_sha256",
    "mutation",
    "mutation_id",
    "seed",
    "sensor_instance_ids",
    "target_instance_ids",
)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode("utf-8")


def digest_json(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_object(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not a JSON object")
    return value


def ranks(values: Sequence[float]) -> list[float]:
    sorted_pairs = sorted(enumerate(values), key=lambda item: item[1])
    result = [0.0] * len(values)
    cursor = 0
    while cursor < len(sorted_pairs):
        end = cursor + 1
        while end < len(sorted_pairs) and sorted_pairs[end][1] == sorted_pairs[cursor][1]:
            end += 1
        tied_rank = ((cursor + 1) + end) / 2.0
        for original_index, _ in sorted_pairs[cursor:end]:
            result[original_index] = tied_rank
        cursor = end
    return result


def correlation(left: Sequence[float], right: Sequence[float]) -> float | None:
    if len(left) != len(right) or len(left) < 2:
        return None
    left_mean = fmean(left)
    right_mean = fmean(right)
    centered_left = [value - left_mean for value in left]
    centered_right = [value - right_mean for value in right]
    denominator = math.sqrt(
        sum(value * value for value in centered_left)
        * sum(value * value for value in centered_right)
    )
    if denominator == 0.0:
        return None
    return sum(
        x * y for x, y in zip(centered_left, centered_right, strict=True)
    ) / denominator


def partial_correlation(
    causal: Sequence[float], gain: Sequence[float], raw: Sequence[float]
) -> float | None:
    causal_gain = correlation(causal, gain)
    causal_raw = correlation(causal, raw)
    gain_raw = correlation(gain, raw)
    if causal_gain is None:
        return None
    if causal_raw is None or gain_raw is None:
        return causal_gain
    denominator = math.sqrt(
        max(0.0, (1.0 - causal_raw**2) * (1.0 - gain_raw**2))
    )
    if denominator <= 1e-15:
        return None
    return (causal_gain - causal_raw * gain_raw) / denominator


def centered_vectors(
    groups: Sequence[Sequence[dict[str, Any]]],
    causal_values: Sequence[Sequence[float]] | None = None,
) -> tuple[list[float], list[float], list[float]]:
    pooled_causal: list[float] = []
    pooled_raw: list[float] = []
    pooled_gain: list[float] = []
    for index, group in enumerate(groups):
        raw_rank = ranks([float(row["raw_score"]) for row in group])
        causal_rank = ranks(
            [float(row["causal_score"]) for row in group]
            if causal_values is None
            else causal_values[index]
        )
        gain_rank = ranks([float(row["actual_gain"]) for row in group])
        for destination, vector in (
            (pooled_causal, causal_rank),
            (pooled_raw, raw_rank),
            (pooled_gain, gain_rank),
        ):
            average = fmean(vector)
            destination.extend(value - average for value in vector)
    return pooled_causal, pooled_raw, pooled_gain


def top_one_regret(scores: Sequence[float], gains: Sequence[float]) -> float:
    selected = max(range(len(scores)), key=lambda index: (scores[index], -index))
    return max(gains) - gains[selected]


def random_top_one_regret(gains: Sequence[float]) -> float:
    return max(gains) - fmean(gains)


def top_k_regret(scores: Sequence[float], gains: Sequence[float], k: int) -> float:
    order = sorted(range(len(scores)), key=lambda index: (-scores[index], index))
    selected = order[: min(k, len(order))]
    return max(gains) - max(gains[index] for index in selected)


def random_top_k_regret(gains: Sequence[float], k: int) -> float:
    count = len(gains)
    k = min(k, count)
    ordered = sorted(gains)
    denominator = math.comb(count, k)
    expected_max = 0.0
    for index in range(k - 1, count):
        expected_max += ordered[index] * math.comb(index, k - 1) / denominator
    return ordered[-1] - expected_max


def verify_migration_manifest(manifest: dict[str, Any], repository_root: Path) -> int:
    if manifest.get("schema") != "prune.e118a-repository-migration.v1":
        raise ValueError("unrecognized migration manifest")
    checked = 0
    for entry in manifest.get("artifacts", []):
        if not isinstance(entry, dict) or entry.get("byte_identical") is not True:
            continue
        original = str(entry.get("original_sha256"))
        destination = repository_root / str(entry.get("destination_path"))
        actual = digest_file(destination)
        if actual != original or entry.get("destination_sha256") != original:
            raise ValueError(f"migrated evidence digest mismatch: {destination}")
        checked += 1
    if checked != 7:
        raise ValueError(f"expected seven byte-identical evidence files, found {checked}")
    assertions = manifest.get("migration_assertions")
    if not isinstance(assertions, dict):
        raise ValueError("migration assertions missing")
    expected = {
        "e118_outcome_regenerated": False,
        "e118a_outcome_regenerated": False,
        "e118b_authorized": False,
        "e118c_started": False,
        "repository_ownership_only": True,
        "scientific_evidence_changed": False,
    }
    if any(assertions.get(key) != value for key, value in expected.items()):
        raise ValueError("migration assertions do not preserve the scientific boundary")
    return checked


def verify_e118_bindings(
    config: dict[str, Any], payload: dict[str, Any], e118: dict[str, Any]
) -> dict[str, Any]:
    split = e118["manifest"]["repository_split"]
    if config["e118_repository_split"] != split:
        raise ValueError("E118 repository split changed")
    if config["evaluation_repositories"] != split["train"]:
        raise ValueError("E118a targets are not exactly E118 train repositories")
    if config["sensor_source_repositories"] != split["probe"]:
        raise ValueError("E118a sensor sources are not exactly E118 probe repositories")
    if config["excluded_repositories"] != split["evaluation"]:
        raise ValueError("E118 held-out quarantine changed")
    by_instance = {
        row["instance_id"]: row for row in e118["manifest"]["selected_instances"]
    }
    target_repositories: set[str] = set()
    sensor_repositories: set[str] = set()
    for row in payload["rows"]:
        repository = str(row["repository"])
        target_repositories.add(repository)
        for instance_id in row["target_instance_ids"]:
            source = by_instance.get(instance_id)
            if source is None or source["split"] != "train" or source["repo"] != repository:
                raise ValueError("target instance is not bound to its frozen E118 train repository")
        for instance_id in row["sensor_instance_ids"]:
            source = by_instance.get(instance_id)
            if source is None or source["split"] != "probe":
                raise ValueError("sensor instance is not a frozen E118 probe instance")
            sensor_repositories.add(str(source["repo"]))
    if target_repositories != set(split["train"]):
        raise ValueError("not all frozen E118 train repositories are represented")
    if sensor_repositories != set(split["probe"]):
        raise ValueError("not all frozen E118 probe repositories are represented")
    return {
        "target_repositories": sorted(target_repositories),
        "sensor_source_repositories": sorted(sensor_repositories),
        "excluded_repositories": sorted(split["evaluation"]),
    }


def close(actual: Any, expected: Any, path: str) -> None:
    if actual is None or expected is None:
        if actual != expected:
            raise ValueError(f"{path}: {actual!r} != {expected!r}")
        return
    if not math.isclose(float(actual), float(expected), rel_tol=1e-12, abs_tol=1e-12):
        raise ValueError(f"{path}: {actual!r} != {expected!r}")


def verify(
    config: dict[str, Any], payload: dict[str, Any],
    artifact: dict[str, Any], plan: dict[str, Any],
    e118: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if artifact["config"] != config:
        raise ValueError("artifact config differs from frozen config")
    if digest_json(payload["rows"]) != artifact["corpus_digest"]:
        raise ValueError("artifact corpus digest mismatch")
    if digest_json([
        {field: row[field] for field in IDENTITY_FIELDS} for row in payload["rows"]
    ]) != str(config["candidate_identity_sha256"]).removeprefix("sha256:"):
        raise ValueError("payload identities differ from frozen plan")
    if payload["candidate_identity_sha256"] != config["candidate_identity_sha256"]:
        raise ValueError("payload candidate identity binding mismatch")
    if plan["candidate_identity_sha256"] != config["candidate_identity_sha256"].removeprefix("sha256:"):
        raise ValueError("plan candidate identity binding mismatch")
    if payload["held_out_evaluation_accesses"] != 0:
        raise ValueError("held-out evaluation access recorded")
    if payload["accessed_e118_splits"] != ["probe", "train"]:
        raise ValueError("unexpected E118 split access")
    repository_bindings = None if e118 is None else verify_e118_bindings(config, payload, e118)

    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in payload["rows"]:
        if row["sensor_id"] != config["transferred_sensor_id"]:
            raise ValueError("row sensor ID mismatch")
        if row["metric_id"] != config["stage_a_metric_id"]:
            raise ValueError("row metric ID mismatch")
        if row["raw_model_id"] != config["raw_model_id"]:
            raise ValueError("row raw model ID mismatch")
        close(
            float(row["post_stage_a_metric"]) - float(row["base_stage_a_metric"]),
            row["actual_gain"], "actual gain",
        )
        close(
            float(row["sensor_post_stage_a_metric"])
            - float(row["sensor_base_stage_a_metric"]),
            row["causal_score"], "causal score",
        )
        grouped[(str(row["repository"]), str(row["parent_id"]))].append(row)

    duplicate_count = 0
    no_op_count = 0
    effective_groups = []
    repositories = set()
    minimum = int(config["min_siblings_per_parent"])
    for key in sorted(grouped):
        retained = []
        seen = set()
        for row in grouped[key]:
            candidate = str(row["candidate_config_sha256"])
            if candidate == str(row["parent_config_sha256"]):
                no_op_count += 1
                continue
            if candidate in seen:
                duplicate_count += 1
                continue
            seen.add(candidate)
            retained.append(row)
        if len(retained) >= minimum:
            repositories.add(key[0])
            effective_groups.append(retained)
    if len(effective_groups) < int(config["min_eligible_parents"]):
        raise ValueError("independent verifier found insufficient parents")
    if len(repositories) < int(config["min_evaluation_repositories"]):
        raise ValueError("independent verifier found insufficient repositories")

    causal, raw, gain = centered_vectors(effective_groups)
    causal_corr = correlation(causal, gain)
    raw_corr = correlation(raw, gain)
    incremental = partial_correlation(causal, gain, raw)
    if causal_corr is None or incremental is None:
        raise ValueError("decision-bearing correlation is undefined")

    generator = random.Random(int(config["permutation_seed"]))
    observed_causal = [
        [float(row["causal_score"]) for row in group] for group in effective_groups
    ]
    signal_extreme = 0
    incremental_extreme = 0
    permutations = int(config["permutations"])
    for _ in range(permutations):
        permuted_groups = []
        for parent_local_values in observed_causal:
            values = list(parent_local_values)
            generator.shuffle(values)
            permuted_groups.append(values)
        perm_causal, perm_raw, perm_gain = centered_vectors(
            effective_groups, permuted_groups
        )
        perm_signal = correlation(perm_causal, perm_gain)
        perm_incremental = partial_correlation(perm_causal, perm_gain, perm_raw)
        if perm_signal is not None and perm_signal >= causal_corr - 1e-15:
            signal_extreme += 1
        if perm_incremental is not None and perm_incremental >= incremental - 1e-15:
            incremental_extreme += 1
    signal_p = (signal_extreme + 1) / (permutations + 1)
    incremental_p = (incremental_extreme + 1) / (permutations + 1)

    causal_regrets = []
    raw_regrets = []
    random_regrets = []
    causal_topk_regrets = []
    raw_topk_regrets = []
    random_topk_regrets = []
    top_k = int(config["top_k"])
    for group in effective_groups:
        gains = [float(row["actual_gain"]) for row in group]
        causal_scores = [float(row["causal_score"]) for row in group]
        raw_scores = [float(row["raw_score"]) for row in group]
        causal_regrets.append(
            top_one_regret(causal_scores, gains)
        )
        raw_regrets.append(
            top_one_regret(raw_scores, gains)
        )
        random_regrets.append(random_top_one_regret(gains))
        causal_topk_regrets.append(top_k_regret(causal_scores, gains, top_k))
        raw_topk_regrets.append(top_k_regret(raw_scores, gains, top_k))
        random_topk_regrets.append(random_top_k_regret(gains, top_k))
    causal_top1 = fmean(causal_regrets)
    raw_top1 = fmean(raw_regrets)
    random_top1 = fmean(random_regrets)
    causal_topk = fmean(causal_topk_regrets)
    raw_topk = fmean(raw_topk_regrets)
    random_topk = fmean(random_topk_regrets)
    alpha = float(config["alpha"])
    signal_gate = causal_corr > 0.0 and signal_p <= alpha and causal_top1 < random_top1
    incremental_gate = incremental > 0.0 and incremental_p <= alpha
    verdict = (
        "PASS_PROCEED_TO_E118B"
        if signal_gate and incremental_gate
        else "FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY"
    )

    result = artifact["result"]
    if result["verdict"] != verdict:
        raise ValueError("verdict mismatch")
    if result["eligible_parents"] != len(effective_groups):
        raise ValueError("eligible parent count mismatch")
    if result["eligible_repositories"] != sorted(repositories):
        raise ValueError("eligible repository list mismatch")
    if result["duplicate_config_rows_excluded"] != duplicate_count:
        raise ValueError("duplicate exclusion count mismatch")
    if result["no_op_rows_excluded"] != no_op_count:
        raise ValueError("no-op exclusion count mismatch")
    close(result["pooled_within_parent_causal_rank_corr"], causal_corr, "causal corr")
    close(result["pooled_within_parent_raw_rank_corr"], raw_corr, "raw corr")
    close(result["pooled_causal_partial_rank_corr_given_raw"], incremental, "partial corr")
    close(result["causal_signal_permutation_p_one_sided"], signal_p, "signal p")
    close(result["causal_incremental_permutation_p_one_sided"], incremental_p, "incremental p")
    close(result["mean_top1_regret"]["causal"], causal_top1, "causal top1 regret")
    close(result["mean_top1_regret"]["raw"], raw_top1, "raw top1 regret")
    close(result["mean_top1_regret"]["random"], random_top1, "random top1 regret")
    if int(result["mean_topk_regret"]["k"]) != top_k:
        raise ValueError("top-k value mismatch")
    close(result["mean_topk_regret"]["causal"], causal_topk, "causal top-k regret")
    close(result["mean_topk_regret"]["raw"], raw_topk, "raw top-k regret")
    close(result["mean_topk_regret"]["random"], random_topk, "random top-k regret")
    return {
        "schema": "prune.e118a-migration-verification.v1",
        "verification_type": "MIGRATION_VERIFICATION",
        "verification": "VERIFIED",
        "verdict": verdict,
        "repositories": len(repositories),
        "eligible_parents": len(effective_groups),
        "generated_candidates": len(payload["rows"]),
        "analyzed_candidates": sum(len(group) for group in effective_groups),
        "duplicate_config_rows_excluded": duplicate_count,
        "no_op_rows_excluded": no_op_count,
        "pooled_within_parent_causal_rank_corr": causal_corr,
        "pooled_within_parent_raw_rank_corr": raw_corr,
        "pooled_causal_partial_rank_corr_given_raw": incremental,
        "causal_signal_permutation_p_one_sided": signal_p,
        "causal_incremental_permutation_p_one_sided": incremental_p,
        "mean_top1_regret": {
            "causal": causal_top1,
            "raw": raw_top1,
            "random": random_top1,
        },
        "mean_topk_regret": {
            "k": top_k,
            "causal": causal_topk,
            "raw": raw_topk,
            "random": random_topk,
        },
        "repository_bindings": repository_bindings,
        "permutation_scope": "strictly within each (repository, parent_id) group",
        "production_analyzer_imported": False,
        "scientific_outcome_regenerated": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--e118", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--repo-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    config = load_object(args.config)
    payload = load_object(args.input)
    artifact = load_object(args.artifact)
    plan = load_object(args.plan)
    e118 = load_object(args.e118)
    manifest = load_object(args.manifest)
    if digest_file(args.plan) != str(config["candidate_plan_sha256"]).removeprefix("sha256:"):
        raise ValueError("candidate plan file digest mismatch")
    expected_e118 = str(config["source_e118_digest"]).removeprefix("sha256:")
    if digest_file(args.e118) != expected_e118:
        raise ValueError("E118 source artifact digest mismatch")
    migrated_files = verify_migration_manifest(manifest, args.repo_root.resolve())
    result = verify(config, payload, artifact, plan, e118)
    result.update({
        "freeze_commit": artifact["code_revision"],
        "candidate_plan_file_sha256": digest_file(args.plan),
        "input_file_sha256": digest_file(args.input),
        "result_file_sha256": digest_file(args.artifact),
        "result_canonical_sha256": digest_json(artifact),
        "migration_manifest_sha256": digest_file(args.manifest),
        "byte_identical_evidence_files_verified": migrated_files,
    })
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
