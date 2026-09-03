# v0.2.1 → v0.3.1 quality contract

Timing alone cannot represent the central v0.3 safety improvement. The
cross-version `quality_contract` tests make the semantic delta executable:

- a v0.2.1 raw slot index can accidentally read an equally numbered slot in a
  foreign arena; v0.3.1 rejects that capability. v0.2.1 documents this as the
  caller's responsibility — `Idx::from_raw` states that the caller must
  ensure the index is valid for the target arena — so this is a documented
  compromise, not a defect;
- after rollback and slot reuse, a v0.2.1 stale index retargets the replacement
  value (the ABA problem); v0.3.1 rejects it while accepting the fresh index;
- a v0.2.1 checkpoint is only a length and can be applied to another arena;
  v0.3.1 returns `CheckpointError::ForeignArena` without changing the target;
- a v0.2.1 `SharedArena::rollback` updates its publication counters only
  after every dropped value's destructor returns, so a destructor panic
  mid-rollback leaves `published` pointing past a slot whose value was
  already removed: the stale index still reports `is_valid`, but `try_get`
  and `iter` then panic on the empty slot. v0.3.1 updates a slot's counters
  before running that slot's destructor, so the same destructor panic
  leaves `len()` at the correctly reduced count, `try_get` returns `None`,
  and `iter` succeeds;
- a v0.2.1 `SharedArena::alloc_extend` reserves one slot per yielded item
  through repeated `&self` calls to `alloc`, so a concurrent or reentrant
  allocation between two yields can land inside the slot range the batch
  occupies. `alloc_extend`'s own doc promises only the index of the first
  allocated item, not contiguity, so this is not a broken promise — it is an
  asymmetry with `Arena::alloc_extend`, whose `&mut self` receiver makes its
  batch contiguous by construction. v0.3.1's `SharedArena::alloc_block`
  collects the input first, reserves its whole range in one atomic step, and
  documents the returned `Block` as contiguous.

The performance delta therefore reports the measured price of validation next
to the property it buys. Root crate tests cover correctness and panic safety;
benchmark timing does not replace those checks.

Criterion's public table contains each function's marginal interval and a
diagnostic point ratio only. Alternating-order raw observations use a separate closed
matrix that distinguishes allocation work from capacity growth and arena
construction. They carry pair identity, execution order, and content
witnesses. A witness mismatch or a non-positive duration invalidates the
timing pair instead of comparing semantically different results; the workflow
does not infer a performance winner from a single hosted run.
