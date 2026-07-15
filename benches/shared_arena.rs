mod support;

use std::hint::black_box;
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use safe_bump::SharedArena;

use support::usize_to_u64;

const TOTAL_ITEMS: usize = 65_536;
const BLOCK_LEN: usize = 64;

fn concurrent_single_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("shared_arena/concurrent_single_allocations");
    group.throughput(Throughput::Elements(usize_to_u64(TOTAL_ITEMS)));

    for thread_count in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let arena = SharedArena::new();
                    thread::scope(|scope| {
                        for thread_id in 0..thread_count {
                            let arena = &arena;
                            scope.spawn(move || {
                                let start = thread_id * (TOTAL_ITEMS / thread_count);
                                let end = start + (TOTAL_ITEMS / thread_count);
                                for value in start..end {
                                    black_box(arena.alloc(black_box(usize_to_u64(value))));
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

fn concurrent_blocks(c: &mut Criterion) {
    let mut group = c.benchmark_group("shared_arena/concurrent_blocks");
    group.throughput(Throughput::Elements(usize_to_u64(TOTAL_ITEMS)));

    for thread_count in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let arena = SharedArena::new();
                    thread::scope(|scope| {
                        for thread_id in 0..thread_count {
                            let arena = &arena;
                            scope.spawn(move || {
                                let thread_items = TOTAL_ITEMS / thread_count;
                                let thread_start = thread_id * thread_items;
                                for block_start in (0..thread_items).step_by(BLOCK_LEN) {
                                    let start = thread_start + block_start;
                                    let end = start + BLOCK_LEN;
                                    black_box(arena.alloc_block(
                                        (start..end).map(|value| black_box(usize_to_u64(value))),
                                    ));
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

fn published_reads(c: &mut Criterion) {
    let arena = SharedArena::new();
    let block = arena.alloc_block(0..usize_to_u64(TOTAL_ITEMS));

    c.bench_function("shared_arena/published_reads", |b| {
        b.iter(|| {
            let mut checksum = 0_u64;
            for idx in block.indices() {
                checksum = checksum.wrapping_add(*arena.get(black_box(idx)));
            }
            black_box(checksum)
        });
    });
}

criterion_group!(
    benches,
    concurrent_single_allocations,
    concurrent_blocks,
    published_reads
);
criterion_main!(benches);
