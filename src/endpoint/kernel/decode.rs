//! Decode-path helpers for `RouteBranch`.

use crate::endpoint::RecvError;

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
