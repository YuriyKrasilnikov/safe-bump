mod support;

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use safe_bump::Arena;

use support::usize_to_u64;

const BASE_LEN: usize = 1_024;

fn checkpoint(c: &mut Criterion) {
    let mut group = c.benchmark_group("rollback/checkpoint");

    for size in [0, 1_024, 65_536] {
        let arena: Arena<u64> = (0..usize_to_u64(size)).collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(arena.checkpoint()));
        });
    }

    group.finish();
}

fn discard_suffix(c: &mut Criterion) {
    let mut group = c.benchmark_group("rollback/discard_suffix");

    for suffix_len in [1, 8, 64, 1_024, 65_536] {
        group.throughput(Throughput::Elements(usize_to_u64(suffix_len)));
        group.bench_with_input(
            BenchmarkId::from_parameter(suffix_len),
            &suffix_len,
            |b, &suffix_len| {
                b.iter_batched(
                    || {
                        let mut arena = Arena::with_capacity(BASE_LEN + suffix_len);
                        let _ = arena.alloc_block(0..usize_to_u64(BASE_LEN));
                        let checkpoint = arena.checkpoint();
                        let _ = arena.alloc_block(0..usize_to_u64(suffix_len));
                        (arena, checkpoint)
                    },
                    |(mut arena, checkpoint)| {
                        arena.rollback(checkpoint);
                        black_box(arena)
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn reject_foreign_checkpoint(c: &mut Criterion) {
    let left: Arena<u64> = (0..usize_to_u64(BASE_LEN)).collect();
    let foreign = left.checkpoint();
    let mut right: Arena<u64> = (0..usize_to_u64(BASE_LEN)).collect();

    c.bench_function("rollback/reject_foreign_checkpoint", |b| {
        b.iter(|| black_box(right.try_rollback(foreign)));
    });
}

criterion_group!(
    benches,
    checkpoint,
    discard_suffix,
    reject_foreign_checkpoint
);
criterion_main!(benches);
