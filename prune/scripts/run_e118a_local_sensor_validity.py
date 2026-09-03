#!/usr/bin/env python3
"""Prune-native port of the historical E118a planning and collection tool.

The migrated evidence is complete and must not be recollected. This port is
retained to verify how it was produced and to preserve executable provenance.
It imports the exact digest-bound E118 runner from the same Prune repository.
``plan`` never evaluates a strategy. ``collect`` writes exclusively and still
refuses to run unless the complete plan is committed and bound by the frozen
config.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import importlib.util
import json
import math
import subprocess
import sys
from pathlib import Path
from typing import Any


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode("utf-8")


def digest_json(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def digest_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def write_json_exclusive(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    serialized = json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n"
    with path.open("x", encoding="utf-8") as handle:
        handle.write(serialized)


def git_output(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=root, check=True, text=True, capture_output=True
    ).stdout.strip()


def source_segment_hashes(source: str) -> dict[str, str]:
    wanted = {"normalize_path", "score_measurement", "feature", "predict", "split_shards"}
    tree = ast.parse(source)
    result: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name in wanted:
            segment = ast.get_source_segment(source, node)
            if segment is None:
                raise RuntimeError(f"cannot recover source for {node.name}")
            result[node.name] = hashlib.sha256(
                (segment.rstrip() + "\n").encode("utf-8")
            ).hexdigest()
    if set(result) != wanted:
        raise RuntimeError(f"missing frozen E118 functions: {sorted(wanted - set(result))}")
    return result


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    return value


def load_e118_module(path: Path) -> Any:
    spec = importlib.util.spec_from_file_location("frozen_e118_runner", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def verify_contracts(
    config: dict[str, Any], e118: dict[str, Any], runner_path: Path
) -> dict[str, str]:
    expected_digest = str(config["source_e118_digest"])
    if not expected_digest.startswith("sha256:"):
        raise ValueError("source_e118_digest must use sha256:<hex>")
    runner_digest = digest_file(runner_path)
    provenance = e118["provenance"]
    if runner_digest != provenance["source_files"]["runner"]["sha256"]:
        raise ValueError("frozen E118 runner digest mismatch")
    if provenance["source_commit"] != config["e118_provenance"]["execution_commit"]:
        raise ValueError("E118 execution commit mismatch")
    source = runner_path.read_text(encoding="utf-8")
    hashes = source_segment_hashes(source)
    contracts = config["contracts"]
    if hashes != contracts["source_function_sha256"]:
        raise ValueError("frozen E118 function source digest mismatch")

    metric = contracts["stage_a_metric"]
    if "sha256:" + digest_json(metric) != config["stage_a_metric_id"]:
        raise ValueError("Stage-A metric contract identifier mismatch")
    raw_model = contracts["raw_model"]
    if "sha256:" + digest_json(raw_model) != config["raw_model_id"]:
        raise ValueError("raw model contract identifier mismatch")
    sensor = contracts["transferred_sensor"]
    if "sha256:" + digest_json(sensor) != config["transferred_sensor_id"]:
        raise ValueError("transferred sensor contract identifier mismatch")
    if raw_model["ridge_sha256"] != digest_json(e118["bootstrap"]["ridge"]):
        raise ValueError("raw ridge model differs from frozen E118 artifact")
    if raw_model["genes_sha256"] != digest_json(e118["protocol"]["genes"]):
        raise ValueError("raw model gene order differs from frozen E118 artifact")
    if raw_model["bounds_sha256"] != digest_json(e118["protocol"]["bounds"]):
        raise ValueError("raw model bounds differ from frozen E118 artifact")
    return hashes


def verify_e118(
    config_path: Path, e118_path: Path, prune_root: Path
) -> tuple[dict[str, Any], dict[str, Any], Any, Path]:
    config = load_json(config_path)
    if config.get("protocol_frozen") is not True:
        raise ValueError("protocol_frozen must be true")
    expected = str(config["source_e118_digest"]).removeprefix("sha256:")
    if digest_file(e118_path) != expected:
        raise ValueError("E118 result artifact digest mismatch")
    e118 = load_json(e118_path)
    if e118.get("decision") != "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED":
        raise ValueError("unexpected E118 adjudication")
    runner_path = prune_root / "scripts" / "run_e118_swebench_sparse_causal_prune.py"
    verify_contracts(config, e118, runner_path)
    module = load_e118_module(runner_path)
    if module.SEED != e118["protocol"]["seed"]:
        raise ValueError("E118 runner seed mismatch")
    if tuple(module.GENES) != tuple(e118["protocol"]["genes"]):
        raise ValueError("E118 runner gene order mismatch")
    return config, e118, module, runner_path


def selected_rows(module: Any, e118: dict[str, Any]) -> list[dict[str, Any]]:
    rows, metadata = module.load_swebench()
    selected, repository_split = module.select_and_split(rows)
    manifest = module.public_manifest(selected)
    if metadata != e118["dataset"]:
        raise ValueError("pinned dataset metadata differs from E118 artifact")
    if manifest != e118["manifest"]["selected_instances"]:
        raise ValueError("selected instance manifest differs from E118 artifact")
    if repository_split != e118["manifest"]["repository_split"]:
        raise ValueError("repository split differs from E118 artifact")
    return selected


def sensor_shards(module: Any, rows: list[dict[str, Any]]) -> list[list[dict[str, Any]]]:
    probe = [row for row in rows if row["_split"] == "probe"]
    shards = module.split_shards(probe, count=8)
    if not shards or any(not shard for shard in shards):
        raise ValueError("unexpected empty E118 probe shard")
    return shards


def panel_assignments(config: dict[str, Any]) -> list[dict[str, Any]]:
    targets = list(config["panel_design"]["target_repository_order"])
    parents = list(config["panel_design"]["parent_strategy_order"])
    shard_ordinals = list(config["panel_design"]["sensor_shard_ordinal_order"])
    if not (len(targets) == len(parents) == len(shard_ordinals)):
        raise ValueError("panel design lists must have equal length")
    if len(targets) != int(config["min_eligible_parents"]):
        raise ValueError("panel count must equal frozen minimum eligible parents")
    return [
        {
            "panel_index": index,
            "repository": str(target),
            "parent_strategy": str(parent),
            "sensor_shard_ordinal": int(shard),
        }
        for index, (target, parent, shard) in enumerate(
            zip(targets, parents, shard_ordinals, strict=True)
        )
    ]


def make_plan(
    config: dict[str, Any], e118: dict[str, Any], module: Any, rows: list[dict[str, Any]]
) -> dict[str, Any]:
    rng = module.np.random.default_rng(int(config["candidate_generation_seed"]))
    siblings = int(config["siblings_generated_per_parent"])
    shards = sensor_shards(module, rows)
    by_repo: dict[str, list[dict[str, Any]]] = {}
    for repository in config["evaluation_repositories"]:
        by_repo[str(repository)] = [
            row for row in rows
            if row["_split"] == "train" and row["repo"] == repository
        ]
        if not by_repo[str(repository)]:
            raise ValueError(f"no frozen train rows for {repository}")

    panels = []
    identities = []
    for assignment in panel_assignments(config):
        index = assignment["panel_index"]
        repository = assignment["repository"]
        parent_kind = assignment["parent_strategy"]
        shard_ordinal = assignment["sensor_shard_ordinal"]
        if shard_ordinal < 0 or shard_ordinal >= len(shards):
            raise ValueError(f"sensor shard ordinal out of range: {shard_ordinal}")
        parent = e118["final_strategies"][parent_kind]["config"]
        parent_digest = module.config_digest(parent)
        if parent_digest != e118["final_strategies"][parent_kind]["sha256"]:
            raise ValueError(f"parent strategy digest mismatch: {parent_kind}")
        mutations = [module.mutation_spec(rng) for _ in range(siblings)]
        candidates = []
        parent_id = f"panel_{index:02d}_{repository.replace('/', '__')}_{parent_kind}"
        for candidate_index, mutation in enumerate(mutations):
            candidate = module.apply_mutation(parent, mutation)
            candidate_id = f"panel_{index:02d}_candidate_{candidate_index:02d}"
            candidate_digest = module.config_digest(candidate)
            mutation_id = "sha256:" + digest_json(mutation)
            record = {
                "candidate_id": candidate_id,
                "candidate_config": candidate,
                "candidate_config_sha256": candidate_digest,
                "mutation": mutation,
                "mutation_id": mutation_id,
            }
            candidates.append(record)
            identities.append({
                "repository": repository,
                "parent_id": parent_id,
                "candidate_id": candidate_id,
                "parent_config_sha256": parent_digest,
                "candidate_config_sha256": candidate_digest,
                "mutation": mutation,
                "mutation_id": mutation_id,
                "seed": int(config["candidate_generation_seed"]),
                "sensor_instance_ids": [row["instance_id"] for row in shards[shard_ordinal]],
                "target_instance_ids": [row["instance_id"] for row in by_repo[repository]],
            })
        panels.append({
            **assignment,
            "parent_id": parent_id,
            "parent_config": parent,
            "parent_config_sha256": parent_digest,
            "sensor_instance_ids": [row["instance_id"] for row in shards[shard_ordinal]],
            "target_instance_ids": [row["instance_id"] for row in by_repo[repository]],
            "candidates": candidates,
        })
    return {
        "schema": "parity.e118a-candidate-plan.v1",
        "source_e118_digest": config["source_e118_digest"],
        "transferred_sensor_id": config["transferred_sensor_id"],
        "stage_a_metric_id": config["stage_a_metric_id"],
        "raw_model_id": config["raw_model_id"],
        "candidate_generation_seed": int(config["candidate_generation_seed"]),
        "candidate_generation": "exact E118 mutation_spec/apply_mutation, score-blind",
        "panels": panels,
        "candidate_identity_sha256": digest_json(identities),
    }


def verify_frozen_plan(
    config: dict[str, Any], plan_path: Path, expected: dict[str, Any], repository_root: Path
) -> dict[str, Any]:
    plan = load_json(plan_path)
    if plan != expected:
        raise ValueError("candidate plan does not match deterministic frozen generation")
    plan_digest = digest_file(plan_path)
    if plan_digest != str(config["candidate_plan_sha256"]).removeprefix("sha256:"):
        raise ValueError("candidate plan file digest mismatch")
    if plan["candidate_identity_sha256"] != config["candidate_identity_sha256"].removeprefix("sha256:"):
        raise ValueError("candidate identity manifest digest mismatch")
    relative = plan_path.resolve().relative_to(repository_root.resolve())
    git_output(repository_root, "ls-files", "--error-unmatch", str(relative))
    if git_output(repository_root, "status", "--porcelain"):
        raise ValueError("Prune worktree must be clean before outcome collection")
    return plan


def required_index_roots(
    module: Any, rows: list[dict[str, Any]], ce_path: Path, source_cache: Path
) -> list[Path]:
    evaluator = module.Evaluator(ce_path, source_cache, 12000, "prepare", source_cache / "unused-ledger")
    roots = []
    for row in rows:
        if row["_split"] not in {"train", "probe"}:
            continue
        identity = evaluator._index_identity(row)
        cache_key = module.digest_json(identity)
        safe = module.re.sub(r"[^A-Za-z0-9_.-]+", "_", row["instance_id"])[:96]
        root = source_cache / "indexes" / f"{safe}-{cache_key[:16]}"
        if not (root / "READY.json").is_file():
            raise ValueError(f"source E118 index missing for {row['instance_id']}")
        roots.append(root)
    return roots


def prepare_cache(
    module: Any, rows: list[dict[str, Any]], ce_path: Path,
    source_cache: Path, work_cache: Path,
) -> None:
    destination = work_cache / "indexes"
    if destination.exists():
        raise ValueError(f"work index cache already exists: {destination}")
    destination.mkdir(parents=True)
    roots = required_index_roots(module, rows, ce_path, source_cache)
    for root in roots:
        target = destination / root.name
        subprocess.run(["cp", "-cR", str(root), str(target)], check=True)
    print(json.dumps({"prepared_indexes": len(roots), "work_cache": str(work_cache)}, sort_keys=True))


def collect(
    config: dict[str, Any], e118: dict[str, Any], module: Any,
    rows: list[dict[str, Any]], plan: dict[str, Any], ce_path: Path,
    work_cache: Path, repository_root: Path, plan_path: Path,
) -> dict[str, Any]:
    if digest_file(ce_path) != config["e118_provenance"]["ce_sha256"]:
        raise ValueError("ce binary digest mismatch")
    source_commit = config["e118_provenance"]["execution_commit"]
    relative_plan = plan_path.resolve().relative_to(repository_root.resolve())
    plan_commit = git_output(
        repository_root, "log", "-1", "--format=%H", "--", str(relative_plan)
    )
    current_commit = git_output(repository_root, "rev-parse", "HEAD")
    if not plan_commit:
        raise ValueError("candidate plan has no committed provenance")

    by_id = {row["instance_id"]: row for row in rows}
    evaluator = module.Evaluator(
        ce_path, work_cache, 12000,
        digest_json({"config": config, "plan": plan}),
        work_cache / "failure-ledger.jsonl",
    )
    ridge = e118["bootstrap"]["ridge"]
    model = tuple(module.np.asarray(ridge[key], dtype=float) for key in ("mean", "standard_deviation", "beta"))
    output_rows = []
    accessed_splits: set[str] = set()
    for panel in plan["panels"]:
        sensor_rows = [by_id[item] for item in panel["sensor_instance_ids"]]
        target_rows = [by_id[item] for item in panel["target_instance_ids"]]
        if {row["_split"] for row in sensor_rows} != {"probe"}:
            raise ValueError("causal sensor rows must be frozen E118 probe rows")
        if {row["_split"] for row in target_rows} != {"train"}:
            raise ValueError("actual-gain rows must be frozen E118 train rows")
        if {row["repo"] for row in target_rows} != {panel["repository"]}:
            raise ValueError("target repository mismatch")
        accessed_splits.update(row["_split"] for row in [*sensor_rows, *target_rows])
        parent = panel["parent_config"]
        parent_sensor, parent_sensor_keys = evaluator.aggregate(
            parent, sensor_rows, phase="e118a_sensor", arm="shared",
            candidate_id=panel["parent_id"] + "_parent",
        )
        parent_target, parent_target_keys = evaluator.aggregate(
            parent, target_rows, phase="e118a_actual", arm="shared",
            candidate_id=panel["parent_id"] + "_parent",
        )
        children = [candidate["candidate_config"] for candidate in panel["candidates"]]
        raw_scores = module.predict(model, [module.feature(parent, child) for child in children])
        for candidate, raw_score in zip(panel["candidates"], raw_scores, strict=True):
            child = candidate["candidate_config"]
            sensor_value, sensor_keys = evaluator.aggregate(
                child, sensor_rows, phase="e118a_sensor", arm="shared",
                candidate_id=candidate["candidate_id"],
            )
            target_value, target_keys = evaluator.aggregate(
                child, target_rows, phase="e118a_actual", arm="shared",
                candidate_id=candidate["candidate_id"],
            )
            causal_score = float(sensor_value["utility"] - parent_sensor["utility"])
            actual_gain = float(target_value["utility"] - parent_target["utility"])
            values = [float(raw_score), causal_score, actual_gain]
            if not all(math.isfinite(value) for value in values):
                raise ValueError("non-finite E118a outcome")
            output_rows.append({
                "repository": panel["repository"],
                "parent_id": panel["parent_id"],
                "candidate_id": candidate["candidate_id"],
                "raw_score": float(raw_score),
                "causal_score": causal_score,
                "actual_gain": actual_gain,
                "parent_config_sha256": panel["parent_config_sha256"],
                "candidate_config_sha256": candidate["candidate_config_sha256"],
                "mutation": candidate["mutation"],
                "mutation_id": candidate["mutation_id"],
                "seed": int(config["candidate_generation_seed"]),
                "base_stage_a_metric": float(parent_target["utility"]),
                "post_stage_a_metric": float(target_value["utility"]),
                "sensor_base_stage_a_metric": float(parent_sensor["utility"]),
                "sensor_post_stage_a_metric": float(sensor_value["utility"]),
                "sensor_instance_ids": panel["sensor_instance_ids"],
                "target_instance_ids": panel["target_instance_ids"],
                "parent_sensor_pack_keys": parent_sensor_keys,
                "candidate_sensor_pack_keys": sensor_keys,
                "parent_target_pack_keys": parent_target_keys,
                "candidate_target_pack_keys": target_keys,
                "code_revision": current_commit,
                "e118_source_commit": source_commit,
                "sensor_id": config["transferred_sensor_id"],
                "metric_id": config["stage_a_metric_id"],
                "raw_model_id": config["raw_model_id"],
            })
    if accessed_splits != {"train", "probe"}:
        raise ValueError(f"unexpected split access: {sorted(accessed_splits)}")
    identities = [
        {key: row[key] for key in (
            "repository", "parent_id", "candidate_id", "parent_config_sha256",
            "candidate_config_sha256", "mutation", "mutation_id", "seed",
            "sensor_instance_ids", "target_instance_ids",
        )}
        for row in output_rows
    ]
    if digest_json(identities) != plan["candidate_identity_sha256"]:
        raise ValueError("collected row identities differ from frozen plan")
    evaluator.finalize_index_bindings([
        row for row in rows if row["_split"] in {"train", "probe"}
    ])
    return {
        "schema": "parity.e118a-sibling-log.v1",
        "source_e118_digest": config["source_e118_digest"],
        "transferred_sensor_id": config["transferred_sensor_id"],
        "stage_a_metric_id": config["stage_a_metric_id"],
        "raw_model_id": config["raw_model_id"],
        "candidate_plan_sha256": config["candidate_plan_sha256"],
        "candidate_identity_sha256": config["candidate_identity_sha256"],
        "candidate_plan_commit": plan_commit,
        "collector_code_revision": current_commit,
        "accessed_e118_splits": sorted(accessed_splits),
        "held_out_evaluation_accesses": 0,
        "candidate_generation_depended_on_scores": False,
        "index_bindings": [
            evaluator.index_bindings[key] for key in sorted(evaluator.index_bindings)
        ],
        "pack_records": [evaluator.pack_records[key] for key in sorted(evaluator.pack_records)],
        "access_log": evaluator.access_log,
        "rows": output_rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("plan", "prepare-cache", "collect"))
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--e118-result", type=Path, required=True)
    parser.add_argument("--prune-root", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--ce", type=Path)
    parser.add_argument("--source-cache", type=Path)
    parser.add_argument("--work-cache", type=Path)
    args = parser.parse_args()

    repository_root = Path(git_output(Path.cwd(), "rev-parse", "--show-toplevel"))
    config, e118, module, _ = verify_e118(
        args.config.resolve(), args.e118_result.resolve(), args.prune_root.resolve()
    )
    rows = selected_rows(module, e118)
    expected_plan = make_plan(config, e118, module, rows)
    if args.command == "plan":
        if args.output is None:
            parser.error("plan requires --output")
        write_json_exclusive(args.output, expected_plan)
        print(json.dumps({
            "candidate_identity_sha256": expected_plan["candidate_identity_sha256"],
            "candidate_plan_sha256": digest_file(args.output),
            "panels": len(expected_plan["panels"]),
            "candidates": sum(len(panel["candidates"]) for panel in expected_plan["panels"]),
        }, sort_keys=True))
        return 0

    if args.plan is None or args.ce is None:
        parser.error(f"{args.command} requires --plan and --ce")
    plan = verify_frozen_plan(
        config, args.plan.resolve(), expected_plan, repository_root
    )
    if args.command == "prepare-cache":
        if args.source_cache is None or args.work_cache is None:
            parser.error("prepare-cache requires --source-cache and --work-cache")
        prepare_cache(
            module, rows, args.ce.resolve(),
            args.source_cache.resolve(), args.work_cache.resolve(),
        )
        return 0

    if args.output is None or args.work_cache is None:
        parser.error("collect requires --output and --work-cache")
    payload = collect(
        config, e118, module, rows, plan, args.ce.resolve(),
        args.work_cache.resolve(), repository_root, args.plan.resolve(),
    )
    write_json_exclusive(args.output, payload)
    print(json.dumps({
        "output": str(args.output),
        "rows": len(payload["rows"]),
        "repositories": len({row["repository"] for row in payload["rows"]}),
        "parents": len({(row["repository"], row["parent_id"]) for row in payload["rows"]}),
        "held_out_evaluation_accesses": payload["held_out_evaluation_accesses"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
