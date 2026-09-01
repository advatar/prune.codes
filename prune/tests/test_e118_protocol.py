from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np


ROOT = Path(__file__).resolve().parents[1]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load_module(
    "e118_runner", ROOT / "scripts" / "run_e118_swebench_sparse_causal_prune.py"
)
verifier = load_module(
    "e118_verifier", ROOT / "scripts" / "verify_e118_swebench_sparse_causal_prune.py"
)


class E118ProtocolTests(unittest.TestCase):
    def test_gold_patch_parser_returns_only_changed_paths(self) -> None:
        patch = """diff --git a/src/old.rs b/src/new.rs
index 123..456 100644
--- a/src/old.rs
+++ b/src/new.rs
@@ -1 +1 @@
-secret policy text
+different secret policy text
diff --git a/web/a.tsx b/web/a.tsx
"""
        self.assertEqual(runner.patch_paths(patch), ["src/new.rs", "web/a.tsx"])
        self.assertNotIn("secret policy text", json.dumps(runner.patch_paths(patch)))

    def test_repository_split_is_deterministic_and_disjoint(self) -> None:
        rows = []
        for language, suffix in (("Rust", "rs"), ("TypeScript", "ts")):
            for repo_index in range(5):
                repo = f"owner/{language.lower()}-{repo_index}"
                for instance_index in range(3):
                    rows.append({
                        "instance_id": f"{language}-{repo_index}-{instance_index}",
                        "repo": repo,
                        "base_commit": f"{repo_index:040x}",
                        "_language": language,
                        "_problem_statement": "task",
                        "_expect_paths": [f"src/file.{suffix}"],
                        "_problem_statement_sha256": "a" * 64,
                        "_gold_patch_sha256": "b" * 64,
                        "_expected_paths_sha256": "c" * 64,
                    })
        first, first_repos = runner.select_and_split(rows)
        second, second_repos = runner.select_and_split(rows)
        self.assertEqual(
            [(row["instance_id"], row["_split"]) for row in first],
            [(row["instance_id"], row["_split"]) for row in second],
        )
        self.assertEqual(first_repos, second_repos)
        membership = {}
        for split, repos in first_repos.items():
            for repo in repos:
                self.assertNotIn(repo, membership)
                membership[repo] = split
        self.assertGreaterEqual(len(first), 30)

    def test_cross_language_repository_fails_closed(self) -> None:
        rows = []
        for language in ("Rust", "TypeScript"):
            for repo_index in range(5):
                repo = "owner/shared" if repo_index == 0 else f"owner/{language}-{repo_index}"
                for instance_index in range(3):
                    rows.append({
                        "instance_id": f"{language}-{repo_index}-{instance_index}",
                        "repo": repo,
                        "_language": language,
                    })
        with self.assertRaises(runner.QualificationError):
            runner.select_and_split(rows)

    def test_candidate_schedule_is_reproducible(self) -> None:
        schedule = runner.shared_candidate_schedule()
        rng = np.random.default_rng(runner.SEED + 1)
        independently_generated = [
            [verifier.mutation_spec(rng) for _ in range(12)] for _ in range(10)
        ]
        self.assertEqual(schedule, independently_generated)

    def test_mutation_clamps_and_preserves_unmutated_genes(self) -> None:
        base = json.loads((ROOT / "experiments" / "E118-base-strategy.json").read_text())
        child = runner.apply_mutation(base, {"gene": "edge_radius", "signed_step": 10.0})
        self.assertEqual(child["edge_radius"], 4)
        self.assertEqual(
            {key: value for key, value in child.items() if key != "edge_radius"},
            {key: value for key, value in base.items() if key != "edge_radius"},
        )

    def test_frozen_decision_boundaries(self) -> None:
        self.assertEqual(
            runner.classify(0.01, 3, 4, 0.35),
            "SWE_BENCH_SPARSE_CAUSAL_PROMISING",
        )
        self.assertEqual(
            runner.classify(0.001, 2, 4, 0.35),
            "SWE_BENCH_SPARSE_CAUSAL_WEAK_SIGNAL",
        )
        self.assertEqual(
            runner.classify(-0.01, 4, 4, 0.0),
            "SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED",
        )
        self.assertEqual(
            runner.classify(0.009, 1, 4, 0.1),
            "SWE_BENCH_SPARSE_CAUSAL_INCONCLUSIVE",
        )

    def test_result_write_mode_is_exclusive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "result.json"
            with path.open("x") as handle:
                handle.write("{}")
            with self.assertRaises(FileExistsError):
                path.open("x")


if __name__ == "__main__":
    unittest.main()
