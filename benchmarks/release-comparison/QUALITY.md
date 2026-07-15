# v0.2.1 → v0.3.0 quality contract

Timing alone cannot represent the central v0.3 safety improvement. The
cross-version `quality_contract` tests make the semantic delta executable:

- a v0.2.1 raw slot index can accidentally read an equally numbered slot in a
  foreign arena; v0.3.0 rejects that capability;
- after rollback and slot reuse, a v0.2.1 stale index retargets the replacement
  value (the ABA problem); v0.3.0 rejects it while accepting the fresh index;
- a v0.2.1 checkpoint is only a length and can be applied to another arena;
  v0.3.0 returns `CheckpointError::ForeignArena` without changing the target.

The performance delta therefore reports the measured price of validation next
to the property it buys. Root crate tests cover correctness and panic safety;
benchmark timing does not replace those checks.

Criterion's public table contains each function's marginal interval and a
diagnostic point ratio only. Balanced raw observations carry pair identity,
execution order, and content witnesses. A witness mismatch invalidates the
timing pair instead of comparing semantically different results; the workflow
does not infer a performance winner from a single hosted run.
