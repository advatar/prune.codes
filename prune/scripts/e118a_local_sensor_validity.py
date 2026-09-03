from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from statistics import mean, median
from typing import Sequence


@dataclass(frozen=True, slots=True)
class ExperimentArtifact:
    """Original Parity artifact envelope, reproduced locally without dependency."""

    experiment: str
    code_revision: str
    seed: int
    config: dict[str, object]
    corpus_digest: str
    status: str
    result: dict[str, object]

    def canonical_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, separators=(",", ":"))

    def write(self, path: str | Path) -> None:
        target = Path(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(self.canonical_json() + "\n", encoding="utf-8")


REQUIRED_FIELDS = (
    "repository",
    "parent_id",
    "candidate_id",
    "raw_score",
    "causal_score",
    "actual_gain",
    "parent_config_sha256",
    "candidate_config_sha256",
    "mutation",
    "mutation_id",
    "seed",
    "base_stage_a_metric",
    "post_stage_a_metric",
    "sensor_base_stage_a_metric",
    "sensor_post_stage_a_metric",
    "sensor_instance_ids",
    "target_instance_ids",
    "code_revision",
    "e118_source_commit",
    "sensor_id",
    "metric_id",
    "raw_model_id",
)

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


def _digest_json(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _as_float(value: object, field: str) -> float:
    if not isinstance(value, (str, int, float)):
        raise ValueError(f"{field} must be numeric")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{field} must be finite")
    return result


def _average_ranks(values: Sequence[float]) -> list[float]:
    order = sorted(range(len(values)), key=values.__getitem__)
    ranks = [0.0] * len(values)
    i = 0
    while i < len(order):
        j = i + 1
        while j < len(order) and values[order[j]] == values[order[i]]:
            j += 1
        rank = (i + 1 + j) / 2.0
        for index in order[i:j]:
            ranks[index] = rank
        i = j
    return ranks


def _pearson(xs: Sequence[float], ys: Sequence[float]) -> float | None:
    if len(xs) != len(ys) or len(xs) < 2:
        return None
    mx = mean(xs)
    my = mean(ys)
    dx = [x - mx for x in xs]
    dy = [y - my for y in ys]
    denom = math.sqrt(sum(x * x for x in dx) * sum(y * y for y in dy))
    if denom == 0.0:
        return None
    return sum(x * y for x, y in zip(dx, dy, strict=True)) / denom


def _partial_corr(
    xs: Sequence[float], ys: Sequence[float], controls: Sequence[float]
) -> float | None:
    r_xy = _pearson(xs, ys)
    r_xc = _pearson(xs, controls)
    r_yc = _pearson(ys, controls)
    if r_xy is None:
        return None
    if r_xc is None or r_yc is None:
        return r_xy
    denom = math.sqrt(max(0.0, (1.0 - r_xc * r_xc) * (1.0 - r_yc * r_yc)))
    if denom <= 1e-15:
        return None
    return (r_xy - r_xc * r_yc) / denom


def _top_k_regret(scores: Sequence[float], gains: Sequence[float], k: int) -> float:
    order = sorted(range(len(scores)), key=lambda i: (-scores[i], i))
    selected = order[: min(k, len(order))]
    return max(gains) - max(gains[i] for i in selected)


def _random_top_k_regret(gains: Sequence[float], k: int) -> float:
    # Exact expectation over a uniformly random k-subset. The selected maximum
    # equals ordered[i] when i is the highest-gain selected item.
    n = len(gains)
    k = min(k, n)
    ordered = sorted(gains)
    denom = math.comb(n, k)
    expected_max = 0.0
    for i in range(k - 1, n):
        expected_max += ordered[i] * math.comb(i, k - 1) / denom
    return ordered[-1] - expected_max


def _validate_and_group(
    payload: dict[str, object], config: dict[str, object]
) -> dict[tuple[str, str], list[dict[str, object]]]:
    rows = payload.get("rows")
    if not isinstance(rows, list):
        raise ValueError("payload.rows must be a list")

    configured_repositories = config.get("evaluation_repositories")
    if not isinstance(configured_repositories, list):
        raise ValueError("evaluation_repositories must be a list")
    allowed_repositories = set(str(x) for x in configured_repositories)
    if not allowed_repositories:
        raise ValueError("evaluation_repositories must be non-empty")

    for field in (
        "source_e118_digest",
        "transferred_sensor_id",
        "stage_a_metric_id",
        "raw_model_id",
        "candidate_plan_sha256",
        "candidate_identity_sha256",
    ):
        if payload.get(field) != config.get(field):
            raise ValueError(f"input {field} does not match frozen config")
    if payload.get("candidate_generation_depended_on_scores") is not False:
        raise ValueError("candidate generation must be score-blind")
    if payload.get("held_out_evaluation_accesses") != 0:
        raise ValueError("held-out E118 evaluation access is forbidden")
    if payload.get("accessed_e118_splits") != ["probe", "train"]:
        raise ValueError("E118a input must access only frozen probe/train splits")

    grouped: dict[tuple[str, str], list[dict[str, object]]] = defaultdict(list)
    seen_candidates: set[tuple[str, str, str]] = set()
    identities: list[dict[str, object]] = []

    for raw_row in rows:
        if not isinstance(raw_row, dict):
            raise ValueError("every row must be an object")
        missing = [field for field in REQUIRED_FIELDS if field not in raw_row]
        if missing:
            raise ValueError(f"row missing required fields: {missing}")

        repository = str(raw_row["repository"])
        if repository not in allowed_repositories:
            raise ValueError(
                f"row repository {repository!r} was not frozen in evaluation_repositories"
            )
        parent_id = str(raw_row["parent_id"])
        candidate_id = str(raw_row["candidate_id"])
        identity = (repository, parent_id, candidate_id)
        if identity in seen_candidates:
            raise ValueError(f"duplicate candidate identity: {identity}")
        seen_candidates.add(identity)

        row = dict(raw_row)
        row["repository"] = repository
        row["parent_id"] = parent_id
        row["candidate_id"] = candidate_id
        for field in (
            "raw_score",
            "causal_score",
            "actual_gain",
            "base_stage_a_metric",
            "post_stage_a_metric",
            "sensor_base_stage_a_metric",
            "sensor_post_stage_a_metric",
        ):
            row[field] = _as_float(raw_row[field], field)
        if abs(
            _as_float(row["post_stage_a_metric"], "post_stage_a_metric")
            - _as_float(row["base_stage_a_metric"], "base_stage_a_metric")
            - _as_float(row["actual_gain"], "actual_gain")
        ) > 1e-12:
            raise ValueError("actual_gain does not equal post minus base Stage-A metric")
        if abs(
            _as_float(row["sensor_post_stage_a_metric"], "sensor_post_stage_a_metric")
            - _as_float(row["sensor_base_stage_a_metric"], "sensor_base_stage_a_metric")
            - _as_float(row["causal_score"], "causal_score")
        ) > 1e-12:
            raise ValueError("causal_score does not equal sensor post minus base metric")
        if row["sensor_id"] != config["transferred_sensor_id"]:
            raise ValueError("row sensor_id does not match frozen sensor")
        if row["metric_id"] != config["stage_a_metric_id"]:
            raise ValueError("row metric_id does not match frozen metric")
        if row["raw_model_id"] != config["raw_model_id"]:
            raise ValueError("row raw_model_id does not match frozen raw model")
        e118_provenance = config.get("e118_provenance")
        if not isinstance(e118_provenance, dict):
            raise ValueError("e118_provenance must be an object")
        if row["e118_source_commit"] != e118_provenance.get("execution_commit"):
            raise ValueError("row E118 source commit mismatch")
        if not isinstance(row["seed"], (str, int)):
            raise ValueError("row seed must be an integer")
        configured_seed = config.get("candidate_generation_seed")
        if not isinstance(configured_seed, (str, int)):
            raise ValueError("candidate_generation_seed must be an integer")
        if int(row["seed"]) != int(configured_seed):
            raise ValueError("row candidate-generation seed mismatch")
        if not isinstance(row["mutation"], dict):
            raise ValueError("row mutation must be an object")
        if row["mutation_id"] != "sha256:" + _digest_json(row["mutation"]):
            raise ValueError("row mutation_id mismatch")
        sensor_instances = row["sensor_instance_ids"]
        target_instances = row["target_instance_ids"]
        if not isinstance(sensor_instances, list) or not sensor_instances:
            raise ValueError("sensor_instance_ids must be a non-empty list")
        if not isinstance(target_instances, list) or not target_instances:
            raise ValueError("target_instance_ids must be a non-empty list")
        if set(str(item) for item in sensor_instances) & set(
            str(item) for item in target_instances
        ):
            raise ValueError("sensor and actual-gain instances must be disjoint")
        identities.append({field: row[field] for field in IDENTITY_FIELDS})
        grouped[(repository, parent_id)].append(row)

    expected_identity = str(config["candidate_identity_sha256"]).removeprefix(
        "sha256:"
    )
    if _digest_json(identities) != expected_identity:
        raise ValueError("row identity manifest differs from frozen candidate plan")
    return grouped


def _deduplicate_effective_siblings(
    grouped: dict[tuple[str, str], list[dict[str, object]]],
) -> tuple[
    dict[tuple[str, str], list[dict[str, object]]], int, int
]:
    effective: dict[tuple[str, str], list[dict[str, object]]] = {}
    duplicate_count = 0
    no_op_count = 0
    for key, rows in grouped.items():
        seen_configs: set[str] = set()
        retained = []
        for row in rows:
            candidate_digest = str(row["candidate_config_sha256"])
            if candidate_digest == str(row["parent_config_sha256"]):
                no_op_count += 1
                continue
            if candidate_digest in seen_configs:
                duplicate_count += 1
                continue
            seen_configs.add(candidate_digest)
            retained.append(row)
        effective[key] = retained
    return effective, duplicate_count, no_op_count


def _parent_metrics(
    key: tuple[str, str], rows: Sequence[dict[str, object]], top_k: int
) -> dict[str, object]:
    raw = [_as_float(row["raw_score"], "raw_score") for row in rows]
    causal = [_as_float(row["causal_score"], "causal_score") for row in rows]
    gains = [_as_float(row["actual_gain"], "actual_gain") for row in rows]

    raw_ranks = _average_ranks(raw)
    causal_ranks = _average_ranks(causal)
    gain_ranks = _average_ranks(gains)

    return {
        "repository": key[0],
        "parent_id": key[1],
        "siblings": len(rows),
        "raw_spearman": _pearson(raw_ranks, gain_ranks),
        "causal_spearman": _pearson(causal_ranks, gain_ranks),
        "causal_partial_rank_corr_given_raw": _partial_corr(
            causal_ranks, gain_ranks, raw_ranks
        ),
        "raw_top1_regret": _top_k_regret(raw, gains, 1),
        "causal_top1_regret": _top_k_regret(causal, gains, 1),
        "raw_topk_regret": _top_k_regret(raw, gains, top_k),
        "causal_topk_regret": _top_k_regret(causal, gains, top_k),
        "random_top1_regret": _random_top_k_regret(gains, 1),
        "random_topk_regret": _random_top_k_regret(gains, top_k),
        "best_actual_gain": max(gains),
        "mean_actual_gain": mean(gains),
    }


def _centered_rank_vectors(
    groups: Sequence[Sequence[dict[str, object]]],
    *,
    causal_permutations: Sequence[Sequence[float]] | None = None,
) -> tuple[list[float], list[float], list[float]]:
    causal_all: list[float] = []
    raw_all: list[float] = []
    gain_all: list[float] = []

    for group_index, rows in enumerate(groups):
        raw = [_as_float(row["raw_score"], "raw_score") for row in rows]
        gains = [_as_float(row["actual_gain"], "actual_gain") for row in rows]
        causal = (
            [_as_float(row["causal_score"], "causal_score") for row in rows]
            if causal_permutations is None
            else list(causal_permutations[group_index])
        )
        raw_ranks = _average_ranks(raw)
        causal_ranks = _average_ranks(causal)
        gain_ranks = _average_ranks(gains)

        for target, ranks in (
            (raw_all, raw_ranks),
            (causal_all, causal_ranks),
            (gain_all, gain_ranks),
        ):
            midpoint = mean(ranks)
            target.extend(rank - midpoint for rank in ranks)

    return causal_all, raw_all, gain_all


def _permutation_p_values(
    groups: Sequence[Sequence[dict[str, object]]],
    *,
    permutations: int,
    seed: int,
) -> tuple[float | None, float | None]:
    causal, raw, gains = _centered_rank_vectors(groups)
    observed_signal = _pearson(causal, gains)
    observed_incremental = _partial_corr(causal, gains, raw)
    if observed_signal is None or observed_incremental is None:
        return None, None

    rng = random.Random(seed)
    signal_extreme = 0
    incremental_extreme = 0
    causal_values = [
        [_as_float(row["causal_score"], "causal_score") for row in rows]
        for rows in groups
    ]

    for _ in range(permutations):
        shuffled = []
        for values in causal_values:
            permuted = list(values)
            rng.shuffle(permuted)
            shuffled.append(permuted)
        perm_causal, perm_raw, perm_gains = _centered_rank_vectors(
            groups, causal_permutations=shuffled
        )
        signal = _pearson(perm_causal, perm_gains)
        incremental = _partial_corr(perm_causal, perm_gains, perm_raw)
        if signal is not None and signal >= observed_signal - 1e-15:
            signal_extreme += 1
        if incremental is not None and incremental >= observed_incremental - 1e-15:
            incremental_extreme += 1

    return (
        (signal_extreme + 1) / (permutations + 1),
        (incremental_extreme + 1) / (permutations + 1),
    )


def analyze(
    payload: dict[str, object], config: dict[str, object], *, code_revision: str
) -> ExperimentArtifact:
    if config.get("protocol_frozen") is not True:
        raise ValueError("protocol_frozen must be true before E118a can be evaluated")
    for field in (
        "source_e118_digest",
        "transferred_sensor_id",
        "stage_a_metric_id",
        "raw_model_id",
        "candidate_plan_sha256",
        "candidate_identity_sha256",
    ):
        value = config.get(field)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"{field} must be a non-empty frozen identifier")
    if payload.get("source_e118_digest") != config["source_e118_digest"]:
        raise ValueError("input source_e118_digest does not match frozen config")
    if payload.get("transferred_sensor_id") != config["transferred_sensor_id"]:
        raise ValueError("input transferred_sensor_id does not match frozen config")
    if payload.get("stage_a_metric_id") != config["stage_a_metric_id"]:
        raise ValueError("input stage_a_metric_id does not match frozen config")

    grouped = _validate_and_group(payload, config)
    effective_grouped, duplicate_count, no_op_count = (
        _deduplicate_effective_siblings(grouped)
    )
    min_siblings = int(str(config.get("min_siblings_per_parent", 4)))
    min_parents = int(str(config.get("min_eligible_parents", 12)))
    min_repositories = int(str(config.get("min_evaluation_repositories", 3)))
    top_k = int(str(config.get("top_k", 4)))
    permutations = int(str(config.get("permutations", 10000)))
    alpha = float(str(config.get("alpha", 0.05)))
    seed = int(str(config.get("permutation_seed", 20260902118)))

    if min_siblings < 2:
        raise ValueError("min_siblings_per_parent must be >= 2")
    if min_parents < 1 or min_repositories < 1:
        raise ValueError("minimum sample gates must be positive")
    if top_k < 1 or permutations < 99:
        raise ValueError("top_k must be >=1 and permutations must be >=99")
    if not 0.0 < alpha < 1.0:
        raise ValueError("alpha must be between zero and one")

    eligible_items = [
        (key, rows)
        for key, rows in sorted(effective_grouped.items())
        if len(rows) >= min_siblings
    ]
    eligible_groups = [rows for _, rows in eligible_items]
    parent_metrics = [
        _parent_metrics(key, rows, top_k) for key, rows in eligible_items
    ]
    repositories = sorted({key[0] for key, _ in eligible_items})

    canonical_rows = json.dumps(
        payload.get("rows", []), sort_keys=True, separators=(",", ":")
    )
    corpus_digest = hashlib.sha256(canonical_rows.encode("utf-8")).hexdigest()

    if len(parent_metrics) < min_parents or len(repositories) < min_repositories:
        return ExperimentArtifact(
            experiment="e118a-local-sensor-validity",
            code_revision=code_revision,
            seed=seed,
            config=config,
            corpus_digest=corpus_digest,
            status="INCONCLUSIVE",
            result={
                "verdict": "INCONCLUSIVE_INSUFFICIENT_ELIGIBLE_PARENTS",
                "eligible_parents": len(parent_metrics),
                "eligible_repositories": repositories,
                "required_parents": min_parents,
                "required_repositories": min_repositories,
                "generated_candidates": sum(len(rows) for rows in grouped.values()),
                "analyzed_candidates": sum(
                    len(rows) for rows in effective_grouped.values()
                ),
                "duplicate_config_rows_excluded": duplicate_count,
                "no_op_rows_excluded": no_op_count,
                "parent_metrics": parent_metrics,
            },
        )

    causal, raw, gains = _centered_rank_vectors(eligible_groups)
    pooled_causal_rank_corr = _pearson(causal, gains)
    pooled_raw_rank_corr = _pearson(raw, gains)
    pooled_incremental = _partial_corr(causal, gains, raw)
    signal_p, incremental_p = _permutation_p_values(
        eligible_groups, permutations=permutations, seed=seed
    )

    causal_top1 = mean(_as_float(row["causal_top1_regret"], "causal_top1_regret") for row in parent_metrics)
    raw_top1 = mean(_as_float(row["raw_top1_regret"], "raw_top1_regret") for row in parent_metrics)
    random_top1 = mean(_as_float(row["random_top1_regret"], "random_top1_regret") for row in parent_metrics)
    causal_topk = mean(_as_float(row["causal_topk_regret"], "causal_topk_regret") for row in parent_metrics)
    raw_topk = mean(_as_float(row["raw_topk_regret"], "raw_topk_regret") for row in parent_metrics)
    random_topk = mean(_as_float(row["random_topk_regret"], "random_topk_regret") for row in parent_metrics)

    signal_valid = (
        pooled_causal_rank_corr is not None
        and pooled_causal_rank_corr > 0.0
        and signal_p is not None
        and signal_p <= alpha
        and causal_top1 < random_top1
    )
    incremental_valid = (
        pooled_incremental is not None
        and pooled_incremental > 0.0
        and incremental_p is not None
        and incremental_p <= alpha
    )
    proceed = signal_valid and incremental_valid

    causal_parent_rhos = [
        _as_float(row["causal_spearman"], "causal_spearman")
        for row in parent_metrics
        if row["causal_spearman"] is not None
    ]
    result: dict[str, object] = {
        "verdict": (
            "PASS_PROCEED_TO_E118B"
            if proceed
            else "FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY"
        ),
        "eligible_parents": len(parent_metrics),
        "eligible_repositories": repositories,
        "generated_candidates": sum(len(rows) for rows in grouped.values()),
        "analyzed_candidates": sum(len(rows) for rows in effective_grouped.values()),
        "duplicate_config_rows_excluded": duplicate_count,
        "no_op_rows_excluded": no_op_count,
        "pooled_within_parent_raw_rank_corr": pooled_raw_rank_corr,
        "pooled_within_parent_causal_rank_corr": pooled_causal_rank_corr,
        "pooled_causal_partial_rank_corr_given_raw": pooled_incremental,
        "causal_signal_permutation_p_one_sided": signal_p,
        "causal_incremental_permutation_p_one_sided": incremental_p,
        "mean_top1_regret": {
            "raw": raw_top1,
            "causal": causal_top1,
            "random": random_top1,
        },
        "mean_topk_regret": {
            "k": top_k,
            "raw": raw_topk,
            "causal": causal_topk,
            "random": random_topk,
        },
        "median_parent_causal_spearman": (
            None if not causal_parent_rhos else median(causal_parent_rhos)
        ),
        "signal_gate": signal_valid,
        "incremental_gate": incremental_valid,
        "parent_metrics": parent_metrics,
        "interpretation": (
            "PASS licenses E118b only: it shows that the frozen transferred causal "
            "measurement contains prospective within-parent ranking information on "
            "the target Prune substrate, including information conditional on the raw "
            "score. It does not establish closed-loop improvement, repository transfer, "
            "or a benefit from a target-calibrated sensor."
            if proceed
            else
            "The preregistered local-validity gate did not establish prospective "
            "incremental ranking information. Under this protocol E118b must not run; "
            "the next work should inspect mutation/search stability or redesign the "
            "measurement representation rather than close the loop."
        ),
    }

    return ExperimentArtifact(
        experiment="e118a-local-sensor-validity",
        code_revision=code_revision,
        seed=seed,
        config=config,
        corpus_digest=corpus_digest,
        status="PASS" if proceed else "FAIL",
        result=result,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    parser.add_argument("--input", required=True)
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--code-revision", required=True)
    args = parser.parse_args()

    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    payload = json.loads(Path(args.input).read_text(encoding="utf-8"))
    artifact = analyze(payload, config, code_revision=args.code_revision)
    artifact.write(args.artifact)
    print(artifact.canonical_json())
    return 0 if artifact.status in {"PASS", "INCONCLUSIVE"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
