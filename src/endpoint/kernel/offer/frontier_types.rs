//! Offer frontier observation and materialization metadata.

use super::super::frontier::FrontierKind;
use super::Arm;
use super::first_recv_dispatch::FirstRecvDispatchCache;
use crate::eff::{EffIndex, EventOrigin};
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::StateIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct CachedRecvMeta {
    pub(in crate::endpoint::kernel) eff_index: EffIndex,
    pub(in crate::endpoint::kernel) cursor_index: StateIndex,
    pub(in crate::endpoint::kernel) peer: u8,
    pub(in crate::endpoint::kernel) label: u8,
    pub(in crate::endpoint::kernel) frame_label: u8,
    pub(in crate::endpoint::kernel) origin: EventOrigin,
    pub(in crate::endpoint::kernel) lane: u8,
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CachedRecvMeta {
    pub(in crate::endpoint::kernel) const FLAG_RECV_STEP: u8 = 1;
    pub(in crate::endpoint::kernel) const FLAG_NEXT_PRESENT: u8 = 1 << 1;

    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        eff_index: EffIndex::ZERO,
        cursor_index: StateIndex::ABSENT,
        peer: 0,
        label: 0,
        frame_label: 0,
        origin: EventOrigin::User,
        lane: 0,
        flags: 0,
    };

    #[inline]
    pub(in crate::endpoint::kernel) const fn next_presence_flag(next: StateIndex) -> u8 {
        if next.is_absent() {
            0
        } else {
            Self::FLAG_NEXT_PRESENT
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn is_empty(&self) -> bool {
        self.cursor_index.is_absent() || (self.flags & Self::FLAG_NEXT_PRESENT) == 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn is_recv_step(&self) -> bool {
        (self.flags & Self::FLAG_RECV_STEP) != 0
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) passive_arm_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) passive_child_scope: [ScopeId; 2],
    pub(in crate::endpoint::kernel) first_recv_dispatch: FirstRecvDispatchCache,
}

impl ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        passive_arm_entry: [StateIndex::ABSENT; 2],
        passive_child_scope: [ScopeId::none(); 2],
        first_recv_dispatch: FirstRecvDispatchCache::EMPTY,
    };

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_arm_entry(&self, arm: Arm) -> Option<StateIndex> {
        let entry = self.passive_arm_entry[arm.as_u8() as usize];
        (!entry.is_absent()).then_some(entry)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_child_scope(&self, arm: Arm) -> Option<ScopeId> {
        let scope = self.passive_child_scope[arm.as_u8() as usize];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_first_recv_dispatch(&mut self, arm_mask: u8) {
        self.first_recv_dispatch.record(arm_mask);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_first_recv_dispatch(&self, arm: Arm) -> bool {
        self.first_recv_dispatch.arm_has_dispatch(arm)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CurrentScopeSelectionMeta {
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CurrentScopeSelectionMeta {
    pub(in crate::endpoint::kernel) const FLAG_ROUTE_ENTRY: u8 = 1;
    pub(in crate::endpoint::kernel) const FLAG_HAS_OFFER_LANES: u8 = 1 << 1;
    pub(in crate::endpoint::kernel) const FLAG_CONTROLLER: u8 = 1 << 2;

    pub(in crate::endpoint::kernel) const EMPTY: Self = Self { flags: 0 };

    #[inline]
    pub(in crate::endpoint::kernel) fn is_route_entry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_ENTRY) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn has_offer_lanes(self) -> bool {
        !self.is_route_entry() || (self.flags & Self::FLAG_HAS_OFFER_LANES) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::CachedRecvMeta;

    #[test]
    fn cached_recv_meta_is_exactly_ten_bytes() {
        assert_eq!(core::mem::size_of::<CachedRecvMeta>(), 10);
        assert_eq!(core::mem::align_of::<CachedRecvMeta>(), 2);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::endpoint::kernel) enum CurrentReentryControllerEvidence {
    ProgressSatisfiedOrNotController,
    ProgressEvidenceAbsent,
}

impl CurrentReentryControllerEvidence {
    #[inline]
    pub(in crate::endpoint::kernel) const fn allows_cross_frontier_progress_sibling(self) -> bool {
        matches!(self, Self::ProgressEvidenceAbsent)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CurrentFrontierSelectionState {
    pub(in crate::endpoint::kernel) frontier: FrontierKind,
    pub(in crate::endpoint::kernel) parallel_root: ScopeId,
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CurrentFrontierSelectionState {
    pub(in crate::endpoint::kernel) const FLAG_CONTROLLER: u8 = 1;
    pub(in crate::endpoint::kernel) const FLAG_READY: u8 = 1 << 1;
    pub(in crate::endpoint::kernel) const FLAG_PROGRESS_EVIDENCE: u8 = 1 << 2;

    #[inline]
    pub(in crate::endpoint::kernel) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS_EVIDENCE) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_ready(&mut self) {
        self.flags |= Self::FLAG_READY;
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_progress_evidence(&mut self) {
        self.flags |= Self::FLAG_PROGRESS_EVIDENCE;
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn parallel(self) -> Option<ScopeId> {
        if self.parallel_root.is_none() {
            None
        } else {
            Some(self.parallel_root)
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn reentry_controller_evidence(
        self,
    ) -> CurrentReentryControllerEvidence {
        if self.frontier == FrontierKind::Reentry
            && self.is_controller()
            && self.ready()
            && !self.has_progress_evidence()
        {
            CurrentReentryControllerEvidence::ProgressEvidenceAbsent
        } else {
            CurrentReentryControllerEvidence::ProgressSatisfiedOrNotController
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FrontierFacts {
    pub(in crate::endpoint::kernel) frontier: FrontierKind,
    pub(in crate::endpoint::kernel) readiness: FrontierReadiness,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) enum FrontierReadiness {
    Waiting = 0,
    Ready = 1,
}

impl FrontierReadiness {
    #[inline]
    pub(in crate::endpoint::kernel) const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl FrontierFacts {
    #[inline]
    pub(in crate::endpoint::kernel) const fn ready(self) -> bool {
        self.readiness.is_ready()
    }
}
