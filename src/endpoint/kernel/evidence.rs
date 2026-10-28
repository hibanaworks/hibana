//! Scope-evidence owners for route selection.

use super::authority::Arm;
use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteArmState {
    pub(super) scope: ScopeId,
    pub(super) arm: u8,
}

impl RouteArmState {
    pub(super) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
    };

    #[inline]
    pub(super) const fn new(scope: ScopeId, arm: u8) -> Self {
        Self { scope, arm }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScopeEvidenceStatus {
    Clear = 0,
    Conflicted = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScopeEvidence {
    pub(super) ready_arm_mask: u8,
    pub(super) poll_ready_arm_mask: u8,
    pub(super) status: ScopeEvidenceStatus,
}

impl ScopeEvidence {
    pub(super) const EMPTY: Self = Self {
        ready_arm_mask: 0,
        poll_ready_arm_mask: 0,
        status: ScopeEvidenceStatus::Clear,
    };

    #[inline]
    pub(super) const fn arm_bit(arm: Arm) -> u8 {
        1 << arm.as_u8()
    }
}
