//! Offer frontier metadata and typed state fragments.

use super::super::authority::{RouteDecisionSource, RouteDecisionToken};
use super::super::core::BranchPreviewView;
#[cfg(test)]
use super::super::frontier::FrontierCandidate;
use super::super::frontier::FrontierKind;
use super::super::route_state::RouteArmCommitProof;
use crate::control::cap::mint::CapShot;
use crate::eff::EffIndex;
use crate::global::compiled::images::ControlSemanticKind;
use crate::global::const_dsl::{PolicyMode, ScopeId};
use crate::global::typestate::{
    MAX_FIRST_RECV_DISPATCH, RecvMeta, StateIndex, state_index_to_usize,
};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct LaneIngressEvidence {
    pub(in crate::endpoint::kernel) lane_idx: usize,
    pub(in crate::endpoint::kernel) evidence: crate::binding::IngressEvidence,
}

impl LaneIngressEvidence {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
    ) -> Self {
        Self { lane_idx, evidence }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn frame_label(self) -> u8 {
        self.evidence.frame_label.raw()
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn lane(self) -> u8 {
        self.lane_idx as u8
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn into_parts(
        self,
    ) -> (usize, crate::binding::IngressEvidence) {
        (self.lane_idx, self.evidence)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct OfferScopeSelection {
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) frontier_parallel_root: Option<ScopeId>,
    pub(in crate::endpoint::kernel) offer_lane: u8,
    pub(in crate::endpoint::kernel) offer_lane_idx: u8,
    pub(in crate::endpoint::kernel) at_route_offer_entry: bool,
}

#[derive(Clone, Copy)]
pub(super) enum ResolvePendingState {
    Ready,
    YieldRestart { armed: bool },
    StaticPassiveProgress { selected_arm: u8 },
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
        *self = Self::YieldRestart { armed: false };
    }

    #[inline]
    pub(super) fn arm_static_passive_progress(&mut self, selected_arm: u8) {
        *self = Self::StaticPassiveProgress { selected_arm };
    }

    #[inline]
    pub(super) fn complete_yield_turn(&mut self) {
        *self = Self::YieldRestart { armed: true };
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchCommitPlan {
    pub(in crate::endpoint::kernel) preview: BranchPreviewView,
    pub(in crate::endpoint::kernel) meta: Option<RecvMeta>,
    pub(in crate::endpoint::kernel) route_arm_proof: Option<RouteArmCommitProof>,
    pub(in crate::endpoint::kernel) clear_other_lanes: bool,
}

impl BranchCommitPlan {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn meta(&self) -> Option<RecvMeta> {
        self.meta
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn route_arm_proof(&self) -> Option<RouteArmCommitProof> {
        self.route_arm_proof
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
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) arm_count: u8,
    pub(in crate::endpoint::kernel) controller_arm_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) controller_arm_label: [u8; 2],
    pub(in crate::endpoint::kernel) controller_cross_role_recv_mask: u8,
    pub(in crate::endpoint::kernel) recv_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) passive_arm_entry: [StateIndex; 2],
    pub(in crate::endpoint::kernel) passive_arm_scope: [ScopeId; 2],
    pub(in crate::endpoint::kernel) first_recv_dispatch:
        [(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(in crate::endpoint::kernel) first_recv_len: u8,
    pub(in crate::endpoint::kernel) first_recv_frame_label_mask: crate::transport::FrameLabelMask,
    pub(in crate::endpoint::kernel) first_recv_dispatch_arm_mask: u8,
}

impl ScopeArmMaterializationMeta {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
        first_recv_dispatch: [(0, 0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
        first_recv_frame_label_mask: crate::transport::FrameLabelMask::EMPTY,
        first_recv_dispatch_arm_mask: 0,
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
    pub(in crate::endpoint::kernel) fn first_recv_target_for_lane_frame_label(
        &self,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (entry_frame_label, entry_lane, arm, target) = self.first_recv_dispatch[idx];
            if entry_frame_label == frame_label && entry_lane == lane && !target.is_max() {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_first_recv_dispatch(&self, arm: u8) -> bool {
        arm < 2 && (self.first_recv_dispatch_arm_mask & (1u8 << arm)) != 0
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
pub(in crate::endpoint::kernel) struct ResolvedRouteDecision {
    pub(in crate::endpoint::kernel) route_token: RouteDecisionToken,
    pub(in crate::endpoint::kernel) selected_arm: u8,
    pub(in crate::endpoint::kernel) resolved_hint_frame_label: Option<u8>,
    pub(in crate::endpoint::kernel) poll_route_decision_authority: bool,
}

pub(in crate::endpoint::kernel) enum ResolveTokenOutcome {
    RestartFrontier,
    Resolved(ResolvedRouteDecision),
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

/// Branch metadata carried from `offer()` to `decode()`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BranchMeta {
    /// The scope this branch belongs to.
    pub(crate) scope_id: ScopeId,
    /// The selected arm (0, 1, ...).
    pub(crate) selected_arm: u8,
    /// Wire lane for this branch.
    pub(crate) lane_wire: u8,
    /// EffIndex for lane cursor advancement.
    pub(crate) eff_index: EffIndex,
    /// Transport/binding discriminator expected for this branch.
    pub(crate) frame_label: u8,
    /// Branch dispatch category for decode() dispatch.
    pub(crate) kind: BranchKind,
    /// Route decision source used when commit emits route-decision events.
    pub(in crate::endpoint::kernel) route_source: RouteDecisionSource,
    /// True only when a `Poll` source came from a route-decision frame.
    /// Passive payload/frame-label evidence is demux evidence, not authority.
    pub(in crate::endpoint::kernel) poll_route_decision_authority: bool,
}

/// Branch type taxonomy for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should continue on the same borrowed endpoint with `flow().send()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}
