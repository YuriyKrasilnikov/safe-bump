use std::hint::black_box;
use std::thread;
use std::time::Instant;

// Baseline is safe-bump v0.2.1; candidate is the current working version.
use safe_bump_current::{Arena as CandidateArena, SharedArena as CandidateSharedArena};
use safe_bump_release_comparison::{paired_parameters, paired_workloads};
use safe_bump_v2::{Arena as BaselineArena, SharedArena as BaselineSharedArena};

const WARMUPS: usize = 2;
const DEFAULT_REPETITIONS: usize = 15;
const TOTAL_SHARED_ITEMS: usize = 65_536;
const BASELINE_LABEL: &str = "v0.2.1";
const CANDIDATE_LABEL: &str = "v0.3.1";

#[derive(Clone, Copy)]
struct Observation {
    elapsed_ns: u64,
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
            expected: paired_workloads(),
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

fn elapsed_ns(started: Instant) -> u64 {
    let elapsed = u64::try_from(started.elapsed().as_nanos())
        .expect("benchmark interval fits u64 nanoseconds");
    assert!(
        elapsed > 0,
        "zero-length benchmark interval: the clock cannot resolve this observation"
    );
    elapsed
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
    let elapsed_ns = elapsed_ns(started);
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
    mut baseline: impl FnMut() -> Observation,
    mut candidate: impl FnMut() -> Observation,
) {
    progress.begin_workload(group, parameter);
    for _ in 0..WARMUPS {
        black_box(baseline());
        black_box(candidate());
    }

    for repetition in 0..repetitions {
        let pair_id = format!("{group}:{parameter}:{repetition}");
        if repetition % 2 == 0 {
            let before = baseline();
            let after = candidate();
            assert_eq!(
                before.witness, after.witness,
                "content witness mismatch for {pair_id}"
            );
            emit(
                group,
                parameter,
                repetition,
                "baseline-candidate",
                0,
                BASELINE_LABEL,
                before,
            );
            emit(
                group,
                parameter,
                repetition,
                "baseline-candidate",
                1,
                CANDIDATE_LABEL,
                after,
            );
        } else {
            let before = candidate();
            let after = baseline();
            assert_eq!(
                before.witness, after.witness,
                "content witness mismatch for {pair_id}"
            );
            emit(
                group,
                parameter,
                repetition,
                "candidate-baseline",
                0,
                CANDIDATE_LABEL,
                before,
            );
            emit(
                group,
                parameter,
                repetition,
                "candidate-baseline",
                1,
                BASELINE_LABEL,
                after,
            );
        }
        progress.complete_repetition(group, parameter, repetition);
    }
    progress.complete_workload();
}

fn allocation_no_growth(progress: &mut Progress, repetitions: usize) {
    let group = "release/allocation_no_growth";
    for size in paired_parameters(group) {
        run_pairs(
            progress,
            group,
            size,
            repetitions,
            || {
                let mut arena = BaselineArena::with_capacity(size);
                let started = Instant::now();
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(arena.len(), arena.iter().copied()),
                }
            },
            || {
                let mut arena = CandidateArena::with_capacity(size);
                let started = Instant::now();
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(arena.len(), arena.iter().copied()),
                }
            },
        );
    }
}

fn allocation_growth(progress: &mut Progress, repetitions: usize) {
    let group = "release/allocation_growth";
    for size in paired_parameters(group) {
        run_pairs(
            progress,
            group,
            size,
            repetitions,
            || {
                let mut arena = BaselineArena::new();
                let started = Instant::now();
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(arena.len(), arena.iter().copied()),
                }
            },
            || {
                let mut arena = CandidateArena::new();
                let started = Instant::now();
                for value in 0..size {
                    black_box(arena.alloc(usize_to_u64(value)));
                }
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(arena.len(), arena.iter().copied()),
                }
            },
        );
    }
}

fn arena_creation(progress: &mut Progress, repetitions: usize) {
    let group = "release/arena_creation";
    for count in paired_parameters(group) {
        run_pairs(
            progress,
            group,
            count,
            repetitions,
            || {
                let started = Instant::now();
                let arenas: Vec<BaselineArena<u64>> =
                    (0..count).map(|_| BaselineArena::new()).collect();
                black_box(&arenas);
                Observation {
                    elapsed_ns: elapsed_ns(started),
                    witness: usize_to_u64(arenas.len()),
                }
            },
            || {
                let started = Instant::now();
                let arenas: Vec<CandidateArena<u64>> =
                    (0..count).map(|_| CandidateArena::new()).collect();
                black_box(&arenas);
                Observation {
                    elapsed_ns: elapsed_ns(started),
                    witness: usize_to_u64(arenas.len()),
                }
            },
        );
    }
}

fn arena_with_capacity(progress: &mut Progress, repetitions: usize) {
    let group = "release/arena_with_capacity";
    for capacity in paired_parameters(group) {
        run_pairs(
            progress,
            group,
            capacity,
            repetitions,
            || {
                timed(
                    || BaselineArena::<u64>::with_capacity(capacity),
                    |arena| usize_to_u64(arena.capacity()),
                )
            },
            || {
                timed(
                    || CandidateArena::<u64>::with_capacity(capacity),
                    |arena| usize_to_u64(arena.capacity()),
                )
            },
        );
    }
}

fn validated_lookup(progress: &mut Progress, repetitions: usize) {
    let group = "release/validated_lookup";
    for size in paired_parameters(group) {
        let mut baseline = BaselineArena::with_capacity(size);
        let baseline_indices: Vec<_> = (0..size)
            .map(|value| baseline.alloc(usize_to_u64(value)))
            .collect();
        let mut candidate = CandidateArena::with_capacity(size);
        let candidate_indices: Vec<_> = (0..size)
            .map(|value| candidate.alloc(usize_to_u64(value)))
            .collect();
        let baseline_witness = ordered_witness(
            baseline_indices.len(),
            baseline_indices.iter().map(|index| *baseline.get(*index)),
        );
        let candidate_witness = ordered_witness(
            candidate_indices.len(),
            candidate_indices.iter().map(|index| *candidate.get(*index)),
        );
        assert_eq!(
            baseline_witness, candidate_witness,
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
                        black_box(baseline_indices.as_slice())
                            .iter()
                            .map(|index| *baseline.get(*index))
                            .fold(0_u64, u64::wrapping_add)
                    },
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        baseline_witness
                    },
                )
            },
            || {
                timed(
                    || {
                        black_box(candidate_indices.as_slice())
                            .iter()
                            .map(|index| *candidate.get(*index))
                            .fold(0_u64, u64::wrapping_add)
                    },
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        candidate_witness
                    },
                )
            },
        );
    }
}

fn iteration(progress: &mut Progress, repetitions: usize) {
    let group = "release/iteration";
    for size in paired_parameters(group) {
        let baseline: BaselineArena<u64> = (0..usize_to_u64(size)).collect();
        let candidate: CandidateArena<u64> = (0..usize_to_u64(size)).collect();
        let baseline_witness = ordered_witness(baseline.len(), baseline.iter().copied());
        let candidate_witness = ordered_witness(candidate.len(), candidate.iter().copied());
        assert_eq!(
            baseline_witness, candidate_witness,
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
                    || baseline.iter().copied().fold(0_u64, u64::wrapping_add),
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        baseline_witness
                    },
                )
            },
            || {
                timed(
                    || candidate.iter().copied().fold(0_u64, u64::wrapping_add),
                    |checksum| {
                        assert_eq!(*checksum, expected_checksum);
                        candidate_witness
                    },
                )
            },
        );
    }
}

fn speculative_rollback(progress: &mut Progress, repetitions: usize) {
    let group = "release/speculative_rollback";
    for suffix_len in paired_parameters(group) {
        let mut baseline: BaselineArena<u64> = (0..1_024_u64).collect();
        let mut candidate: CandidateArena<u64> = (0..1_024_u64).collect();
        run_pairs(
            progress,
            group,
            suffix_len,
            repetitions,
            || {
                let started = Instant::now();
                let checkpoint = baseline.checkpoint();
                for value in 0..suffix_len {
                    black_box(baseline.alloc(usize_to_u64(value)));
                }
                baseline.rollback(checkpoint);
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(baseline.len(), baseline.iter().copied()),
                }
            },
            || {
                let started = Instant::now();
                let checkpoint = candidate.checkpoint();
                for value in 0..suffix_len {
                    black_box(candidate.alloc(usize_to_u64(value)));
                }
                candidate.rollback(checkpoint);
                let elapsed_ns = elapsed_ns(started);
                Observation {
                    elapsed_ns,
                    witness: ordered_witness(candidate.len(), candidate.iter().copied()),
                }
            },
        );
    }
}

fn concurrent_allocation(progress: &mut Progress, repetitions: usize) {
    let group = "release/shared_concurrent_allocation";
    for thread_count in paired_parameters(group) {
        run_pairs(
            progress,
            group,
            thread_count,
            repetitions,
            || {
                timed(
                    || {
                        let arena = BaselineSharedArena::new();
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
                        let arena = CandidateSharedArena::new();
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
    println!("safe-bump-paired-raw-v2");
    println!(
        "pair_id\tgroup\tparameter\trepetition\torder\tposition\tversion\telapsed_ns\twitness"
    );
    allocation_no_growth(&mut progress, repetitions);
    allocation_growth(&mut progress, repetitions);
    arena_creation(&mut progress, repetitions);
    arena_with_capacity(&mut progress, repetitions);
    validated_lookup(&mut progress, repetitions);
    iteration(&mut progress, repetitions);
    speculative_rollback(&mut progress, repetitions);
    concurrent_allocation(&mut progress, repetitions);
    progress.finish();
}
