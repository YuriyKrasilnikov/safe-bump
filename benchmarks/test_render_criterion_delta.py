from __future__ import annotations

import json
import shutil
import subprocess
import sys
import unittest
import uuid
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parent.parent
RENDERER = REPOSITORY / "benchmarks" / "render_criterion_delta.py"
SCRATCH_ROOT = REPOSITORY / "target" / "check-work"


class RendererContractTests(unittest.TestCase):
    def setUp(self) -> None:
        SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
        self.scratch = SCRATCH_ROOT / f"renderer-test-{uuid.uuid4().hex}"
        self.scratch.mkdir()
        self.criterion = self.scratch / "criterion"
        self.manifest = self.scratch / "workloads.tsv"
        self.output = self.scratch / "report.md"

    def tearDown(self) -> None:
        shutil.rmtree(self.scratch)

    def write_manifest(self, *workloads: tuple[str, str]) -> None:
        rows = ["safe-bump-release-workloads-v1"]
        rows.extend(f"{group}\t{parameter}" for group, parameter in workloads)
        self.manifest.write_text("\n".join(rows) + "\n", encoding="utf-8")

    def write_sample(
        self,
        directory: str,
        group: str,
        parameter: str,
        function: str,
        point: float,
    ) -> None:
        new = self.criterion / directory / "new"
        new.mkdir(parents=True)
        (new / "benchmark.json").write_text(
            json.dumps(
                {
                    "group_id": group,
                    "value_str": parameter,
                    "function_id": function,
                }
            ),
            encoding="utf-8",
        )
        (new / "estimates.json").write_text(
            json.dumps(
                {
                    "median": {
                        "point_estimate": point,
                        "confidence_interval": {
                            "lower_bound": point * 0.9,
                            "upper_bound": point * 1.1,
                        },
                    }
                }
            ),
            encoding="utf-8",
        )

    def run_renderer(
        self, previous: str = "v0.2.1", current: str = "v0.3.0"
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(RENDERER),
                str(self.criterion),
                "--manifest",
                str(self.manifest),
                "--manifest-header",
                "safe-bump-release-workloads-v1",
                "--previous",
                previous,
                "--current",
                current,
                "--output",
                str(self.output),
            ],
            check=False,
            text=True,
            capture_output=True,
        )

    def test_complete_matrix_is_diagnostic_not_paired(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        self.write_sample("previous", "release/allocation", "64", "v0.2.1", 100.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        result = self.run_renderer()
        self.assertEqual(result.returncode, 0, result.stderr)
        report = self.output.read_text(encoding="utf-8")
        self.assertIn("marginal 95% confidence interval", report)
        self.assertIn("do not establish a paired speedup", report)
        self.assertNotIn("Ratio 95% CI", report)

    def test_manifest_exposes_workload_missing_both_versions(self) -> None:
        self.write_manifest(
            ("release/allocation", "64"),
            ("release/allocation", "1024"),
        )
        self.write_sample("previous", "release/allocation", "64", "v0.2.1", 100.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release/allocation/1024", result.stderr)
        self.assertIn("missing ['v0.2.1', 'v0.3.0']", result.stderr)

    def test_duplicate_observation_is_rejected(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        self.write_sample("previous-a", "release/allocation", "64", "v0.2.1", 100.0)
        self.write_sample("previous-b", "release/allocation", "64", "v0.2.1", 101.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate Criterion observation", result.stderr)

    def test_blank_manifest_row_is_rejected(self) -> None:
        self.manifest.write_text(
            "safe-bump-release-workloads-v1\n\nrelease/allocation\t64\n",
            encoding="utf-8",
        )
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("malformed workload manifest row", result.stderr)

    def test_comment_manifest_row_is_rejected(self) -> None:
        self.manifest.write_text(
            "safe-bump-release-workloads-v1\n# hidden row\n",
            encoding="utf-8",
        )
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("malformed workload manifest row", result.stderr)

    def test_unexpected_observation_is_rejected(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        self.write_sample("previous", "release/allocation", "64", "v0.2.1", 100.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        self.write_sample("extra", "release/iteration", "64", "v0.3.0", 70.0)
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected release workload", result.stderr)

    def test_same_version_comparison_is_rejected(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        result = self.run_renderer(previous="v0.3.0", current="v0.3.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("versions must be distinct", result.stderr)

    def test_unknown_version_is_rejected(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        self.write_sample("previous", "release/allocation", "64", "v0.2.1", 100.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        self.write_sample("unknown", "release/allocation", "64", "v0.4.0", 70.0)
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected release version", result.stderr)

    def test_non_positive_estimate_is_rejected(self) -> None:
        self.write_manifest(("release/allocation", "64"))
        self.write_sample("previous", "release/allocation", "64", "v0.2.1", 0.0)
        self.write_sample("current", "release/allocation", "64", "v0.3.0", 80.0)
        result = self.run_renderer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("non-positive or non-finite", result.stderr)


if __name__ == "__main__":
    unittest.main()
