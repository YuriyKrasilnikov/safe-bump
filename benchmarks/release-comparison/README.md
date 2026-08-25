# safe-bump release comparison

This standalone benchmark package runs the last published v0.2 release and
the current v0.3 source in the same Criterion process, with identical values
on the same host. It is separate from the library package because
cross-version measurement is release evidence, not a runtime dependency.

The common-operation comparison exposes both improvements and costs. In
particular, v0.3 validates arena identity and allocation history for every
handle; its lookup result must therefore be interpreted together with that
stronger safety contract. New v0.3-only block operations are measured in the
root benchmark suite instead of being given a false v0.2 equivalent.

Run it from the repository root with:

```console
cargo bench --locked --manifest-path benchmarks/release-comparison/Cargo.toml
```

`workloads.tsv` is the closed expected matrix for the public report. The
renderer rejects missing, extra, and duplicate observations, including a
workload for which both versions disappeared. Criterion's per-function
confidence intervals remain marginal estimates; the report does not construct
or claim a paired ratio confidence interval.

The `raw_pairs` binary executes the closed `paired-workloads.tsv` matrix in
alternating AB/BA order and emits exact pair IDs, execution positions, versions,
positive elapsed nanoseconds, and deterministic content witnesses. The paired
matrix separates allocation with reserved capacity, allocation with growth,
empty arena creation, and capacity reservation so their costs cannot mask one
another:

```console
cargo run --release --locked \
  --manifest-path benchmarks/release-comparison/Cargo.toml \
  --bin raw_pairs -- --repetitions 15 > target/raw-pairs.tsv
```

The raw matrix is intentionally distinct from the Criterion report matrix:
Criterion remains a broad public diagnostic, while raw pairs preserve the
execution relation needed by a controlled repeated analysis. The runner
aborts on a cross-version witness mismatch. Concurrent arena
publication order is scheduler-dependent, so that workload uses a
permutation-invariant content witness while still verifying exact length and
values. Witness construction is outside measured intervals; lookup and
iteration fixtures are validated first, then time the same wrapping checksum
used by Criterion. Draw performance conclusions only from controlled repeated
runs and a paired analysis appropriate to the question.
