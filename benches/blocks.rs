mod support;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use safe_bump::Arena;

use support::usize_to_u64;

const SIZES: [usize; 6] = [1, 2, 8, 64, 1_024, 65_536];

fn allocate(c: &mut Criterion) {
    let mut group = c.benchmark_group("blocks/allocate");

    for size in SIZES {
        group.throughput(Throughput::Elements(usize_to_u64(size)));

        group.bench_with_input(BenchmarkId::new("individual", size), &size, |b, &size| {
            b.iter(|| {
                let mut arena = Arena::with_capacity(size);
                for value in 0..size {
                    black_box(arena.alloc(black_box(usize_to_u64(value))));
                }
                black_box(arena)
            });
        });

        group.bench_with_input(BenchmarkId::new("block", size), &size, |b, &size| {
            b.iter(|| {
                let mut arena = Arena::with_capacity(size);
                let block =
                    arena.alloc_block((0..size).map(|value| black_box(usize_to_u64(value))));
                black_box((arena, block))
            });
        });
    }

    group.finish();
}

fn derive_and_validate_indices(c: &mut Criterion) {
    let mut group = c.benchmark_group("blocks/derive_and_validate_indices");

    for size in [8, 64, 1_024, 65_536] {
        let mut arena = Arena::with_capacity(size);
        let block = arena.alloc_block(0..usize_to_u64(size));

        group.throughput(Throughput::Elements(usize_to_u64(size)));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let mut checksum = 0_u64;
                for idx in block.indices() {
                    checksum = checksum.wrapping_add(*arena.get(black_box(idx)));
                }
                black_box(checksum)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, allocate, derive_and_validate_indices);
criterion_main!(benches);
