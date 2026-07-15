use std::hint::black_box;
use std::thread;
use std::time::Instant;

use safe_bump_current::{Arena as CurrentArena, SharedArena as CurrentSharedArena};
use safe_bump_release_comparison::{parameters, workloads};
use safe_bump_v2::{Arena as PreviousArena, SharedArena as PreviousSharedArena};

const WARMUPS: usize = 2;
const DEFAULT_REPETITIONS: usize = 15;
const TOTAL_SHARED_ITEMS: usize = 65_536;

#[derive(Clone, Copy)]
struct Observation {
    elapsed_ns: u128,
    witness: u64,
}

struct Progress {
    expected: Vec<(&'static str, usize)>,
    completed_workloads: usize,
    repetitions: usize,
}

impl Progress {
    fn new(repetitions: usize) -> Self {
        Self {
            expected: workloads(),
            completed_workloads: 0,
            repetitions,
        }
    }

    fn begin_workload(&self, group: &str, parameter: usize) {
        let expected = self
            .expected
            .get(self.completed_workloads)
            .copied()
            .unwrap_or_else(|| panic!("unexpected raw workload: {group}/{parameter}"));
        assert_eq!(
            expected,
            (group, parameter),
            "raw workload order differs from the closed manifest"
        );
        eprintln!(
            "benchmark_progress phase=3/6 workload={}/{} repetition=0/{} name={group}/{parameter} status=warmup",
            self.completed_workloads + 1,
            self.expected.len(),
            self.repetitions,
        );
    }

    fn complete_repetition(&self, group: &str, parameter: usize, repetition: usize) {
        eprintln!(
            "benchmark_progress phase=3/6 workload={}/{} repetition={}/{} name={group}/{parameter} status=complete",
            self.completed_workloads + 1,
            self.expected.len(),
            repetition + 1,
            self.repetitions,
        );
    }

    fn complete_workload(&mut self) {
        self.completed_workloads += 1;
    }

    fn finish(self) {
        assert_eq!(
            self.completed_workloads,
            self.expected.len(),
            "raw workload matrix is incomplete"
        );
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark size fits u64")
}

fn repetitions() -> usize {
    let arguments: Vec<_> = std::env::args().collect();
    match arguments.as_slice() {
        [_] => DEFAULT_REPETITIONS,
        [_, flag, value] if flag == "--repetitions" => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or_else(|| panic!("repetitions must be a positive integer")),
        _ => panic!("usage: raw_pairs [--repetitions N]"),
    }
}

fn mix_witness(state: u64, value: u64) -> u64 {
    state
        .rotate_left(11)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(value ^ 0xD6E8_FEB8_6659_FD93)
}

fn ordered_witness(len: usize, values: impl IntoIterator<Item = u64>) -> u64 {
    let mut state = usize_to_u64(len);
    let mut count = 0_usize;
    for (ordinal, value) in values.into_iter().enumerate() {
        state = mix_witness(state, usize_to_u64(ordinal));
        state = mix_witness(state, value);
        count += 1;
    }
    assert_eq!(count, len, "iterator length disagrees with arena length");
    state
}

fn shared_content_witness(values: impl IntoIterator<Item = u64>) -> u64 {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_unstable();
    assert_eq!(
        values.len(),
        TOTAL_SHARED_ITEMS,
        "shared arena published an unexpected number of values"
    );
    for (expected, actual) in values.iter().copied().enumerate() {
        assert_eq!(
            usize_to_u64(expected),
            actual,
            "shared arena content is not the exact allocated value set"
        );
    }
    ordered_witness(values.len(), values)
}

fn timed<T>(operation: impl FnOnce() -> T, witness: impl FnOnce(&T) -> u64) -> Observation {
    let started = Instant::now();
    let value = black_box(operation());
    let elapsed_ns = started.elapsed().as_nanos();
    Observation {
        elapsed_ns,
        witness: witness(&value),
    }
}

fn emit(
    group: &str,
    parameter: usize,
    repetition: usize,
    order: &str,
    position: usize,
    version: &str,
    observation: Observation,
) {
    println!(
        "{group}:{parameter}:{repetition}\t{group}\t{parameter}\t{repetition}\t{order}\t{position}\t{version}\t{}\t{:016x}",
        observation.elapsed_ns, observation.witness
    );
}

fn run_pairs(
    progress: &mut Progress,
    group: &str,
    parameter: usize,
    repetitions: usize,
    mut previous: impl FnMut() -> Observation,
    mut current: impl FnMut() -> Observation,
) {
    progress.begin_workload(group, parameter);
    for _ in 0..WARMUPS {
        black_box(previous());
        black_box(current());
    }

    for repetition in 0..repetitions {
        let pair_id = format!("{group}:{parameter}:{repetition}");
        if repetition % 2 == 0 {
            let before = previous();
            let after = current();
            assert_eq!(
                before.witness, after.witness,
                "content witness mismatch for {pair_id}"
            );
            emit(
                group,
                parameter,
                repetition,
                "previous-current",
                0,
                "v0.2.1",
                before,
            );
            emit(
                group,
                parameter,
                repetition,
                "previous-current",
                1,
                "v0.3.0",
                after,
            );
        } else {
            let before = current();
            let after = previous();
            assert_eq!(
                before.witness, after.witness,
                "content witness mismatch for {pair_id}"
            );
            emit(
                group,
                parameter,
                repetition,
                "current-previous",
                0,
                "v0.3.0",
                before,
            );
            emit(
                group,
                parameter,
                repetition,
                "current-previous",
                1,
                "v0.2.1",
                after,
            );
        }
        progress.complete_repetition(group, parameter, repetition);
    }
    progress.complete_workload();
}

fn allocation(progress: &mut Progress, repetitions: usize) {
    let group = "release/allocation";
    for size in parameters(group) {
        run_pairs(
            progress,
            group,
            size,
            repetitions,
            || {
                timed(
                    || {
                        let mut arena = PreviousArena::with_capacity(size);
                        for value in 0..size {
                            black_box(arena.alloc(usize_to_u64(value)));
                        }
                        arena
                    },
                    |arena| ordered_witness(arena.len(), arena.iter().copied()),
                )
            },
            || {
                timed(
                    || {
                        let mut arena = CurrentArena::with_capacity(size);
                        for value in 0..size {
                            black_box(arena.alloc(usize_to_u64(value)));
                        }
                        arena
                    },
                    |arena| ordered_witness(arena.len(), arena.iter().copied()),
                )
            },
        );
    }
}

fn validated_lookup(progress: &mut Progress, repetitions: usize) {
    let group = "release/validated_lookup";
    for size in parameters(group) {
        let mut previous = PreviousArena::with_capacity(size);
        let previous_indices: Vec<_> = (0..size)
            .map(|value| previous.alloc(usize_to_u64(value)))
            .collect();
        let mut current = CurrentArena::with_capacity(size);
        let current_indices: Vec<_> = (0..size)
            .map(|value| current.alloc(usize_to_u64(value)))
            .collect();
        let previous_witness = ordered_witness(
            previous_indices.len(),
            previous_indices
                .iter()
                .map(|index| *previous.get(*index)),
        );
        let current_witness = ordered_witness(
            current_indices.len(),
            current_indices.iter().map(|index| *current.get(*index)),
        );
        assert_eq!(
            previous_witness, current_witness,
            "lookup fixtures differ across versions"
        );
        let expected_checksum = (0..usize_to_u64(size)).fold(0_u64, u64::wrapping_add);
        run_pairs(
            progress,
            group,
            size,
            repetitions,
            || {
                timed(
                    || {
                        black_box(previous_indices.as_slice())
                            .iter()
                            .map(|index| *previous.get(*index))
                            .fold(0_u64, u64::wrapping_add)
                    },
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        previous_witness
                    },
                )
            },
            || {
                timed(
                    || {
                        black_box(current_indices.as_slice())
                            .iter()
                            .map(|index| *current.get(*index))
                            .fold(0_u64, u64::wrapping_add)
                    },
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        current_witness
                    },
                )
            },
        );
    }
}

fn iteration(progress: &mut Progress, repetitions: usize) {
    let group = "release/iteration";
    for size in parameters(group) {
        let previous: PreviousArena<u64> = (0..usize_to_u64(size)).collect();
        let current: CurrentArena<u64> = (0..usize_to_u64(size)).collect();
        let previous_witness = ordered_witness(previous.len(), previous.iter().copied());
        let current_witness = ordered_witness(current.len(), current.iter().copied());
        assert_eq!(
            previous_witness, current_witness,
            "iteration fixtures differ across versions"
        );
        let expected_checksum = (0..usize_to_u64(size)).fold(0_u64, u64::wrapping_add);
        run_pairs(
            progress,
            group,
            size,
            repetitions,
            || {
                timed(
                    || previous.iter().copied().fold(0_u64, u64::wrapping_add),
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        previous_witness
                    },
                )
            },
            || {
                timed(
                    || current.iter().copied().fold(0_u64, u64::wrapping_add),
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        current_witness
                    },
                )
            },
        );
    }
}

fn speculative_rollback(progress: &mut Progress, repetitions: usize) {
    let group = "release/speculative_rollback";
    for suffix_len in parameters(group) {
        let mut previous: PreviousArena<u64> = (0..1_024_u64).collect();
        let mut current: CurrentArena<u64> = (0..1_024_u64).collect();
        run_pairs(
            progress,
            group,
            suffix_len,
            repetitions,
            || {
                let started = Instant::now();
                let checkpoint = previous.checkpoint();
                for value in 0..suffix_len {
                    black_box(previous.alloc(usize_to_u64(value)));
                }
                previous.rollback(checkpoint);
                let elapsed_ns = started.elapsed().as_nanos();
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(previous.len(), previous.iter().copied()),
                }
            },
            || {
                let started = Instant::now();
                let checkpoint = current.checkpoint();
                for value in 0..suffix_len {
                    black_box(current.alloc(usize_to_u64(value)));
                }
                current.rollback(checkpoint);
                let elapsed_ns = started.elapsed().as_nanos();
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(current.len(), current.iter().copied()),
                }
            },
        );
    }
}

fn concurrent_allocation(progress: &mut Progress, repetitions: usize) {
    let group = "release/shared_concurrent_allocation";
    for thread_count in parameters(group) {
        run_pairs(
            progress,
            group,
            thread_count,
            repetitions,
            || {
                timed(
                    || {
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
                        arena
                    },
                    |arena| shared_content_witness(arena.iter().copied()),
                )
            },
            || {
                timed(
                    || {
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
                        arena
                    },
                    |arena| shared_content_witness(arena.iter().copied()),
                )
            },
        );
    }
}

fn main() {
    let repetitions = repetitions();
    let mut progress = Progress::new(repetitions);
    println!("safe-bump-raw-pairs-v1");
    println!(
        "pair_id\tgroup\tparameter\trepetition\torder\tposition\tversion\telapsed_ns\twitness"
    );
    allocation(&mut progress, repetitions);
    validated_lookup(&mut progress, repetitions);
    iteration(&mut progress, repetitions);
    speculative_rollback(&mut progress, repetitions);
    concurrent_allocation(&mut progress, repetitions);
    progress.finish();
}
