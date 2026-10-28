//! Route-decision resolution state and outcomes.

use super::super::authority::RouteArmToken;
use super::{BranchKind, CachedRecvMeta};
use crate::global::typestate::EnabledEventCommit;
#[derive(Clone, Copy)]
pub(super) enum ResolvePendingState {
    Ready,
    YieldRestartUnarmed,
    YieldRestartArmed,
    IntrinsicPassiveProgress { selected_arm: u8 },
}

impl ResolvePendingState {
    #[inline]
    pub(super) const fn ready() -> Self {
        Self::Ready
    }

    #[inline]
    pub(super) const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        *self = Self::Ready;
    }

    #[inline]
    pub(super) fn arm_yield_restart(&mut self) {
        *self = Self::YieldRestartUnarmed;
    }

    #[inline]
    pub(super) fn arm_intrinsic_passive_progress(&mut self, selected_arm: u8) {
        *self = Self::IntrinsicPassiveProgress { selected_arm };
    }

    #[inline]
    pub(super) fn complete_yield_turn(&mut self) {
        *self = Self::YieldRestartArmed;
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::endpoint::kernel) enum RouteArmCommitEvidence {
    Resolver,
    PollFrame,
    Committed,
}

impl RouteArmCommitEvidence {
    #[inline]
    pub(in crate::endpoint::kernel) const fn emits_route_arm_selection_event(self) -> bool {
        matches!(self, Self::PollFrame)
    }
}

/// One admission result for a callback-free read phase. No pending/public
/// branch state stores this value; a callback discards its grant, keeping only
/// the immutable metadata as intent for a fresh admission.
pub(in crate::endpoint::kernel) struct AdmittedOfferEvent {
    pub(in crate::endpoint::kernel) metadata: CachedRecvMeta,
    pub(in crate::endpoint::kernel) enabled: EnabledEventCommit,
    pub(in crate::endpoint::kernel) kind: BranchKind,
}

pub(in crate::endpoint::kernel) struct ResolvedRouteArm {
    pub(in crate::endpoint::kernel) route_token: RouteArmToken,
    pub(in crate::endpoint::kernel) selected_arm: u8,
    pub(in crate::endpoint::kernel) route_arm_selection_commit_evidence: RouteArmCommitEvidence,
    pub(in crate::endpoint::kernel) outbound: Option<AdmittedOfferEvent>,
}

pub(in crate::endpoint::kernel) enum ResolveTokenOutcome {
    RestartFrontier,
    Resolved(ResolvedRouteArm),
}
