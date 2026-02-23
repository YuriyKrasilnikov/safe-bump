use std::cell::Cell;
use std::rc::Rc;

use proptest::prelude::*;

use super::*;

#[derive(Debug, Clone)]
enum Op {
    Alloc(i32),
    Checkpoint,
    Rollback(usize),
}

#[derive(Debug, Clone, Copy)]
enum DropOp {
    Alloc,
    Checkpoint,
    Rollback(usize),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => any::<i32>().prop_map(Op::Alloc),
        1 => Just(Op::Checkpoint),
        1 => (0..20_usize).prop_map(Op::Rollback),
    ]
}

fn drop_op_strategy() -> impl Strategy<Value = DropOp> {
    prop_oneof![
        3 => Just(DropOp::Alloc),
        1 => Just(DropOp::Checkpoint),
        1 => (0..20_usize).prop_map(DropOp::Rollback),
    ]
}

proptest! {
    /// Random alloc/checkpoint/rollback sequence — arena contents
    /// always match a `Vec<i32>` model, stale indices are invalid.
    #[test]
    fn arbitrary_ops_preserve_model(ops in prop::collection::vec(op_strategy(), 0..200)) {
        let mut arena = Arena::new();
        let mut model: Vec<i32> = Vec::new();
        let mut arena_cps: Vec<Checkpoint<i32>> = Vec::new();
        let mut high_water: usize = 0;

        for op in ops {
            match op {
                Op::Alloc(val) => {
                    arena.alloc(val);
                    model.push(val);
                    high_water = high_water.max(model.len());
                }
                Op::Checkpoint => {
                    arena_cps.push(arena.checkpoint());
                }
                Op::Rollback(raw) => {
                    if !arena_cps.is_empty() {
                        let pos = raw % arena_cps.len();
                        let saved_len = arena_cps[pos].len();
                        arena.rollback(arena_cps[pos]);
                        model.truncate(saved_len);
                        arena_cps.truncate(pos);
                    }
                }
            }

            // len matches model
            prop_assert_eq!(arena.len(), model.len());

            // contents match model
            let arena_vals: Vec<i32> = arena.iter().copied().collect();
            prop_assert_eq!(&arena_vals, &model);

            // stale indices are invalid
            for i in arena.len()..high_water {
                prop_assert!(!arena.is_valid(Idx::from_raw(i)));
            }
        }
    }

    /// Random alloc/checkpoint/rollback sequence — every allocated
    /// value is dropped exactly once (no leak, no double-drop).
    #[test]
    fn arbitrary_ops_drop_exactly_once(
        ops in prop::collection::vec(drop_op_strategy(), 0..200)
    ) {
        let drop_count = Rc::new(Cell::new(0u32));
        let mut arena: Arena<Tracked> = Arena::new();
        let mut arena_cps: Vec<Checkpoint<Tracked>> = Vec::new();
        let mut total_allocated: u32 = 0;

        for &op in &ops {
            match op {
                DropOp::Alloc => {
                    arena.alloc(Tracked(Rc::clone(&drop_count)));
                    total_allocated += 1;
                }
                DropOp::Checkpoint => {
                    arena_cps.push(arena.checkpoint());
                }
                DropOp::Rollback(raw) => {
                    if !arena_cps.is_empty() {
                        let pos = raw % arena_cps.len();
                        arena.rollback(arena_cps[pos]);
                        arena_cps.truncate(pos);
                    }
                }
            }

            // invariant: dropped + live = total allocated
            let live = u32::try_from(arena.len()).unwrap();
            prop_assert_eq!(
                drop_count.get() + live,
                total_allocated,
                "dropped({}) + live({}) != total({})",
                drop_count.get(),
                live,
                total_allocated,
            );
        }

        // after arena drop: all values dropped
        drop(arena);
        prop_assert_eq!(drop_count.get(), total_allocated);
    }

    // ─── SharedArena property tests ────────────────────────────────────

    /// Random alloc/checkpoint/rollback sequence on SharedArena —
    /// contents always match model, stale indices are invalid.
    #[test]
    fn shared_arena_arbitrary_ops_preserve_model(
        ops in prop::collection::vec(op_strategy(), 0..200)
    ) {
        let mut arena = SharedArena::new();
        let mut model: Vec<(Idx<i32>, i32)> = Vec::new();
        let mut arena_cps: Vec<Checkpoint<i32>> = Vec::new();
        let mut high_water: usize = 0;

        for op in ops {
            match op {
                Op::Alloc(val) => {
                    let idx = arena.alloc(val);
                    model.push((idx, val));
                    high_water = high_water.max(model.len());
                }
                Op::Checkpoint => {
                    arena_cps.push(arena.checkpoint());
                }
                Op::Rollback(raw) => {
                    if !arena_cps.is_empty() {
                        let pos = raw % arena_cps.len();
                        let saved_len = arena_cps[pos].len();
                        arena.rollback(arena_cps[pos]);
                        model.truncate(saved_len);
                        arena_cps.truncate(pos);
                    }
                }
            }

            // len matches model
            prop_assert_eq!(arena.len(), model.len());

            // contents match model (via get, SharedArena has no iter)
            for &(idx, expected) in &model {
                prop_assert_eq!(*arena.get(idx), expected);
            }

            // stale indices are invalid
            for i in arena.len()..high_water {
                prop_assert!(!arena.is_valid(Idx::from_raw(i)));
            }
        }
    }

    /// Random alloc/checkpoint/rollback sequence on SharedArena —
    /// every allocated value is dropped exactly once.
    #[test]
    fn shared_arena_arbitrary_ops_drop_exactly_once(
        ops in prop::collection::vec(drop_op_strategy(), 0..200)
    ) {
        let drop_count = Rc::new(Cell::new(0u32));
        let mut arena: SharedArena<Tracked> = SharedArena::new();
        let mut arena_cps: Vec<Checkpoint<Tracked>> = Vec::new();
        let mut total_allocated: u32 = 0;

        for &op in &ops {
            match op {
                DropOp::Alloc => {
                    arena.alloc(Tracked(Rc::clone(&drop_count)));
                    total_allocated += 1;
                }
                DropOp::Checkpoint => {
                    arena_cps.push(arena.checkpoint());
                }
                DropOp::Rollback(raw) => {
                    if !arena_cps.is_empty() {
                        let pos = raw % arena_cps.len();
                        arena.rollback(arena_cps[pos]);
                        arena_cps.truncate(pos);
                    }
                }
            }

            // invariant: dropped + live = total allocated
            let live = u32::try_from(arena.len()).unwrap();
            prop_assert_eq!(
                drop_count.get() + live,
                total_allocated,
                "dropped({}) + live({}) != total({})",
                drop_count.get(),
                live,
                total_allocated,
            );
        }

        // after arena drop: all values dropped
        drop(arena);
        prop_assert_eq!(drop_count.get(), total_allocated);
    }
}
