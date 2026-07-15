#!/usr/bin/env python3
"""Stream Criterion output with closed-manifest progress markers."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import TextIO


def read_workloads(path: Path, expected_header: str) -> list[tuple[str, str]]:
    raw_lines = path.read_text(encoding="utf-8").split("\n")
    if raw_lines and raw_lines[-1] == "":
        raw_lines.pop()
    lines = [
        (number, line[:-1] if line.endswith("\r") else line)
        for number, line in enumerate(raw_lines, 1)
    ]
    if not lines or lines[0][1] != expected_header:
        raise SystemExit(f"invalid workload manifest header: {path}")

    workloads: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for number, line in lines[1:]:
        fields = line.split("\t")
        if len(fields) != 2 or not all(fields):
            raise SystemExit(f"malformed workload manifest row: {path}:{number}")
        workload = (fields[0], fields[1])
        if workload in seen:
            raise SystemExit(
                f"duplicate workload manifest row: {path}:{number}: "
                f"{fields[0]}/{fields[1]}"
            )
        seen.add(workload)
        workloads.append(workload)
    if not workloads:
        raise SystemExit(f"empty workload manifest: {path}")
    return workloads


def parse_phase(value: str) -> str:
    fields = value.split("/")
    if len(fields) != 2 or not all(field.isdigit() for field in fields):
        raise argparse.ArgumentTypeError("phase must have the form i/n")
    current, total = (int(field) for field in fields)
    if current < 1 or total < 1 or current > total:
        raise argparse.ArgumentTypeError("phase must satisfy 1 <= i <= n")
    return value


def benchmark_start(line: str) -> str | None:
    normalized = line.rstrip("\r\n")
    prefix = "Benchmarking "
    separator = ": Warming up"
    if not normalized.startswith(prefix) or separator not in normalized:
        return None
    return normalized[len(prefix) :].split(separator, 1)[0]


def stream_progress(
    source: TextIO,
    destination: TextIO,
    workloads: list[tuple[str, str]],
    previous: str,
    current: str,
    phase: str,
) -> None:
    if not previous or not current or previous == current:
        raise SystemExit("previous and current versions must be distinct and non-empty")
    versions = (previous, current)
    expected = {
        f"{group}/{version}/{parameter}": (group, parameter, version)
        for group, parameter in workloads
        for version in versions
    }
    started_versions: set[str] = set()
    completed_versions: set[str] = set()
    started_workloads: set[tuple[str, str]] = set()
    completed_workloads: set[tuple[str, str]] = set()

    for line in source:
        destination.write(line)
        destination.flush()
        started_label = benchmark_start(line)
        normalized = line.rstrip("\r\n")
        if started_label is not None:
            if started_label.startswith("release/") and started_label not in expected:
                raise SystemExit(
                    f"unexpected Criterion progress observation: {started_label}"
                )
            workload = expected.get(started_label)
            if workload is None:
                continue
            if started_label in started_versions:
                raise SystemExit(
                    f"duplicate Criterion progress observation: {started_label}"
                )
            started_versions.add(started_label)
            group, parameter, version = workload
            if version != previous:
                continue
            key = (group, parameter)
            if key in started_workloads:
                raise SystemExit(
                    f"duplicate Criterion workload progress marker: {group}/{parameter}"
                )
            started_workloads.add(key)
            destination.write(
                "benchmark_progress "
                f"phase={phase} workload={len(started_workloads)}/{len(workloads)} "
                f"repetition=- name={group}/{parameter} status=running\n"
            )
            destination.flush()
            continue

        workload = expected.get(normalized)
        if workload is None:
            continue
        if normalized not in started_versions:
            raise SystemExit(
                f"Criterion completion preceded its start: {normalized}"
            )
        if normalized in completed_versions:
            raise SystemExit(f"duplicate Criterion completion observation: {normalized}")
        completed_versions.add(normalized)
        group, parameter, _ = workload
        key = (group, parameter)
        labels = {f"{group}/{version}/{parameter}" for version in versions}
        if labels <= completed_versions:
            if key in completed_workloads:
                raise SystemExit(
                    f"duplicate Criterion workload completion: {group}/{parameter}"
                )
            completed_workloads.add(key)
            destination.write(
                "benchmark_progress "
                f"phase={phase} workload={len(completed_workloads)}/{len(workloads)} "
                f"repetition=- name={group}/{parameter} status=complete\n"
            )
            destination.flush()

    missing_starts = [label for label in expected if label not in started_versions]
    if missing_starts:
        raise SystemExit(
            "missing Criterion progress observations: " + ", ".join(missing_starts)
        )
    missing_completions = [
        label for label in expected if label not in completed_versions
    ]
    if missing_completions:
        raise SystemExit(
            "missing Criterion completion observations: "
            + ", ".join(missing_completions)
        )
    if len(started_workloads) != len(workloads):
        raise SystemExit(
            "Criterion workload progress cardinality mismatch: "
            f"started={len(started_workloads)} expected={len(workloads)}"
        )
    if len(completed_workloads) != len(workloads):
        raise SystemExit(
            "Criterion workload completion cardinality mismatch: "
            f"completed={len(completed_workloads)} expected={len(workloads)}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--manifest-header", required=True)
    parser.add_argument("--previous", required=True)
    parser.add_argument("--current", required=True)
    parser.add_argument("--phase", required=True, type=parse_phase)
    args = parser.parse_args()

    workloads = read_workloads(args.manifest, args.manifest_header)
    stream_progress(
        sys.stdin,
        sys.stdout,
        workloads,
        args.previous,
        args.current,
        args.phase,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
