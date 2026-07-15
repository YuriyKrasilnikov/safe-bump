#!/usr/bin/env python3
"""Render a strict, diagnostic Criterion release comparison.

Criterion estimates for two benchmark functions are marginal estimates, even
when both functions execute in one process.  This renderer therefore never
constructs or labels a ratio confidence interval from their endpoint
arithmetic.  Any paired analysis must consume the raw interleaved samples;
this renderer remains a public diagnostic surface only.
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator


@dataclass(frozen=True)
class Estimate:
    point: float
    lower: float
    upper: float


@dataclass(frozen=True)
class Sample:
    group: str
    parameter: str
    function: str
    estimate: Estimate
    source: Path


def read_workload_manifest(
    path: Path, expected_header: str
) -> set[tuple[str, str]]:
    raw_lines = path.read_text(encoding="utf-8").split("\n")
    if raw_lines and raw_lines[-1] == "":
        raw_lines.pop()
    lines = [
        (number, line[:-1] if line.endswith("\r") else line)
        for number, line in enumerate(raw_lines, 1)
    ]
    if not lines or lines[0][1] != expected_header:
        raise SystemExit(f"invalid workload manifest header: {path}")

    workloads: set[tuple[str, str]] = set()
    for number, line in lines[1:]:
        fields = line.split("\t")
        if len(fields) != 2 or not all(fields):
            raise SystemExit(f"malformed workload manifest row: {path}:{number}")
        key = (fields[0], fields[1])
        if key in workloads:
            raise SystemExit(
                f"duplicate workload manifest row: {path}:{number}: {fields[0]}/{fields[1]}"
            )
        workloads.add(key)
    if not workloads:
        raise SystemExit(f"empty workload manifest: {path}")
    return workloads


def read_samples(root: Path) -> Iterator[Sample]:
    for benchmark_path in sorted(root.rglob("new/benchmark.json")):
        estimates_path = benchmark_path.with_name("estimates.json")
        if not estimates_path.is_file():
            continue
        benchmark = json.loads(benchmark_path.read_text(encoding="utf-8"))
        estimates = json.loads(estimates_path.read_text(encoding="utf-8"))
        median = estimates["median"]
        interval = median["confidence_interval"]
        estimate = Estimate(
            point=float(median["point_estimate"]),
            lower=float(interval["lower_bound"]),
            upper=float(interval["upper_bound"]),
        )
        values = (estimate.point, estimate.lower, estimate.upper)
        if not all(math.isfinite(value) and value > 0 for value in values):
            raise SystemExit(f"non-positive or non-finite Criterion estimate: {estimates_path}")
        if estimate.lower > estimate.upper:
            raise SystemExit(f"reversed Criterion confidence interval: {estimates_path}")
        yield Sample(
            group=benchmark["group_id"],
            parameter=benchmark.get("value_str", ""),
            function=benchmark.get("function_id", ""),
            estimate=estimate,
            source=benchmark_path,
        )


def format_duration(nanoseconds: float) -> str:
    if nanoseconds < 1_000:
        return f"{nanoseconds:.2f} ns"
    if nanoseconds < 1_000_000:
        return f"{nanoseconds / 1_000:.2f} µs"
    if nanoseconds < 1_000_000_000:
        return f"{nanoseconds / 1_000_000:.2f} ms"
    return f"{nanoseconds / 1_000_000_000:.2f} s"


def format_estimate(estimate: Estimate) -> str:
    return (
        f"{format_duration(estimate.point)} "
        f"[{format_duration(estimate.lower)}, {format_duration(estimate.upper)}]"
    )


def sort_key(item: tuple[str, str]) -> tuple[str, int, str]:
    group, parameter = item
    try:
        numeric = int(parameter.replace(",", "").replace("_", ""))
    except ValueError:
        numeric = 0
    return group, numeric, parameter


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("criterion_root", type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--manifest-header", required=True)
    parser.add_argument("--previous", required=True)
    parser.add_argument("--current", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    if not args.previous or not args.current or args.previous == args.current:
        raise SystemExit("previous and current versions must be distinct and non-empty")
    expected = read_workload_manifest(args.manifest, args.manifest_header)
    observed: dict[tuple[str, str], dict[str, Sample]] = {}
    for sample in read_samples(args.criterion_root):
        key = (sample.group, sample.parameter)
        if key not in expected:
            raise SystemExit(
                "unexpected release workload: "
                f"{sample.group}/{sample.parameter}/{sample.function}: {sample.source}"
            )
        if sample.function not in {args.previous, args.current}:
            raise SystemExit(
                "unexpected release version: "
                f"{sample.group}/{sample.parameter}/{sample.function}: {sample.source}"
            )
        versions = observed.setdefault(key, {})
        if sample.function in versions:
            previous_source = versions[sample.function].source
            raise SystemExit(
                "duplicate Criterion observation: "
                f"{sample.group}/{sample.parameter}/{sample.function}: "
                f"{previous_source} and {sample.source}"
            )
        versions[sample.function] = sample

    missing = []
    for key in sorted(expected, key=sort_key):
        present = observed.get(key, {})
        absent = [
            version
            for version in (args.previous, args.current)
            if version not in present
        ]
        if absent:
            missing.append((key, absent))
    if missing:
        details = ", ".join(
            f"{group}/{parameter}: missing {absent}"
            for (group, parameter), absent in missing
        )
        raise SystemExit(f"incomplete release workload matrix: {details}")

    lines = [
        "# Release benchmark delta",
        "",
        (
            "Same-process Criterion marginal median estimates. Brackets are "
            "each function's own marginal 95% confidence interval; they are "
            "not a paired ratio interval."
        ),
        "",
        (
            "| Workload | Input | "
            f"{args.previous} median [marginal 95% CI] | "
            f"{args.current} median [marginal 95% CI] | Point ratio | "
            "Diagnostic interval relation |"
        ),
        "|---|---:|---:|---:|---:|---|",
    ]

    for key in sorted(expected, key=sort_key):
        group, parameter = key
        previous = observed[key][args.previous].estimate
        current = observed[key][args.current].estimate
        ratio = current.point / previous.point
        if current.upper < previous.lower:
            verdict = "marginal intervals disjoint; current lower"
        elif current.lower > previous.upper:
            verdict = "marginal intervals disjoint; current higher"
        else:
            verdict = "marginal intervals overlap"
        lines.append(
            f"| `{group}` | {parameter or '—'} | "
            f"{format_estimate(previous)} | "
            f"{format_estimate(current)} | {ratio:.3f}× | {verdict} |"
        )

    lines.extend(
        [
            "",
            (
                "This table reports timing evidence only. Semantic changes "
                "(for example stronger capability validation) remain part of "
                "the release contract even when they add measured cost."
            ),
            (
                "The point ratio and marginal-interval relation are diagnostic. "
                "They do not establish a paired speedup, regression, rank, or "
                "production selection decision."
            ),
            "",
        ]
    )
    rendered = "\n".join(lines)
    args.output.write_text(rendered, encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
