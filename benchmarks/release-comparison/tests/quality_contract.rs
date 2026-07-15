use safe_bump_current::{Arena as CurrentArena, CheckpointError};
use safe_bump_v2::Arena as PreviousArena;

#[test]
fn v03_rejects_foreign_indices_that_v02_cannot_distinguish() {
    let mut previous_left = PreviousArena::new();
    let previous_foreign = previous_left.alloc(10_u64);
    let mut previous_right = PreviousArena::new();
    previous_right.alloc(20_u64);
    assert_eq!(previous_right.try_get(previous_foreign), Some(&20));

    let mut current_left = CurrentArena::new();
    let current_foreign = current_left.alloc(10_u64);
    let mut current_right = CurrentArena::new();
    current_right.alloc(20_u64);
    assert_eq!(current_right.try_get(current_foreign), None);
}

#[test]
fn v03_rejects_stale_aba_indices_that_v02_retargets() {
    let mut previous = PreviousArena::new();
    previous.alloc(1_u64);
    let previous_checkpoint = previous.checkpoint();
    let previous_stale = previous.alloc(2);
    previous.rollback(previous_checkpoint);
    previous.alloc(3);
    assert_eq!(previous.try_get(previous_stale), Some(&3));

    let mut current = CurrentArena::new();
    current.alloc(1_u64);
    let current_checkpoint = current.checkpoint();
    let current_stale = current.alloc(2);
    current.rollback(current_checkpoint);
    let current_replacement = current.alloc(3);
    assert_eq!(current.try_get(current_stale), None);
    assert_eq!(current.try_get(current_replacement), Some(&3));
}

#[test]
fn v03_rejects_foreign_checkpoints_that_v02_accepts_by_length() {
    let previous_left = PreviousArena::<u64>::new();
    let previous_foreign = previous_left.checkpoint();
    let mut previous_right = PreviousArena::new();
    previous_right.alloc(2_u64);
    previous_right.rollback(previous_foreign);
    assert!(previous_right.is_empty());

    let current_left = CurrentArena::<u64>::new();
    let current_foreign = current_left.checkpoint();
    let mut current_right = CurrentArena::new();
    current_right.alloc(2_u64);
    assert_eq!(
        current_right.try_rollback(current_foreign),
        Err(CheckpointError::ForeignArena)
    );
    assert_eq!(current_right.len(), 1);
}
