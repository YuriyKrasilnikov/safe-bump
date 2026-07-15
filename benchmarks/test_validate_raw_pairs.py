from __future__ import annotations

import shutil
import subprocess
import sys
import unittest
import uuid
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parent.parent
VALIDATOR = REPOSITORY / "benchmarks" / "validate_raw_pairs.py"
SCRATCH_ROOT = REPOSITORY / "target" / "check-work"


class RawPairValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
        self.scratch = SCRATCH_ROOT / f"raw-pair-test-{uuid.uuid4().hex}"
        self.scratch.mkdir()
        self.manifest = self.scratch / "workloads.tsv"
        self.raw = self.scratch / "raw.tsv"
        self.manifest.write_text(
            "safe-bump-release-workloads-v1\nrelease/allocation\t64\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        shutil.rmtree(self.scratch)

    @staticmethod
    def header() -> list[str]:
        return [
            "safe-bump-raw-pairs-v1",
            "pair_id\tgroup\tparameter\trepetition\torder\tposition\tversion\telapsed_ns\twitness",
        ]

    @staticmethod
    def rows() -> list[str]:
        return [
            "release/allocation:64:0\trelease/allocation\t64\t0\tprevious-current\t0\tv0.2.1\t100\t0123456789abcdef",
            "release/allocation:64:0\trelease/allocation\t64\t0\tprevious-current\t1\tv0.3.0\t80\t0123456789abcdef",
            "release/allocation:64:1\trelease/allocation\t64\t1\tcurrent-previous\t0\tv0.3.0\t81\tfedcba9876543210",
            "release/allocation:64:1\trelease/allocation\t64\t1\tcurrent-previous\t1\tv0.2.1\t101\tfedcba9876543210",
        ]

    def validate(self, rows: list[str]) -> subprocess.CompletedProcess[str]:
        self.raw.write_text("\n".join(self.header() + rows) + "\n", encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(VALIDATOR),
                str(self.raw),
                "--manifest",
                str(self.manifest),
                "--manifest-header",
                "safe-bump-release-workloads-v1",
                "--schema",
                "safe-bump-raw-pairs-v1",
                "--previous",
                "v0.2.1",
                "--current",
                "v0.3.0",
                "--repetitions",
                "2",
            ],
            check=False,
            text=True,
            capture_output=True,
        )

    def test_complete_balanced_pairs_pass(self) -> None:
        result = self.validate(self.rows())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("raw_pair_pairs=2", result.stdout)

    def test_truncated_pair_is_rejected(self) -> None:
        result = self.validate(self.rows()[:-1])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("incomplete raw pair", result.stderr)

    def test_duplicate_observation_is_rejected(self) -> None:
        rows = self.rows()
        result = self.validate(rows + [rows[0]])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate raw-pair observation", result.stderr)

    def test_witness_mismatch_is_rejected(self) -> None:
        rows = self.rows()
        rows[1] = rows[1].replace("0123456789abcdef", "1111111111111111")
        result = self.validate(rows)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("content witness mismatch", result.stderr)


if __name__ == "__main__":
    unittest.main()
