from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import sys
import unittest
from dataclasses import asdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MIGRATION = ROOT / "experiments" / "e118a_local_sensor_validity"
FROZEN = MIGRATION / "frozen_parity_tree"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


analyzer = load_module(
    "e118a_migrated_analyzer", ROOT / "scripts" / "e118a_local_sensor_validity.py"
)
verifier = load_module(
    "e118a_migration_verifier",
    ROOT / "scripts" / "verify_e118a_local_sensor_validity.py",
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


def digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def load_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"expected object at {path}")
    return value


def synthetic_case(
    *, causal_sign: float = 1.0, parents: int = 12
) -> tuple[dict[str, object], dict[str, object]]:
    rows = []
    for parent in range(parents):
        repository = f"r{parent % 3}"
        raw_order = [1.0, 3.0, 0.0, 2.0]
        for candidate in range(4):
            actual = float(candidate) + parent * 1e-4
            mutation = {"gene": "g", "signed_step": float(candidate + 1)}
            rows.append({
                "repository": repository,
                "parent_id": f"p{parent}",
                "candidate_id": f"c{candidate}",
                "raw_score": raw_order[candidate],
                "causal_score": causal_sign * float(candidate),
                "actual_gain": actual,
                "parent_config_sha256": f"parent-{parent}",
                "candidate_config_sha256": f"child-{parent}-{candidate}",
                "mutation": mutation,
                "mutation_id": "sha256:" + digest(mutation),
                "seed": 11,
                "base_stage_a_metric": 1.0,
                "post_stage_a_metric": 1.0 + actual,
                "sensor_base_stage_a_metric": 1.0,
                "sensor_post_stage_a_metric": 1.0 + causal_sign * float(candidate),
                "sensor_instance_ids": [f"sensor-{parent}"],
                "target_instance_ids": [f"target-{parent}"],
                "code_revision": "a" * 40,
                "e118_source_commit": "b" * 40,
                "sensor_id": "sensor-v1",
                "metric_id": "stage-a-v1",
                "raw_model_id": "raw-v1",
            })
    identity = digest([{field: row[field] for field in IDENTITY_FIELDS} for row in rows])
    config: dict[str, object] = {
        "protocol_frozen": True,
        "source_e118_digest": "e118-digest",
        "transferred_sensor_id": "sensor-v1",
        "stage_a_metric_id": "stage-a-v1",
        "raw_model_id": "raw-v1",
        "candidate_plan_sha256": "plan-v1",
        "candidate_identity_sha256": "sha256:" + identity,
        "candidate_generation_seed": 11,
        "e118_provenance": {"execution_commit": "b" * 40},
        "evaluation_repositories": ["r0", "r1", "r2"],
        "min_siblings_per_parent": 4,
        "min_eligible_parents": 12,
        "min_evaluation_repositories": 3,
        "top_k": 4,
        "permutations": 199,
        "alpha": 0.05,
        "permutation_seed": 7,
    }
    payload: dict[str, object] = {
        "source_e118_digest": "e118-digest",
        "transferred_sensor_id": "sensor-v1",
        "stage_a_metric_id": "stage-a-v1",
        "raw_model_id": "raw-v1",
        "candidate_plan_sha256": "plan-v1",
        "candidate_identity_sha256": "sha256:" + identity,
        "candidate_generation_depended_on_scores": False,
        "held_out_evaluation_accesses": 0,
        "accessed_e118_splits": ["probe", "train"],
        "rows": rows,
    }
    return config, payload


def rebind(config: dict[str, object], payload: dict[str, object]) -> None:
    rows = payload["rows"]
    assert isinstance(rows, list)
    identity = digest([{field: row[field] for field in IDENTITY_FIELDS} for row in rows])
    config["candidate_identity_sha256"] = "sha256:" + identity
    payload["candidate_identity_sha256"] = "sha256:" + identity


class E118aMigratedUnitTests(unittest.TestCase):
    def test_perfect_incremental_sensor_passes(self) -> None:
        config, payload = synthetic_case()
        artifact = analyzer.analyze(payload, config, code_revision="test")
        self.assertEqual(artifact.status, "PASS")
        self.assertEqual(artifact.result["verdict"], "PASS_PROCEED_TO_E118B")
        self.assertAlmostEqual(artifact.result["pooled_within_parent_causal_rank_corr"], 1.0)

    def test_anti_sensor_fails(self) -> None:
        config, payload = synthetic_case(causal_sign=-1.0)
        artifact = analyzer.analyze(payload, config, code_revision="test")
        self.assertEqual(artifact.status, "FAIL")
        self.assertEqual(
            artifact.result["verdict"],
            "FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY",
        )

    def test_insufficient_parents_are_inconclusive(self) -> None:
        config, payload = synthetic_case(parents=6)
        artifact = analyzer.analyze(payload, config, code_revision="test")
        self.assertEqual(artifact.status, "INCONCLUSIVE")

    def test_protocol_must_be_frozen(self) -> None:
        config, payload = synthetic_case()
        config["protocol_frozen"] = False
        with self.assertRaisesRegex(ValueError, "protocol_frozen"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_payload_sensor_contract_mismatch_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["transferred_sensor_id"] = "different"
        with self.assertRaisesRegex(ValueError, "transferred_sensor_id"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_row_sensor_contract_mismatch_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["rows"][0]["sensor_id"] = "different"
        with self.assertRaisesRegex(ValueError, "sensor_id"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_row_metric_contract_mismatch_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["rows"][0]["metric_id"] = "different"
        with self.assertRaisesRegex(ValueError, "metric_id"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_unfrozen_repository_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["rows"][0]["repository"] = "surprise/repo"
        rebind(config, payload)
        with self.assertRaisesRegex(ValueError, "not frozen"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_duplicate_candidate_identity_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["rows"].append(copy.deepcopy(payload["rows"][0]))
        rebind(config, payload)
        with self.assertRaisesRegex(ValueError, "duplicate candidate"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_candidate_plan_identity_change_fails_closed(self) -> None:
        config, payload = synthetic_case()
        payload["rows"][0]["candidate_config_sha256"] = "substituted"
        with self.assertRaisesRegex(ValueError, "identity manifest"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_sensor_and_target_instances_must_be_disjoint(self) -> None:
        config, payload = synthetic_case()
        payload["rows"][0]["target_instance_ids"] = ["sensor-0"]
        rebind(config, payload)
        with self.assertRaisesRegex(ValueError, "must be disjoint"):
            analyzer.analyze(payload, config, code_revision="test")

    def test_duplicates_and_noops_do_not_inflate_sample(self) -> None:
        config, payload = synthetic_case()
        rows = payload["rows"]
        rows[1]["candidate_config_sha256"] = rows[0]["candidate_config_sha256"]
        rows[2]["candidate_config_sha256"] = rows[2]["parent_config_sha256"]
        config["min_siblings_per_parent"] = 2
        rebind(config, payload)
        artifact = analyzer.analyze(payload, config, code_revision="test")
        self.assertEqual(artifact.result["duplicate_config_rows_excluded"], 1)
        self.assertEqual(artifact.result["no_op_rows_excluded"], 1)
        self.assertEqual(artifact.result["analyzed_candidates"], 46)

    def test_independent_verifier_recomputes_synthetic_verdict(self) -> None:
        config, payload = synthetic_case()
        artifact = analyzer.analyze(payload, config, code_revision="test")
        plan = {
            "candidate_identity_sha256": str(
                config["candidate_identity_sha256"]
            ).removeprefix("sha256:")
        }
        result = verifier.verify(config, payload, asdict(artifact), plan)
        self.assertEqual(result["verification"], "VERIFIED")
        self.assertEqual(result["verdict"], "PASS_PROCEED_TO_E118B")
        self.assertFalse(result["production_analyzer_imported"])
        self.assertEqual(result["mean_topk_regret"]["k"], 4)


class E118aMigrationEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.config = load_json(
            FROZEN / "experiments/configs/e118a_local_sensor_validity.json"
        )
        cls.payload = load_json(
            FROZEN / "experiments/e118a_local_sensor_validity_mutations.json"
        )
        cls.artifact = load_json(
            FROZEN / "experiments/e118a_local_sensor_validity.json"
        )
        cls.plan = load_json(FROZEN / "experiments/configs/e118a_candidate_plan.json")
        cls.e118 = load_json(ROOT / "experiments/E118-swebench-sparse-causal-prune.json")

    def test_manifest_verifies_seven_byte_identical_files(self) -> None:
        manifest = load_json(MIGRATION / "migration_manifest.json")
        self.assertEqual(verifier.verify_migration_manifest(manifest, ROOT.parent), 7)

    def test_production_analyzer_reproduces_frozen_result_byte_for_byte(self) -> None:
        reproduced = analyzer.analyze(
            self.payload,
            self.config,
            code_revision=str(self.artifact["code_revision"]),
        )
        expected = json.dumps(self.artifact, sort_keys=True, separators=(",", ":"))
        self.assertEqual(reproduced.canonical_json(), expected)

    def test_independent_verifier_reproduces_real_verdict_and_topk(self) -> None:
        result = verifier.verify(
            self.config, self.payload, self.artifact, self.plan, self.e118
        )
        self.assertEqual(
            result["verdict"], "FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY"
        )
        self.assertEqual(result["repositories"], 6)
        self.assertEqual(result["eligible_parents"], 12)
        self.assertEqual(result["generated_candidates"], 144)
        self.assertEqual(result["mean_topk_regret"]["k"], 4)
        self.assertEqual(
            result["permutation_scope"],
            "strictly within each (repository, parent_id) group",
        )

    def test_repository_and_sensor_bindings_match_frozen_e118(self) -> None:
        bindings = verifier.verify_e118_bindings(self.config, self.payload, self.e118)
        self.assertEqual(
            bindings["target_repositories"],
            sorted(self.config["evaluation_repositories"]),
        )
        self.assertEqual(
            bindings["sensor_source_repositories"],
            sorted(self.config["sensor_source_repositories"]),
        )
        self.assertEqual(self.payload["held_out_evaluation_accesses"], 0)

    def test_verifier_does_not_import_production_analyzer(self) -> None:
        source = (ROOT / "scripts/verify_e118a_local_sensor_validity.py").read_text()
        self.assertNotIn("import e118a_local_sensor_validity", source)
        self.assertNotIn("from e118a_local_sensor_validity", source)


if __name__ == "__main__":
    unittest.main()
