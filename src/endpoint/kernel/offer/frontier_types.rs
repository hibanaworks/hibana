//! Offer frontier observation and materialization metadata.

use super::super::frontier::FrontierKind;
use super::first_recv_dispatch::FirstRecvDispatchCache;
use crate::eff::{EffIndex, EventOrigin};
use crate::global::compiled::images::EventSemanticKind;
use crate::global::const_dsl::{RouteResolver, ScopeId};
use crate::global::typestate::{RecvMeta, RouteChoiceMark, StateIndex, state_index_to_usize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct FrontierObservationDomain {
    root: ScopeId,
}

impl FrontierObservationDomain {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn global() -> Self {
        Self {
            root: ScopeId::none(),
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn root(root: ScopeId) -> Self {
        Self { root }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn from_parallel(root: Option<ScopeId>) -> Self {
        match root {
            Some(root) => Self::root(root),
            None => Self::global(),
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn root_scope(self) -> ScopeId {
        self.root
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn uses_root_entries(self) -> bool {
        !self.root.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(in crate::endpoint::kernel) struct CachedRouteArm(u8);

impl CachedRouteArm {
    const ABSENT_RAW: u8 = u8::MAX;

    #[inline]
    pub(in crate::endpoint::kernel) const fn none() -> Self {
        Self(Self::ABSENT_RAW)
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn some(arm: u8) -> Self {
        if arm == Self::ABSENT_RAW {
            crate::invariant();
        }
        Self(arm)
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn from_option(arm: Option<u8>) -> Self {
        match arm {
            Some(arm) => Self::some(arm),
            None => Self::none(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn as_option(self) -> Option<u8> {
        if self.0 == Self::ABSENT_RAW {
            None
        } else {
            Some(self.0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct CachedRecvMeta {
    pub(in crate::endpoint::kernel) cursor_index: StateIndex,
    pub(in crate::endpoint::kernel) eff_index: EffIndex,
    pub(in crate::endpoint::kernel) peer: u8,
    pub(in crate::endpoint::kernel) label: u8,
    pub(in crate::endpoint::kernel) frame_label: u8,
    pub(in crate::endpoint::kernel) semantic: EventSemanticKind,
    pub(in crate::endpoint::kernel) origin: EventOrigin,
    pub(in crate::endpoint::kernel) next: StateIndex,
    pub(in crate::endpoint::kernel) scope: ScopeId,
    pub(in crate::endpoint::kernel) route_arm: CachedRouteArm,
    pub(in crate::endpoint::kernel) choice: RouteChoiceMark,
    pub(in crate::endpoint::kernel) resolver: RouteResolver,
    pub(in crate::endpoint::kernel) lane: u8,
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CachedRecvMeta {
    pub(in crate::endpoint::kernel) const FLAG_RECV_STEP: u8 = 1;

    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        cursor_index: StateIndex::ABSENT,
        eff_index: EffIndex::ZERO,
        peer: 0,
        label: 0,
        frame_label: 0,
        semantic: EventSemanticKind::ProtocolEvent,
        origin: EventOrigin::User,
        next: StateIndex::ABSENT,
        scope: ScopeId::none(),
        route_arm: CachedRouteArm::none(),
        choice: RouteChoiceMark::Ordinary,
        resolver: RouteResolver::Intrinsic,
        lane: 0,
        flags: 0,
    };

    #[inline]
    pub(in crate::endpoint::kernel) const fn is_empty(&self) -> bool {
        self.cursor_index.is_absent() || self.next.is_absent()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn recv_meta(&self) -> Option<(usize, RecvMeta)> {
        if self.is_empty() {
            return None;
        }
        Some((
            state_index_to_usize(self.cursor_index),
            RecvMeta {
                eff_index: self.eff_index,
                peer: self.peer,
                label: self.label,
                frame_label: self.frame_label,
                semantic: self.semantic,
                origin: self.origin,
                next: state_index_to_usize(self.next),
                scope: self.scope,
                route_arm: self.route_arm.as_option(),
                choice: self.choice,
                resolver: self.resolver,
                lane: self.lane,
            },
        ))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn is_recv_step(&self) -> bool {
        (self.flags & Self::FLAG_RECV_STEP) != 0
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) arm_count: u8,
    pub(in crate::endpoint::kernel) controller_arm_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) controller_arm_label: [u8; 2],
    pub(in crate::endpoint::kernel) controller_cross_role_recv_mask: u8,
    pub(in crate::endpoint::kernel) recv_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) passive_arm_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) passive_child_scope: [ScopeId; 2],
    pub(in crate::endpoint::kernel) first_recv_dispatch: FirstRecvDispatchCache,
}

impl ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        arm_count: 0,
        controller_arm_entry: [StateIndex::ABSENT; 2],
        controller_arm_label: [0; 2],
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::ABSENT; 2],
        passive_arm_entry: [StateIndex::ABSENT; 2],
        passive_child_scope: [ScopeId::none(); 2],
        first_recv_dispatch: FirstRecvDispatchCache::EMPTY,
    };

    #[inline]
    pub(in crate::endpoint::kernel) fn controller_arm_entry(
        &self,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.controller_arm_entry[arm];
        (!entry.is_absent()).then_some((entry, self.controller_arm_label[arm]))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn recv_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.recv_entry[arm];
        (!entry.is_absent()).then_some(entry)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_arm_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.passive_arm_entry[arm];
        (!entry.is_absent()).then_some(entry)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_child_scope(&self, arm: u8) -> Option<ScopeId> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let scope = self.passive_child_scope[arm];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_first_recv_dispatch(&mut self, arm_mask: u8) {
        self.first_recv_dispatch.record(arm_mask);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_first_recv_dispatch(&self, arm: u8) -> bool {
        self.first_recv_dispatch.arm_has_dispatch(arm)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn controller_arm_requires_ready_evidence(
        &self,
        arm: u8,
    ) -> bool {
        arm < 2 && (self.controller_cross_role_recv_mask & (1u8 << arm)) != 0
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
    pub(in crate::endpoint::kernel) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(in crate::endpoint::kernel) const FLAG_READY: u8 = 1 << 2;
    pub(in crate::endpoint::kernel) const FLAG_PROGRESS_EVIDENCE: u8 = 1 << 3;

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
