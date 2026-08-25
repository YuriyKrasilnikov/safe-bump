#!/usr/bin/env python3
"""Fail-closed structural validation for alternating-order release observations."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from render_criterion_delta import read_workload_manifest, sort_key


COLUMNS = (
    "pair_id",
    "group",
    "parameter",
    "repetition",
    "order",
    "position",
    "version",
    "elapsed_ns",
    "witness",
)
WITNESS = re.compile(r"[0-9a-f]{16}")


def fail(message: str) -> None:
    raise SystemExit(message)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("raw_pairs", type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--manifest-header", required=True)
    parser.add_argument("--schema", required=True)
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--repetitions", required=True, type=int)
    args = parser.parse_args()

    if args.repetitions <= 0:
        fail("repetitions must be positive")
    if not args.baseline or not args.candidate or args.baseline == args.candidate:
        fail("baseline and candidate identities must be distinct and non-empty")
    versions_under_test = (args.baseline, args.candidate)
    workloads = read_workload_manifest(args.manifest, args.manifest_header)
    lines = args.raw_pairs.read_text(encoding="utf-8").splitlines()
    if len(lines) < 2 or lines[0] != args.schema:
        fail("invalid raw-pair schema")
    if tuple(lines[1].split("\t")) != COLUMNS:
        fail("invalid raw-pair column header")

    pairs: dict[tuple[str, str, int], dict[str, tuple[int, str, int]]] = {}
    for line_number, line in enumerate(lines[2:], 3):
        fields = line.split("\t")
        if len(fields) != len(COLUMNS):
            fail(f"malformed raw-pair row at line {line_number}")
        (
            pair_id,
            group,
            parameter,
            repetition_text,
            order,
            position_text,
            version,
            elapsed_text,
            witness,
        ) = fields
        workload = (group, parameter)
        if workload not in workloads:
            fail(f"unexpected raw-pair workload at line {line_number}: {group}/{parameter}")
        try:
            repetition = int(repetition_text)
            position = int(position_text)
            elapsed = int(elapsed_text)
        except ValueError:
            fail(f"non-integer raw-pair field at line {line_number}")
        if repetition not in range(args.repetitions):
            fail(f"raw-pair repetition out of range at line {line_number}")
        if position not in (0, 1) or elapsed <= 0:
            fail(f"invalid position or elapsed value at line {line_number}")
        if version not in versions_under_test:
            fail(f"unexpected raw-pair version at line {line_number}: {version}")
        if WITNESS.fullmatch(witness) is None:
            fail(f"malformed raw-pair witness at line {line_number}")
        expected_pair_id = f"{group}:{parameter}:{repetition}"
        if pair_id != expected_pair_id:
            fail(f"raw-pair identity mismatch at line {line_number}")
        expected_order = (
            "baseline-candidate" if repetition % 2 == 0 else "candidate-baseline"
        )
        if order != expected_order:
            fail(f"raw-pair order mismatch at line {line_number}")
        versions = pairs.setdefault((group, parameter, repetition), {})
        if version in versions:
            fail(f"duplicate raw-pair observation at line {line_number}: {pair_id}/{version}")
        versions[version] = (position, witness, elapsed)

    expected_pairs = {
        (group, parameter, repetition)
        for group, parameter in workloads
        for repetition in range(args.repetitions)
    }
    missing = sorted(expected_pairs - set(pairs), key=lambda item: (*sort_key(item[:2]), item[2]))
    if missing:
        fail(
            "missing raw pairs: "
            + ", ".join(
                f"{group}/{parameter}/{repetition}"
                for group, parameter, repetition in missing
            )
        )

    for key in sorted(expected_pairs, key=lambda item: (*sort_key(item[:2]), item[2])):
        versions = pairs[key]
        if set(versions) != set(versions_under_test):
            fail(f"incomplete raw pair: {key}: {sorted(versions)}")
        baseline_position, baseline_witness, _ = versions[args.baseline]
        candidate_position, candidate_witness, _ = versions[args.candidate]
        expected_baseline_position = 0 if key[2] % 2 == 0 else 1
        if (
            baseline_position != expected_baseline_position
            or candidate_position == baseline_position
        ):
            fail(f"raw-pair execution positions disagree with order: {key}")
        if baseline_witness != candidate_witness:
            fail(f"raw-pair content witness mismatch: {key}")

    expected_rows = len(expected_pairs) * 2
    if len(lines) - 2 != expected_rows:
        fail(
            f"raw-pair row cardinality mismatch: expected {expected_rows}, "
            f"actual {len(lines) - 2}"
        )

    print("raw_pair_validation_status=valid")
    print(f"raw_pair_workloads={len(workloads)}")
    print(f"raw_pair_pairs={len(expected_pairs)}")
    print(f"raw_pair_observations={expected_rows}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
