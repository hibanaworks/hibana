//! Offer-path helpers for scope selection and branch materialization.

use super::authority::RouteDecisionToken;
use super::core::{CursorEndpoint, RouteBranch};
use super::evidence::{ScopeLabelMeta, ScopeLoopMeta};
#[cfg(test)]
use super::frontier::FrontierCandidate;
use super::frontier::{FrontierKind, FrontierVisitSet};
use super::lane_port;
use crate::binding::BindingSlot;
use crate::control::cap::mint::{CapShot, EpochTable, MintConfigMarker};
use crate::eff::EffIndex;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{PolicyMode, ScopeId};
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{
    ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RecvMeta, StateIndex, state_index_to_usize,
};
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::Transport;

#[derive(Clone, Copy)]
pub(super) struct OfferScopeSelection {
    pub(super) scope_id: ScopeId,
    pub(super) frontier_parallel_root: Option<ScopeId>,
    pub(super) offer_lanes: [u8; MAX_LANES],
    pub(super) offer_lane_mask: u8,
    pub(super) offer_lanes_len: usize,
    pub(super) offer_lane: u8,
    pub(super) offer_lane_idx: usize,
    pub(super) label_meta: ScopeLabelMeta,
    pub(super) materialization_meta: ScopeArmMaterializationMeta,
    pub(super) passive_recv_meta: [CachedRecvMeta; 2],
    pub(super) at_route_offer_entry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CachedRecvMeta {
    pub(super) cursor_index: StateIndex,
    pub(super) eff_index: EffIndex,
    pub(super) peer: u8,
    pub(super) label: u8,
    pub(super) resource: Option<u8>,
    pub(super) is_control: bool,
    pub(super) next: StateIndex,
    pub(super) scope: ScopeId,
    pub(super) route_arm: u8,
    pub(super) is_choice_determinant: bool,
    pub(super) shot: Option<CapShot>,
    pub(super) policy: PolicyMode,
    pub(super) lane: u8,
    pub(super) flags: u8,
}

impl CachedRecvMeta {
    pub(super) const FLAG_RECV_STEP: u8 = 1;

    pub(super) const EMPTY: Self = Self {
        cursor_index: StateIndex::MAX,
        eff_index: EffIndex::ZERO,
        peer: 0,
        label: 0,
        resource: None,
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
    pub(super) fn recv_meta(self) -> Option<(usize, RecvMeta)> {
        if self.cursor_index.is_max() || self.next.is_max() {
            return None;
        }
        Some((
            state_index_to_usize(self.cursor_index),
            RecvMeta {
                eff_index: self.eff_index,
                peer: self.peer,
                label: self.label,
                resource: self.resource,
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
    pub(super) fn is_recv_step(self) -> bool {
        (self.flags & Self::FLAG_RECV_STEP) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeArmMaterializationMeta {
    pub(super) arm_count: u8,
    pub(super) controller_arm_entry: [StateIndex; 2],
    pub(super) controller_arm_label: [u8; 2],
    pub(super) controller_recv_mask: u8,
    pub(super) controller_cross_role_recv_mask: u8,
    pub(super) recv_entry: [StateIndex; 2],
    pub(super) passive_arm_entry: [StateIndex; 2],
    pub(super) passive_arm_scope: [ScopeId; 2],
    pub(super) binding_demux_lane_mask: [u8; 2],
    pub(super) first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(super) first_recv_len: u8,
}

impl ScopeArmMaterializationMeta {
    pub(super) const EMPTY: Self = Self {
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_recv_mask: 0,
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
        binding_demux_lane_mask: [0; 2],
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
    };

    #[inline]
    pub(super) fn controller_arm_entry(self, arm: u8) -> Option<(StateIndex, u8)> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.controller_arm_entry[arm];
        (!entry.is_max()).then_some((entry, self.controller_arm_label[arm]))
    }

    #[inline]
    pub(super) fn recv_entry(self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.recv_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(super) fn passive_arm_entry(self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.passive_arm_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(super) fn passive_arm_scope(self, arm: u8) -> Option<ScopeId> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let scope = self.passive_arm_scope[arm];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    pub(super) fn record_binding_demux_lane(&mut self, arm: u8, lane: u8) {
        let bit = 1u8 << (lane as usize);
        if arm == ARM_SHARED {
            self.binding_demux_lane_mask[0] |= bit;
            self.binding_demux_lane_mask[1] |= bit;
            return;
        }
        let arm = arm as usize;
        if arm < self.binding_demux_lane_mask.len() {
            self.binding_demux_lane_mask[arm] |= bit;
        }
    }

    #[inline]
    pub(super) fn binding_demux_lane_mask(self, preferred_arm: Option<u8>) -> u8 {
        preferred_arm
            .and_then(|arm| self.binding_demux_lane_mask.get(arm as usize).copied())
            .unwrap_or(self.binding_demux_lane_mask[0] | self.binding_demux_lane_mask[1])
    }

    #[inline]
    pub(super) fn binding_demux_lane_mask_for_label_mask(
        self,
        label_meta: ScopeLabelMeta,
        label_mask: u128,
    ) -> u8 {
        if label_mask == 0 {
            return 0;
        }
        let mut lane_mask = 0u8;
        let mut arm = 0u8;
        while arm <= 1 {
            if (label_meta.binding_demux_label_mask_for_arm(arm) & label_mask) != 0 {
                lane_mask |= self.binding_demux_lane_mask(Some(arm));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if lane_mask != 0 {
            lane_mask
        } else {
            self.binding_demux_lane_mask(None)
        }
    }

    #[inline]
    pub(super) fn first_recv_target(self, label: u8) -> Option<(u8, StateIndex)> {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (entry_label, arm, target) = self.first_recv_dispatch[idx];
            if entry_label == label && !target.is_max() {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn arm_has_first_recv_dispatch(self, arm: u8) -> bool {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (_label, dispatch_arm, target) = self.first_recv_dispatch[idx];
            if !target.is_max() && (dispatch_arm == arm || dispatch_arm == ARM_SHARED) {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(super) fn controller_arm_is_recv(self, arm: u8) -> bool {
        arm < 2 && (self.controller_recv_mask & (1u8 << arm)) != 0
    }

    #[inline]
    pub(super) fn controller_arm_requires_ready_evidence(self, arm: u8) -> bool {
        arm < 2 && (self.controller_cross_role_recv_mask & (1u8 << arm)) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct ResolvedRouteDecision {
    pub(super) route_token: RouteDecisionToken,
    pub(super) selected_arm: u8,
    pub(super) resolved_label_hint: Option<u8>,
}

pub(super) enum ResolveTokenOutcome {
    RestartFrontier,
    Resolved(ResolvedRouteDecision),
}

#[derive(Clone, Copy)]
pub(super) struct CurrentScopeSelectionMeta {
    pub(super) flags: u8,
}

impl CurrentScopeSelectionMeta {
    pub(super) const FLAG_ROUTE_ENTRY: u8 = 1;
    pub(super) const FLAG_HAS_OFFER_LANES: u8 = 1 << 1;
    pub(super) const FLAG_CONTROLLER: u8 = 1 << 2;

    pub(super) const EMPTY: Self = Self { flags: 0 };

    #[inline]
    pub(super) fn is_route_entry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_ENTRY) != 0
    }

    #[inline]
    pub(super) fn has_offer_lanes(self) -> bool {
        !self.is_route_entry() || (self.flags & Self::FLAG_HAS_OFFER_LANES) != 0
    }

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct CurrentFrontierSelectionState {
    pub(super) frontier: FrontierKind,
    pub(super) parallel_root: ScopeId,
    pub(super) ready: bool,
    pub(super) has_progress_evidence: bool,
    pub(super) flags: u8,
}

impl CurrentFrontierSelectionState {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn parallel(self) -> Option<ScopeId> {
        if self.parallel_root.is_none() {
            None
        } else {
            Some(self.parallel_root)
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn observe_candidate(
        &mut self,
        current_scope: ScopeId,
        current_idx: usize,
        candidate: FrontierCandidate,
    ) {
        if candidate.scope_id == current_scope && candidate.entry_idx == current_idx {
            self.ready = candidate.ready;
            self.has_progress_evidence = candidate.has_evidence;
        }
    }

    #[inline]
    pub(super) fn loop_controller_without_evidence(self) -> bool {
        self.frontier == FrontierKind::Loop
            && self.is_controller()
            && self.ready
            && !self.has_progress_evidence
    }
}

#[derive(Clone, Copy)]
pub(super) struct FrontierStaticFacts {
    pub(super) frontier: FrontierKind,
    pub(super) loop_meta: ScopeLoopMeta,
    pub(super) ready: bool,
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
    /// Classification of the branch for decode() dispatch.
    pub(crate) kind: BranchKind,
}

/// Classification of branch types for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: CanonicalControl self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should use `into_endpoint()` and `flow().send()` instead of `decode()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    /// Observe an inbound route branch.
    ///
    /// Route hints are drained once per call and consumed only when they match
    /// the current route scope.
    /// Loop control evidence that resolves a recv-less branch is treated as
    /// EmptyArmTerminal and skip decode.
    pub async fn offer(self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let mut self_endpoint = self;
        let mut frontier_visited = FrontierVisitSet::EMPTY;
        let mut carried_binding_classification = None;
        let mut carried_transport_payload = None;
        'offer_frontier: loop {
            let selection = self_endpoint.select_scope()?;
            let scope_id = selection.scope_id;
            frontier_visited.record(scope_id);
            let offer_lane_mask = selection.offer_lane_mask;
            let offer_lane = selection.offer_lane;
            let offer_lane_idx = selection.offer_lane_idx;
            let label_meta = selection.label_meta;
            let at_route_offer_entry = selection.at_route_offer_entry;

            let cursor_is_not_recv = !self_endpoint.cursor.is_recv();
            let is_route_controller = self_endpoint.cursor.is_route_controller(scope_id);
            let controller_selected_recv_step = is_route_controller
                && !at_route_offer_entry
                && self_endpoint
                    .cursor
                    .try_recv_meta()
                    .map(|recv_meta| recv_meta.peer != ROLE)
                    .unwrap_or(false);
            let loop_meta = label_meta.loop_meta();

            let route_policy_is_dynamic = self_endpoint
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false);
            let is_dynamic_route_scope = route_policy_is_dynamic;
            let suppress_scope_hint = is_dynamic_route_scope;
            self_endpoint.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
                suppress_scope_hint,
                label_meta,
            );
            let preview_route_decision = self_endpoint.preview_scope_ack_token_non_consuming(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
            );
            let preview_ready_arm_evidence = self_endpoint.scope_has_ready_arm_evidence(scope_id);
            let recvless_loop_control_scope = !is_route_controller
                && !is_dynamic_route_scope
                && loop_meta.control_scope()
                && !loop_meta.arm_has_recv(0)
                && !loop_meta.arm_has_recv(1);

            let is_self_send_controller = cursor_is_not_recv
                && is_route_controller
                && !Self::scope_has_controller_arm_entry(&self_endpoint.cursor, scope_id);
            let controller_non_entry_cursor_ready = cursor_is_not_recv
                && is_route_controller
                && self_endpoint.controller_arm_at_cursor(scope_id).is_none();

            let early_route_decision = if is_route_controller {
                preview_route_decision
            } else {
                preview_route_decision
                    .filter(|token| !self_endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
            };

            let early_decision_arm_has_no_recv = early_route_decision
                .map(|token| !self_endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
                .unwrap_or(false);
            let early_hint_resolves_recvless = false;
            let controller_static_entry_ready = false;
            let controller_pending_materialization = is_route_controller
                && self_endpoint
                    .selected_arm_for_scope(scope_id)
                    .map(|arm| {
                        self_endpoint.arm_requires_materialization_ready_evidence(scope_id, arm)
                            && !self_endpoint.scope_has_ready_arm(scope_id, arm)
                    })
                    .unwrap_or(false);
            let controller_can_skip_recv = is_route_controller
                && !controller_pending_materialization
                && ((at_route_offer_entry
                    && (is_dynamic_route_scope
                        || controller_non_entry_cursor_ready
                        || is_self_send_controller
                        || early_route_decision.is_some()
                        || controller_static_entry_ready))
                    || (!at_route_offer_entry && cursor_is_not_recv));
            let passive_dynamic_scope_has_recv =
                self_endpoint.arm_has_recv(scope_id, 0) || self_endpoint.arm_has_recv(scope_id, 1);
            let passive_ack_is_materializable = self_endpoint
                .preview_scope_ack_token_non_consuming(scope_id, offer_lane_idx, offer_lane_mask)
                .map(|token| {
                    let arm = token.arm().as_u8();
                    self_endpoint.scope_has_ready_arm(scope_id, arm)
                        || !self_endpoint.arm_has_recv(scope_id, arm)
                })
                .unwrap_or(false);
            let passive_dynamic_can_skip_recv = !is_route_controller
                && is_dynamic_route_scope
                && (!passive_dynamic_scope_has_recv
                    || preview_ready_arm_evidence
                    || passive_ack_is_materializable);
            let skip_recv_loop = passive_dynamic_can_skip_recv
                || controller_can_skip_recv
                || early_decision_arm_has_no_recv
                || early_hint_resolves_recvless;
            let mut binding_classification = carried_binding_classification.take();
            let (mut transport_payload_len, mut transport_payload_lane) =
                carried_transport_payload.take().unwrap_or((0, offer_lane));
            if binding_classification.is_none() && transport_payload_len == 0 {
                let payload_view = if skip_recv_loop {
                    0usize
                } else {
                    'offer_recv: loop {
                        if !is_route_controller || controller_selected_recv_step {
                            if let Some((_, classification)) = self_endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                selection.materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self_endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                        }

                        let payload_len = {
                            let port = self_endpoint.port_for_lane(offer_lane_idx);
                            let payload = lane_port::recv_future(port)
                                .await
                                .map_err(RecvError::Transport)?;
                            lane_port::copy_payload_into_scratch(port, &payload)
                                .map_err(|_| RecvError::PhaseInvariant)?
                        };

                        if !is_route_controller || controller_selected_recv_step {
                            if let Some((_, classification)) = self_endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                selection.materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self_endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                        }

                        break 'offer_recv payload_len;
                    }
                };
                if payload_view != 0 {
                    transport_payload_len = payload_view;
                    transport_payload_lane = offer_lane;
                }
            }
            if let Some(classification) = binding_classification.as_ref() {
                self_endpoint.ingest_binding_scope_evidence(
                    scope_id,
                    classification.label,
                    suppress_scope_hint,
                    label_meta,
                );
            }
            self_endpoint.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
                suppress_scope_hint,
                label_meta,
            );
            if self_endpoint.scope_evidence_conflicted(scope_id)
                && !self_endpoint.recover_scope_evidence_conflict(
                    scope_id,
                    is_dynamic_route_scope,
                    is_route_controller,
                )
            {
                return Err(RecvError::PhaseInvariant);
            }

            let resolved = match self_endpoint
                .resolve_token(
                    selection,
                    is_route_controller,
                    is_dynamic_route_scope,
                    &mut binding_classification,
                    &mut transport_payload_len,
                    &mut transport_payload_lane,
                    &mut frontier_visited,
                )
                .await?
            {
                ResolveTokenOutcome::RestartFrontier => {
                    carried_binding_classification = binding_classification;
                    carried_transport_payload = (transport_payload_len != 0)
                        .then_some((transport_payload_len, transport_payload_lane));
                    continue 'offer_frontier;
                }
                ResolveTokenOutcome::Resolved(resolved) => resolved,
            };
            if !is_route_controller
                && self_endpoint.descend_selected_passive_route(selection, resolved)?
            {
                carried_binding_classification = binding_classification;
                carried_transport_payload = (transport_payload_len != 0)
                    .then_some((transport_payload_len, transport_payload_lane));
                continue 'offer_frontier;
            }
            return self_endpoint.materialize_branch(
                selection,
                resolved,
                is_route_controller,
                binding_classification,
                transport_payload_len,
                transport_payload_lane,
            );
        }
    }
}
