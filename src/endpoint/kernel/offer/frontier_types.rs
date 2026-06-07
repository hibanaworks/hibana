//! Offer frontier observation and materialization metadata.

#[cfg(test)]
use super::super::frontier::FrontierCandidate;
use super::super::frontier::FrontierKind;
use super::first_recv_dispatch::FirstRecvDispatchCache;
use crate::control::cap::mint::CapShot;
use crate::eff::EffIndex;
use crate::global::compiled::images::ControlSemanticKind;
use crate::global::const_dsl::{PolicyMode, ScopeId};
use crate::global::typestate::{
    FirstRecvDispatchSpec, MAX_FIRST_RECV_DISPATCH, RecvMeta, StateIndex, state_index_to_usize,
};

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
pub(in crate::endpoint::kernel) struct CachedRecvMeta {
    pub(in crate::endpoint::kernel) cursor_index: StateIndex,
    pub(in crate::endpoint::kernel) eff_index: EffIndex,
    pub(in crate::endpoint::kernel) peer: u8,
    pub(in crate::endpoint::kernel) label: u8,
    pub(in crate::endpoint::kernel) frame_label: u8,
    pub(in crate::endpoint::kernel) resource: Option<u8>,
    pub(in crate::endpoint::kernel) semantic: ControlSemanticKind,
    pub(in crate::endpoint::kernel) is_control: bool,
    pub(in crate::endpoint::kernel) next: StateIndex,
    pub(in crate::endpoint::kernel) scope: ScopeId,
    pub(in crate::endpoint::kernel) route_arm: u8,
    pub(in crate::endpoint::kernel) is_choice_determinant: bool,
    pub(in crate::endpoint::kernel) shot: Option<CapShot>,
    pub(in crate::endpoint::kernel) policy: PolicyMode,
    pub(in crate::endpoint::kernel) lane: u8,
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CachedRecvMeta {
    pub(in crate::endpoint::kernel) const FLAG_RECV_STEP: u8 = 1;

    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        cursor_index: StateIndex::MAX,
        eff_index: EffIndex::ZERO,
        peer: 0,
        label: 0,
        frame_label: 0,
        resource: None,
        semantic: ControlSemanticKind::Other,
        is_control: false,
        next: StateIndex::MAX,
        scope: ScopeId::none(),
        route_arm: u8::MAX,
        is_choice_determinant: false,
        shot: None,
        policy: PolicyMode::static_mode(),
        lane: 0,
        flags: 0,
    };

    #[inline]
    pub(in crate::endpoint::kernel) const fn is_empty(&self) -> bool {
        self.cursor_index.is_max() || self.next.is_max()
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
                resource: self.resource,
                semantic: self.semantic,
                is_control: self.is_control,
                next: state_index_to_usize(self.next),
                scope: self.scope,
                route_arm: (self.route_arm != u8::MAX).then_some(self.route_arm),
                is_choice_determinant: self.is_choice_determinant,
                shot: self.shot,
                policy: self.policy,
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
    pub(in crate::endpoint::kernel) passive_arm_scope: [ScopeId; 2],
    pub(in crate::endpoint::kernel) first_recv_dispatch: FirstRecvDispatchCache,
}

impl ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
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
        (!entry.is_max()).then_some((entry, self.controller_arm_label[arm]))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn recv_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.recv_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_arm_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.passive_arm_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn passive_arm_scope(&self, arm: u8) -> Option<ScopeId> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let scope = self.passive_arm_scope[arm];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_first_recv_dispatch(
        &mut self,
        dispatch: [FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH],
        len: u8,
    ) {
        self.first_recv_dispatch.record(dispatch, len);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_first_recv_dispatch(&self, arm: u8) -> bool {
        self.first_recv_dispatch.arm_has_dispatch(arm)
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn first_recv_dispatch_len(&self) -> u8 {
        self.first_recv_dispatch.len()
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

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CurrentFrontierSelectionState {
    pub(in crate::endpoint::kernel) frontier: FrontierKind,
    pub(in crate::endpoint::kernel) parallel_root: ScopeId,
    pub(in crate::endpoint::kernel) ready: bool,
    pub(in crate::endpoint::kernel) has_progress_evidence: bool,
    pub(in crate::endpoint::kernel) flags: u8,
}

impl CurrentFrontierSelectionState {
    pub(in crate::endpoint::kernel) const FLAG_CONTROLLER: u8 = 1;
    pub(in crate::endpoint::kernel) const FLAG_DYNAMIC: u8 = 1 << 1;

    #[inline]
    pub(in crate::endpoint::kernel) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn parallel(self) -> Option<ScopeId> {
        if self.parallel_root.is_none() {
            None
        } else {
            Some(self.parallel_root)
        }
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn observe_candidate(
        &mut self,
        current_scope: ScopeId,
        current_idx: usize,
        candidate: FrontierCandidate,
    ) {
        if candidate.scope_id == current_scope && candidate.entry_idx as usize == current_idx {
            self.ready = candidate.ready();
            self.has_progress_evidence = candidate.has_evidence();
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn loop_controller_without_evidence(self) -> bool {
        self.frontier == FrontierKind::Loop
            && self.is_controller()
            && self.ready
            && !self.has_progress_evidence
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FrontierStaticFacts {
    pub(in crate::endpoint::kernel) frontier: FrontierKind,
    pub(in crate::endpoint::kernel) ready: bool,
}
