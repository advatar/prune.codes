#!/usr/bin/env python3
"""Zero-call, post-hoc diagnostics for a verified negative E118 result."""
from __future__ import annotations

import argparse
import itertools
import json
import math
from pathlib import Path
from typing import Any

import numpy as np

import verify_e118_swebench_sparse_causal_prune as verifier


def quantiles(values: np.ndarray) -> dict[str, float]:
    return {
        "p025": float(np.quantile(values, 0.025)),
        "p500": float(np.quantile(values, 0.5)),
        "p975": float(np.quantile(values, 0.975)),
    }


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
    args = parser.parse_args()
    verified = verifier.verify_path(
        args.result.resolve(), repo_root=args.repo_root.resolve(),
        plan_path=args.plan.resolve(), base_path=args.base.resolve(),
    )
    if verified["decision"] != "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED":
        raise SystemExit("this diagnostic is frozen for a NOT_SUPPORTED result")

    result: dict[str, Any] = json.loads(args.result.read_text())
    evaluation = result["evaluation"]
    repositories = sorted(evaluation["raw"]["per_repository_utility"])
    repository_deltas = {
        repo: (
            evaluation["causal"]["per_repository_utility"][repo]
            - evaluation["raw"]["per_repository_utility"][repo]
        )
        for repo in repositories
    }
    delta_values = np.asarray(list(repository_deltas.values()), dtype=float)
    bootstrap = np.asarray([
        float(np.mean(delta_values[list(indices)]))
        for indices in itertools.product(
            range(len(repositories)), repeat=len(repositories),
        )
    ])
    leave_one_repository_out = {
        repo: float(np.mean(np.delete(delta_values, index)))
        for index, repo in enumerate(repositories)
    }

    instance_deltas = {
        instance_id: (
            evaluation["causal"]["per_instance"][instance_id]["score"]["utility"]
            - evaluation["raw"]["per_instance"][instance_id]["score"]["utility"]
        )
        for instance_id in evaluation["raw"]["per_instance"]
    }
    worst_instance = min(instance_deltas, key=instance_deltas.get)
    remaining = [
        delta for instance_id, delta in instance_deltas.items()
        if instance_id != worst_instance
    ]

    probe_predictions: list[float] = []
    probe_gains: list[float] = []
    selected_raw_ranks: list[int] = []
    concordant = 0
    discordant = 0
    tied = 0
    triggered_generations: list[int] = []
    for generation in result["causal_trajectory"]["generations"]:
        probe = generation["probe"]
        if not probe:
            continue
        triggered_generations.append(int(generation["generation"]))
        selected_raw_ranks.append(int(generation["selected_raw_rank"]))
        finalists = probe["finalists"]
        for finalist in finalists:
            index = finalist["candidate_index"]
            probe_predictions.append(float(generation["predicted_scores"][index]))
            probe_gains.append(float(finalist["probe_gain"]))
        for left, right in itertools.combinations(finalists, 2):
            predicted_difference = (
                generation["predicted_scores"][left["candidate_index"]]
                - generation["predicted_scores"][right["candidate_index"]]
            )
            measured_difference = left["probe_gain"] - right["probe_gain"]
            product = predicted_difference * measured_difference
            if product > 0:
                concordant += 1
            elif product < 0:
                discordant += 1
            else:
                tied += 1

    bootstrap_candidate_gains = np.asarray([
        float(candidate["gain_vs_base"])
        for candidate in result["bootstrap"]["candidates"]
    ])
    raw_aggregate = evaluation["raw"]["aggregate"]
    causal_aggregate = evaluation["causal"]["aggregate"]
    metric_deltas = {
        metric: float(causal_aggregate[metric] - raw_aggregate[metric])
        for metric in raw_aggregate
    }
    raw_config = result["final_strategies"]["raw"]["config"]
    causal_config = result["final_strategies"]["causal"]["config"]
    strategy_differences = {
        gene: {"raw": raw_config[gene], "causal": causal_config[gene]}
        for gene in sorted(raw_config)
        if raw_config[gene] != causal_config[gene]
    }

    diagnostic = {
        "schema": "prune.e118-negative-diagnostics.v1",
        "experiment_id": result["experiment_id"],
        "post_hoc_scope": (
            "Frozen-evidence analysis only. No search, packing, indexing, threshold "
            "change, candidate selection, or new scientific result."
        ),
        "result_sha256": verified["result_sha256"],
        "source_commit": result["provenance"]["source_commit"],
        "decision": verified["decision"],
        "primary": {
            "raw_gain": verified["raw_gain"],
            "causal_gain": verified["causal_gain"],
            "causal_minus_raw": verified["causal_minus_raw"],
            "causal_repository_wins": verified["causal_repository_wins"],
            "evaluation_repository_count": verified["evaluation_repository_count"],
            "probe_cost_ratio": verified["probe_cost_ratio"],
        },
        "repository_cluster_sensitivity": {
            "repository_deltas_causal_minus_raw": repository_deltas,
            "unweighted_repository_mean_delta": float(np.mean(delta_values)),
            "exact_nonparametric_bootstrap_draws": int(len(bootstrap)),
            "bootstrap_mean_delta_quantiles": quantiles(bootstrap),
            "bootstrap_probability_mean_delta_gt_zero": float(np.mean(bootstrap > 0)),
            "leave_one_repository_out_mean_deltas": leave_one_repository_out,
            "all_leave_one_repository_out_deltas_negative": all(
                value < 0 for value in leave_one_repository_out.values()
            ),
            "one_sided_sign_probability_at_most_one_win_under_equal_odds": (
                sum(math.comb(len(repositories), k) for k in range(2))
                / (2 ** len(repositories))
            ),
            "two_sided_sign_probability": (
                2 * sum(math.comb(len(repositories), k) for k in range(2))
                / (2 ** len(repositories))
            ),
        },
        "instance_sensitivity": {
            "wins": sum(delta > 0 for delta in instance_deltas.values()),
            "losses": sum(delta < 0 for delta in instance_deltas.values()),
            "ties": sum(delta == 0 for delta in instance_deltas.values()),
            "largest_negative_instance": worst_instance,
            "largest_negative_delta": instance_deltas[worst_instance],
            "mean_delta_without_largest_negative_instance": float(np.mean(remaining)),
            "warning": (
                "This deletion is a sensitivity diagnostic, not a revised result or "
                "permitted exclusion."
            ),
        },
        "metric_deltas_causal_minus_raw": metric_deltas,
        "probe_diagnostics": {
            "triggered_generations": triggered_generations,
            "trigger_count": len(triggered_generations),
            "selected_candidate_raw_ranks": selected_raw_ranks,
            "probe_changed_raw_top_choice_count": sum(
                rank != 1 for rank in selected_raw_ranks
            ),
            "finalist_observations": len(probe_gains),
            "pooled_raw_prediction_probe_gain_pearson": float(
                np.corrcoef(probe_predictions, probe_gains)[0, 1]
            ),
            "pairwise_rank_concordant": concordant,
            "pairwise_rank_discordant": discordant,
            "pairwise_rank_tied": tied,
            "probe_gain_min": min(probe_gains),
            "probe_gain_max": max(probe_gains),
            "probe_gain_median_absolute": float(np.median(np.abs(probe_gains))),
        },
        "stage_a_dynamic_range": {
            "bootstrap_candidate_gain_min": float(np.min(bootstrap_candidate_gains)),
            "bootstrap_candidate_gain_max": float(np.max(bootstrap_candidate_gains)),
            "bootstrap_candidate_gain_range": float(np.ptp(bootstrap_candidate_gains)),
            "bootstrap_candidate_gain_standard_deviation": float(
                np.std(bootstrap_candidate_gains)
            ),
        },
        "final_strategy_differences": strategy_differences,
        "cost": result["cost"],
        "failure_mode_findings": {
            "raw_predictor_already_sufficient": (
                "Not supported: the raw-selected strategy also lost to the common "
                "base on held-out evaluation."
            ),
            "causal_probes_too_noisy": (
                "Plausible but not identified separately: finalist probe gains were "
                "usually small and ranks were unstable across small shards."
            ),
            "probe_family_or_task_unrepresentative": (
                "Plausible: probe-selected choices transferred poorly to evaluation; "
                "the frozen evidence cannot separate representation from noise."
            ),
            "causal_information_exists_but_gate_is_poor": (
                "Unresolved: the gate triggered in six generations and changed the raw "
                "top choice in four, but there is no independent gate comparison."
            ),
            "strategy_mutations_have_too_little_effect": (
                "Not supported as a sole explanation: measured mutations had nonzero "
                "train and evaluation effects, including a large path-recall change."
            ),
            "stage_a_utility_has_insufficient_dynamic_range": (
                "Mixed: bootstrap mutations span a measurable range, while most sparse "
                "probe gains are close to zero."
            ),
            "repository_heterogeneity_dominates": (
                "Supported for effect magnitude: one Docusaurus instance dominates the "
                "instance-weighted loss, although three of four repository means remain "
                "negative and every leave-one-repository-out mean is negative."
            ),
            "probe_cost_exceeds_value": (
                "Supported for E118: 44 incremental probe packs consumed 0.11 of the "
                "naive probe budget and produced a worse selected strategy."
            ),
        },
        "claim_boundary": result["claim_boundary"],
    }
    print(json.dumps(diagnostic, indent=2, sort_keys=True, allow_nan=False))


if __name__ == "__main__":
    main()
