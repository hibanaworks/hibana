//! Explicit offer positions and operation-local cycle detection.
use crate::global::typestate::{PRESENT_STATE_INDEX_CAPACITY, StateIndex};

#[inline]
pub(crate) fn checked_state_index(idx: usize) -> Option<StateIndex> {
    if idx < PRESENT_STATE_INDEX_CAPACITY {
        Some(StateIndex::new(idx as u16))
    } else {
        None
    }
}

mod visit_set;
pub(crate) use visit_set::FrontierVisitSet;
