mod support;

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use safe_bump::Arena;

use support::usize_to_u64;

const SIZES: [usize; 4] = [1, 64, 1_024, 65_536];
const TRAVERSAL_SIZES: [usize; 3] = [64, 1_024, 65_536];

struct Droppable(u64);

impl Drop for Droppable {
    fn drop(&mut self) {
        black_box(self.0);
    }
}

fn allocate(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena/allocate");

    for size in SIZES {
        group.throughput(Throughput::Elements(usize_to_u64(size)));

        group.bench_with_input(BenchmarkId::new("safe_bump", size), &size, |b, &size| {
            b.iter(|| {
                let mut arena = Arena::with_capacity(size);
                for value in 0..size {
                    black_box(arena.alloc(black_box(usize_to_u64(value))));
                }
                black_box(arena)
            });
        });

        group.bench_with_input(BenchmarkId::new("vec", size), &size, |b, &size| {
            b.iter(|| {
                let mut values = Vec::with_capacity(size);
                for value in 0..size {
                    let slot = values.len();
                    values.push(black_box(usize_to_u64(value)));
                    black_box(slot);
                }
                black_box(values)
            });
        });
    }

    group.finish();
}

fn validated_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena/validated_lookup");

    for size in TRAVERSAL_SIZES {
        let mut arena = Arena::with_capacity(size);
        let indices: Vec<_> = (0..size)
            .map(|value| arena.alloc(usize_to_u64(value)))
            .collect();
        let values: Vec<_> = (0..usize_to_u64(size)).collect();
        let slots: Vec<_> = (0..size).collect();

        assert_eq!(indices.len(), slots.len());
        for (&idx, &slot) in indices.iter().zip(&slots) {
            assert_eq!(arena.get(idx), &values[slot]);
        }

        group.throughput(Throughput::Elements(usize_to_u64(size)));
        group.bench_with_input(BenchmarkId::new("safe_bump", size), &size, |b, _| {
            b.iter(|| {
                let mut checksum = 0_u64;
                for idx in &indices {
                    checksum = checksum.wrapping_add(*arena.get(black_box(*idx)));
                }
                black_box(checksum)
            });
        });
        group.bench_with_input(BenchmarkId::new("vec", size), &size, |b, _| {
            b.iter(|| {
                let mut checksum = 0_u64;
                for slot in &slots {
                    checksum = checksum.wrapping_add(values[black_box(*slot)]);
                }
                black_box(checksum)
            });
        });
    }

    group.finish();
}

fn sequential_iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena/sequential_iteration");

    for size in TRAVERSAL_SIZES {
        let arena: Arena<u64> = (0..usize_to_u64(size)).collect();
        let values: Vec<_> = (0..usize_to_u64(size)).collect();

        group.throughput(Throughput::Elements(usize_to_u64(size)));
        group.bench_with_input(BenchmarkId::new("safe_bump", size), &size, |b, _| {
            b.iter(|| black_box(arena.iter().copied().fold(0_u64, u64::wrapping_add)));
        });
        group.bench_with_input(BenchmarkId::new("vec", size), &size, |b, _| {
            b.iter(|| black_box(values.iter().copied().fold(0_u64, u64::wrapping_add)));
        });
    }

    group.finish();
}

fn reset(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena/reset");

    for size in [64, 1_024, 65_536] {
        group.throughput(Throughput::Elements(usize_to_u64(size)));
        group.bench_with_input(BenchmarkId::new("safe_bump", size), &size, |b, &size| {
            b.iter_batched(
                || {
                    (0..size)
                        .map(|value| Droppable(usize_to_u64(value)))
                        .collect::<Arena<_>>()
                },
                |mut arena| {
                    arena.reset();
                    black_box(arena)
                },
                BatchSize::LargeInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("vec", size), &size, |b, &size| {
            b.iter_batched(
                || {
                    (0..size)
                        .map(|value| Droppable(usize_to_u64(value)))
                        .collect::<Vec<_>>()
                },
                |mut values| {
                    values.clear();
                    black_box(values)
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    allocate,
    validated_lookup,
    sequential_iteration,
    reset
);
criterion_main!(benches);
