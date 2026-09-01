#!/usr/bin/env python3
"""Deliberately corrupt frozen E118 evidence and require verifier rejection."""
from __future__ import annotations

import argparse
import copy
import importlib.util
import json
import tempfile
from pathlib import Path
from typing import Any, Callable


def load_verifier(path: Path):
    spec = importlib.util.spec_from_file_location("e118_independent_verifier", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load verifier: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def mutate_manifest_base(result: dict[str, Any]) -> None:
    result["manifest"]["selected_instances"][0]["base_commit"] = "0" * 40


def mutate_index_binding(result: dict[str, Any]) -> None:
    result["index_bindings"][0]["checkout_head"] = "f" * 40


def mutate_gold_task_binding(result: dict[str, Any]) -> None:
    row = result["manifest"]["selected_instances"][0]
    row["pack_task_sha256"] = row["gold_patch_sha256"]
    result["manifest"]["sha256"] = digest_json(result["manifest"]["selected_instances"])
    result["scope"]["manifest_sha256"] = result["manifest"]["sha256"]
    result["scope_sha256"] = digest_json(result["scope"])


def mutate_candidate_schedule(result: dict[str, Any]) -> None:
    mutation = result["candidate_opportunity_schedule"][0][0]
    mutation["signed_step"] = -float(mutation["signed_step"])


def mutate_pack_score(result: dict[str, Any]) -> None:
    result["pack_records"][0]["score"]["utility"] += 0.01


def mutate_aggregate(result: dict[str, Any]) -> None:
    result["evaluation"]["raw_gain"] += 0.01


def mutate_decision(result: dict[str, Any]) -> None:
    labels = [
        "SWE_BENCH_SPARSE_CAUSAL_PROMISING",
        "SWE_BENCH_SPARSE_CAUSAL_WEAK_SIGNAL",
        "SWE_BENCH_SPARSE_CAUSAL_INCONCLUSIVE",
        "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED",
    ]
    result["decision"] = next(label for label in labels if label != result["decision"])


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest_json(value: Any) -> str:
    import hashlib

    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--result", type=Path,
        default=Path("experiments/E118-swebench-sparse-causal-prune.json"),
    )
    parser.add_argument(
        "--verifier", type=Path,
        default=Path("scripts/verify_e118_swebench_sparse_causal_prune.py"),
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
    verifier = load_verifier(args.verifier.resolve())
    pristine = json.loads(args.result.read_text())
    mutations: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
        ("manifest_base_commit", mutate_manifest_base),
        ("index_checkout_head", mutate_index_binding),
        ("gold_task_binding", mutate_gold_task_binding),
        ("candidate_schedule", mutate_candidate_schedule),
        ("pack_utility", mutate_pack_score),
        ("aggregate_gain", mutate_aggregate),
        ("decision_label", mutate_decision),
    ]
    rejected: list[dict[str, str]] = []
    with tempfile.TemporaryDirectory(prefix="e118-corruption-") as directory:
        root = Path(directory)
        for name, mutation in mutations:
            corrupted = copy.deepcopy(pristine)
            mutation(corrupted)
            path = root / f"{name}.json"
            path.write_text(json.dumps(corrupted, indent=2, sort_keys=True) + "\n")
            try:
                verifier.verify_path(
                    path, repo_root=args.repo_root.resolve(), plan_path=args.plan.resolve(),
                    base_path=args.base.resolve(), check_receipt=False,
                )
            except (KeyError, TypeError, ValueError, verifier.VerificationError) as error:
                rejected.append({"mutation": name, "rejection": str(error)})
            else:
                raise SystemExit(f"CORRUPTION TEST FAILED: verifier accepted {name}")
    print(json.dumps({
        "corruption_tests": "PASSED", "mutations": len(mutations),
        "rejected": rejected,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
