from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parent.parent
STREAMER = REPOSITORY / "benchmarks" / "stream_criterion_progress.py"
WORKFLOW = REPOSITORY / ".github" / "workflows" / "benchmarks.yml"


class CriterionProgressTests(unittest.TestCase):
    def run_streamer(
        self,
        manifest_rows: list[str],
        criterion_lines: list[str],
        previous: str = "v0.2.1",
        current: str = "v0.3.0",
    ) -> subprocess.CompletedProcess[str]:
        (REPOSITORY / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=REPOSITORY / "target") as scratch:
            manifest = Path(scratch) / "workloads.tsv"
            manifest.write_text(
                "safe-bump-release-workloads-v1\n"
                + "\n".join(manifest_rows)
                + "\n",
                encoding="utf-8",
            )
            return subprocess.run(
                [
                    sys.executable,
                    str(STREAMER),
                    "--manifest",
                    str(manifest),
                    "--manifest-header",
                    "safe-bump-release-workloads-v1",
                    "--previous",
                    previous,
                    "--current",
                    current,
                    "--phase",
                    "2/6",
                ],
                input="\n".join(criterion_lines) + "\n",
                check=False,
                text=True,
                capture_output=True,
            )

    def test_complete_stream_is_preserved_and_counted(self) -> None:
        result = self.run_streamer(
            ["release/allocation\t64", "release/validated_lookup\t1024"],
            [
                "unrelated output",
                "Benchmarking release/allocation/v0.2.1/64: Warming up for 3.0 s",
                "release/allocation/v0.2.1/64",
                "Benchmarking release/allocation/v0.3.0/64: Warming up for 3.0 s",
                "release/allocation/v0.3.0/64",
                "Benchmarking release/validated_lookup/v0.2.1/1024: Warming up for 3.0 s",
                "release/validated_lookup/v0.2.1/1024",
                "Benchmarking release/validated_lookup/v0.3.0/1024: Warming up for 3.0 s",
                "release/validated_lookup/v0.3.0/1024",
            ],
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("unrelated output", result.stdout)
        self.assertIn(
            "phase=2/6 workload=1/2 repetition=- name=release/allocation/64 "
            "status=complete",
            result.stdout,
        )
        self.assertIn(
            "phase=2/6 workload=2/2 repetition=- "
            "name=release/validated_lookup/1024 status=complete",
            result.stdout,
        )

    def test_missing_version_is_rejected(self) -> None:
        result = self.run_streamer(
            ["release/allocation\t64"],
            ["Benchmarking release/allocation/v0.2.1/64: Warming up"],
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing Criterion progress observations", result.stderr)

    def test_duplicate_observation_is_rejected(self) -> None:
        line = "Benchmarking release/allocation/v0.2.1/64: Warming up"
        result = self.run_streamer(
            ["release/allocation\t64"],
            [
                line,
                line,
                "Benchmarking release/allocation/v0.3.0/64: Warming up",
            ],
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate Criterion progress observation", result.stderr)

    def test_missing_completion_is_rejected(self) -> None:
        result = self.run_streamer(
            ["release/allocation\t64"],
            [
                "Benchmarking release/allocation/v0.2.1/64: Warming up",
                "release/allocation/v0.2.1/64",
                "Benchmarking release/allocation/v0.3.0/64: Warming up",
            ],
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing Criterion completion observations", result.stderr)

    def test_unexpected_release_workload_is_rejected(self) -> None:
        result = self.run_streamer(
            ["release/allocation\t64"],
            ["Benchmarking release/unknown/v0.2.1/64: Warming up"],
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected Criterion progress observation", result.stderr)

    def test_same_version_comparison_is_rejected(self) -> None:
        result = self.run_streamer(
            ["release/allocation\t64"],
            [],
            previous="v0.3.0",
            current="v0.3.0",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("versions must be distinct", result.stderr)

    def test_workflow_uses_absolute_closed_output_and_six_phases(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        run_job = workflow.split("  benchmark-run:\n", 1)[1]
        self.assertIn(
            'CARGO_TARGET_DIR="${PWD}/target/release-comparison"', run_job
        )
        self.assertEqual(
            run_job.count('CARGO_TARGET_DIR="${PWD}/target/release-comparison"'),
            2,
        )
        self.assertNotIn("CARGO_TARGET_DIR=target/release-comparison", run_job)
        self.assertEqual(
            run_job.count(
                'CRITERION_HOME="${PWD}/target/release-comparison/criterion"'
            ),
            1,
        )
        self.assertIn("python3 benchmarks/stream_criterion_progress.py", run_job)
        self.assertIn("if [[ -f benchmark-release-delta.md ]]", run_job)
        for phase in range(1, 7):
            self.assertIn(f"[{phase}/6]", run_job)


if __name__ == "__main__":
    unittest.main()
