//! Scope-evidence owners for route selection.

use super::authority::{Arm, RouteArmToken};
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
}

#[derive(Clone, Copy)]
pub(super) struct ScopeReentryMeta {
    pub(super) flags: u8,
}

impl ScopeReentryMeta {
    pub(super) const FLAG_SCOPE_ACTIVE: u8 = 1;
    pub(super) const FLAG_ROUTE_REENTRY: u8 = 1 << 1;
    pub(super) const FLAG_ARM0_HAS_RECV: u8 = 1 << 3;
    pub(super) const FLAG_ARM1_HAS_RECV: u8 = 1 << 4;

    #[inline]
    pub(super) fn scope_active(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_ACTIVE) != 0
    }

    #[inline]
    pub(super) fn route_reentry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_REENTRY) != 0
    }

    #[inline]
    pub(super) fn arm0_has_recv(self) -> bool {
        (self.flags & Self::FLAG_ARM0_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn arm1_has_recv(self) -> bool {
        (self.flags & Self::FLAG_ARM1_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn recvless_arm_ready(self) -> bool {
        (self.scope_active() || self.route_reentry())
            && (!self.arm0_has_recv() || !self.arm1_has_recv())
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidence {
    pub(super) ack: Option<RouteArmToken>,
    pub(super) ready_arm_mask: u8,
    pub(super) poll_ready_arm_mask: u8,
    pub(super) flags: u8,
}

impl ScopeEvidence {
    pub(super) const FLAG_ACK_CONFLICT: u8 = 1;
    pub(super) const EMPTY: Self = Self {
        ack: None,
        ready_arm_mask: 0,
        poll_ready_arm_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(super) const fn arm_bit(arm: Arm) -> u8 {
        1 << arm.as_u8()
    }
}
