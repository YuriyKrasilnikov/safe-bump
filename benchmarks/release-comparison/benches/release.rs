use std::hint::black_box;
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use safe_bump_current::{Arena as CurrentArena, SharedArena as CurrentSharedArena};
use safe_bump_release_comparison::parameters;
use safe_bump_v2::{Arena as PreviousArena, SharedArena as PreviousSharedArena};

const TOTAL_SHARED_ITEMS: usize = 65_536;

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark size fits u64")
}

fn allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("release/allocation");

    for size in parameters("release/allocation") {
        group.throughput(Throughput::Elements(usize_to_u64(size)));

        group.bench_with_input(BenchmarkId::new("v0.2.1", size), &size, |b, &size| {
            b.iter(|| {
                let mut arena = PreviousArena::with_capacity(size);
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                black_box(arena)
            });
        });

        group.bench_with_input(BenchmarkId::new("v0.3.1", size), &size, |b, &size| {
            b.iter(|| {
                let mut arena = CurrentArena::with_capacity(size);
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                black_box(arena)
            });
        });
    }

    group.finish();
}

fn validated_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("release/validated_lookup");

    for size in parameters("release/validated_lookup") {
        let mut previous = PreviousArena::with_capacity(size);
        let previous_indices: Vec<_> = (0..size)
            .map(|value| previous.alloc(usize_to_u64(value)))
            .collect();
        let mut current = CurrentArena::with_capacity(size);
        let current_indices: Vec<_> = (0..size)
            .map(|value| current.alloc(usize_to_u64(value)))
            .collect();
        group.throughput(Throughput::Elements(usize_to_u64(size)));

        group.bench_with_input(BenchmarkId::new("v0.2.1", size), &size, |b, _| {
            b.iter(|| {
                let checksum = black_box(previous_indices.as_slice())
                    .iter()
                    .map(|index| *previous.get(*index))
                    .fold(0_u64, u64::wrapping_add);
                black_box(checksum)
            });
        });

        group.bench_with_input(BenchmarkId::new("v0.3.1", size), &size, |b, _| {
            b.iter(|| {
                let checksum = black_box(current_indices.as_slice())
                    .iter()
                    .map(|index| *current.get(*index))
                    .fold(0_u64, u64::wrapping_add);
                black_box(checksum)
            });
        });
    }

    group.finish();
}

fn iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("release/iteration");

    for size in parameters("release/iteration") {
        let previous: PreviousArena<u64> = (0..usize_to_u64(size)).collect();
        let current: CurrentArena<u64> = (0..usize_to_u64(size)).collect();
        group.throughput(Throughput::Elements(usize_to_u64(size)));

        group.bench_with_input(BenchmarkId::new("v0.2.1", size), &size, |b, _| {
            b.iter(|| black_box(previous.iter().copied().fold(0_u64, u64::wrapping_add)));
        });

        group.bench_with_input(BenchmarkId::new("v0.3.1", size), &size, |b, _| {
            b.iter(|| black_box(current.iter().copied().fold(0_u64, u64::wrapping_add)));
        });
    }

    group.finish();
}

fn speculative_rollback(c: &mut Criterion) {
    let mut group = c.benchmark_group("release/speculative_rollback");

    for suffix_len in parameters("release/speculative_rollback") {
        let mut previous: PreviousArena<u64> = (0..1_024_u64).collect();
        let mut current: CurrentArena<u64> = (0..1_024_u64).collect();
        group.throughput(Throughput::Elements(usize_to_u64(suffix_len)));

        group.bench_with_input(
            BenchmarkId::new("v0.2.1", suffix_len),
            &suffix_len,
            |b, _| {
                b.iter(|| {
                    let checkpoint = previous.checkpoint();
                    for value in 0..suffix_len {
                        black_box(previous.alloc(usize_to_u64(value)));
                    }
                    previous.rollback(checkpoint);
                    black_box(previous.len())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("v0.3.1", suffix_len),
            &suffix_len,
            |b, _| {
                b.iter(|| {
                    let checkpoint = current.checkpoint();
                    for value in 0..suffix_len {
                        black_box(current.alloc(usize_to_u64(value)));
                    }
                    current.rollback(checkpoint);
                    black_box(current.len())
                });
            },
        );
    }

    group.finish();
}

fn concurrent_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("release/shared_concurrent_allocation");
    group.throughput(Throughput::Elements(usize_to_u64(TOTAL_SHARED_ITEMS)));

    for thread_count in parameters("release/shared_concurrent_allocation") {
        group.bench_with_input(
            BenchmarkId::new("v0.2.1", thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let arena = PreviousSharedArena::new();
                    thread::scope(|scope| {
                        for thread_id in 0..thread_count {
                            let arena = &arena;
                            scope.spawn(move || {
                                let per_thread = TOTAL_SHARED_ITEMS / thread_count;
                                let start = thread_id * per_thread;
                                let end = start + per_thread;
                                for value in start..end {
                                    black_box(arena.alloc(usize_to_u64(value)));
                                }
                            });
                        }
                    });
                    black_box(arena)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("v0.3.1", thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let arena = CurrentSharedArena::new();
                    thread::scope(|scope| {
                        for thread_id in 0..thread_count {
                            let arena = &arena;
                            scope.spawn(move || {
                                let per_thread = TOTAL_SHARED_ITEMS / thread_count;
                                let start = thread_id * per_thread;
                                let end = start + per_thread;
                                for value in start..end {
                                    black_box(arena.alloc(usize_to_u64(value)));
                                }
                            });
                        }
                    });
                    black_box(arena)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    allocation,
    validated_lookup,
    iteration,
    speculative_rollback,
    concurrent_allocation
);
criterion_main!(benches);
