//! Decode-path helpers for `RouteBranch`.

use core::task::Poll;

use super::route_state::RouteState;
use super::{
    core::{
        BranchPreviewView, CursorEndpoint, DecodeRuntimeDesc, MaterializedRouteBranch,
        is_linger_route_from_cursor, preflight_route_arm_commit_from_parts,
        scope_slot_for_route_from_cursor,
    },
    inbox::PackedIngressEvidence,
    lane_port,
    offer::BranchCommitPlan,
    route_state::{RouteArmCommitProof, RouteCommitProofList},
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::{
        const_dsl::ScopeKind,
        typestate::{
            ARM_SHARED, JumpReason, LoopMetadata, LoopRole, PhaseCursor, RecvMeta, StateIndex,
        },
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::Payload},
};

#[derive(Clone, Copy)]
struct LoopAckPlan {
    lane_idx: usize,
    idx: u8,
    has_local_decision: bool,
}

#[derive(Clone, Copy)]
struct EndpointRxAuditPlan {
    lane: u8,
    label: u8,
}

#[derive(Clone, Copy)]
enum DecodeCommittedPayload<'r> {
    Staged(super::core::StagedPayload<'r>),
    Synthetic(Payload<'r>),
}

impl<'r> DecodeCommittedPayload<'r> {
    #[inline]
    fn payload(self) -> Payload<'r> {
        match self {
            Self::Staged(payload) => payload.payload(),
            Self::Synthetic(payload) => payload,
        }
    }
}

#[derive(Clone, Copy)]
enum DecodeProgressPlan {
    Wire {
        meta: RecvMeta,
        next_index: StateIndex,
        branch_scope: crate::global::const_dsl::ScopeId,
        branch_selected_arm: u8,
        branch_lane: u8,
    },
    Branch {
        scope: crate::global::const_dsl::ScopeId,
        lane: u8,
        selected_arm: u8,
        progress_eff: crate::eff::EffIndex,
        next_index: StateIndex,
        extra_linger_eff: Option<crate::eff::EffIndex>,
        align_to_lane_progress: bool,
    },
    Empty {
        scope: crate::global::const_dsl::ScopeId,
        lane: u8,
        selected_arm: u8,
        progress_eff: crate::eff::EffIndex,
        next_index: StateIndex,
    },
}

#[derive(Clone, Copy)]
enum DecodeLingerCursorPlan {
    None,
    SetLaneToEff { lane: u8, eff: crate::eff::EffIndex },
}

struct DecodeCommitPlan<'txn, 'r> {
    branch: BranchCommitPlan,
    loop_ack: Option<LoopAckPlan>,
    audit: EndpointRxAuditPlan,
    route_arm_proofs: RouteCommitProofList<'txn>,
    progress: DecodeProgressPlan,
    linger_cursor: DecodeLingerCursorPlan,
    committed_payload: DecodeCommittedPayload<'r>,
}

struct DecodePublishPlan<'r> {
    branch: BranchCommitPlan,
    loop_ack: Option<LoopAckPlan>,
    audit: EndpointRxAuditPlan,
    progress: DecodeProgressPlan,
    linger_cursor: DecodeLingerCursorPlan,
    committed_payload: DecodeCommittedPayload<'r>,
}

struct DecodeCommitTxn<'txn, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    cursor: &'txn PhaseCursor,
    route_state: &'txn mut RouteState,
    route_arm_proofs: Option<RouteCommitProofList<'txn>>,
    _role: core::marker::PhantomData<(&'r T, U, C, E, Mint, B)>,
}

pub(crate) struct DecodeState<'r> {
    pub(crate) branch: Option<MaterializedRouteBranch<'r>>,
    prepared_meta: Option<crate::global::typestate::RecvMeta>,
    pending_recv: lane_port::PendingRecv,
    pub(crate) restore_on_drop: bool,
}

impl<'r> DecodeState<'r> {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            branch: None,
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: false,
        }
    }

    #[inline]
    pub(crate) fn new(branch: MaterializedRouteBranch<'r>) -> Self {
        Self {
            branch: Some(branch),
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: true,
        }
    }

    #[inline]
    pub(crate) fn branch(&self) -> Option<&MaterializedRouteBranch<'r>> {
        self.branch.as_ref()
    }

    #[inline]
    pub(crate) fn branch_mut(&mut self) -> Option<&mut MaterializedRouteBranch<'r>> {
        self.branch.as_mut()
    }

    #[inline]
    pub(crate) fn take_branch(&mut self) -> Option<MaterializedRouteBranch<'r>> {
        self.branch.take()
    }

    #[inline]
    pub(crate) fn prepared_meta(&self) -> Option<crate::global::typestate::RecvMeta> {
        self.prepared_meta
    }

    #[inline]
    pub(crate) fn set_prepared_meta(
        &mut self,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
    ) {
        self.prepared_meta = prepared_meta;
    }

    #[inline]
    pub(crate) fn pending_recv_mut(&mut self) -> &mut lane_port::PendingRecv {
        &mut self.pending_recv
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[cfg(test)]
    pub(crate) fn poll_decode_state(
        &mut self,
        desc: DecodeRuntimeDesc,
        state: &mut DecodeState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        super::core::kernel_decode(self, desc, state, cx)
    }

    fn prepare_decode_transport_wait(
        &mut self,
        branch: &MaterializedRouteBranch<'r>,
        desc: DecodeRuntimeDesc,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        let expected = desc.label();
        if branch.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.label,
            });
        }
        if !matches!(branch.branch_meta.kind, super::offer::BranchKind::WireRecv)
            || branch.binding_evidence.is_present()
            || branch.staged_payload.is_some()
        {
            return Ok(None);
        }
        let meta = self
            .cursor
            .try_recv_meta()
            .ok_or_else(decode_phase_invariant)?;
        if meta.is_control != desc.expects_control() {
            return Err(decode_phase_invariant());
        }
        let _ = self.preflight_decode_loop_ack(meta)?;
        Ok(Some(meta))
    }

    fn preflight_decode_loop_ack(&self, meta: RecvMeta) -> RecvResult<Option<LoopAckPlan>> {
        if !self.control_semantic_kind(meta.semantic).is_loop() {
            return Ok(None);
        }
        let Some(LoopMetadata {
            scope: scope_id,
            controller,
            target,
            role,
            ..
        }) = self.cursor.loop_metadata_inner()
        else {
            return Ok(None);
        };
        if role != LoopRole::Target || target != ROLE {
            return Err(decode_phase_invariant());
        }

        if meta.peer != controller {
            return Err(RecvError::PeerMismatch {
                expected: controller,
                actual: meta.peer,
            });
        }

        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(decode_phase_invariant());
        }
        let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
            .ok_or_else(decode_phase_invariant)?;
        let port = self.port_for_lane(lane_idx);
        let lane = port.lane();
        Ok(Some(LoopAckPlan {
            lane_idx,
            idx,
            has_local_decision: port.loop_table().has_decision(lane, idx),
        }))
    }

    fn publish_decode_loop_ack(&self, plan: LoopAckPlan) {
        let port = self.port_for_lane(plan.lane_idx);
        let lane = port.lane();
        port.loop_table().acknowledge(lane, ROLE, plan.idx);
        if plan.has_local_decision {
            port.ack_loop_decision(plan.idx, ROLE);
        }
    }

    fn synthetic_branch_payload(
        &mut self,
        lane_idx: u8,
        desc: DecodeRuntimeDesc,
    ) -> RecvResult<Payload<'r>> {
        let scratch_ptr = {
            let port = self.port_for_lane(lane_idx as usize);
            lane_port::scratch_ptr(port)
        };
        let payload = {
            let scratch = unsafe { &mut *scratch_ptr };
            desc.synthetic_payload(scratch).map_err(RecvError::Codec)?
        };
        Ok(lane_port::shrink_payload(payload))
    }

    fn finish_route_branch_decode(
        &mut self,
        desc: DecodeRuntimeDesc,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        let label = branch.label;
        let binding_evidence = branch.binding_evidence.into_option();
        let binding_evidence_lane = branch.binding_evidence_lane;
        let branch_meta = branch.branch_meta;

        let expected = desc.label();
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }
        if let Some(evidence) = binding_evidence
            && evidence.label != label
        {
            return Err(RecvError::LabelMismatch {
                expected: label,
                actual: evidence.label,
            });
        }
        if binding_evidence.is_some() && binding_evidence_lane != branch_meta.lane_wire {
            return Err(decode_phase_invariant());
        }

        match branch_meta.kind {
            super::offer::BranchKind::LocalControl
            | super::offer::BranchKind::EmptyArmTerminal
            | super::offer::BranchKind::ArmSendHint => {
                let payload = self.synthetic_branch_payload(branch_meta.lane_wire, desc)?;
                desc.validate_payload(payload).map_err(RecvError::Codec)?;
                let branch_view = BranchPreviewView::from_materialized(branch);
                let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
                let audit = self.build_endpoint_rx_audit_plan(branch_view);
                let progress =
                    self.build_synthetic_decode_progress(branch_view, branch_meta.kind)?;
                let publish_plan = self.with_decode_commit_txn(|mut txn| {
                    let plan = txn.build_synthetic_decode_commit_plan(
                        branch_plan,
                        audit,
                        progress,
                        payload,
                    )?;
                    Ok(txn.publish_decode_commit_plan(plan))
                })?;
                let committed_payload = self.publish_decode_commit_plan(publish_plan);
                branch.staged_payload = None;
                branch.binding_evidence = PackedIngressEvidence::EMPTY;
                branch.binding_evidence_lane = u8::MAX;
                return Ok(committed_payload.payload());
            }

            super::offer::BranchKind::WireRecv => {}
        }

        let meta = if let Some(meta) = prepared_meta {
            meta
        } else if let Some(meta) = self.cursor.try_recv_meta() {
            meta
        } else {
            return Err(decode_phase_invariant());
        };
        if meta.is_control != desc.expects_control() {
            return Err(decode_phase_invariant());
        }

        let loop_ack_plan = self.preflight_decode_loop_ack(meta)?;

        let mut staged_payload = branch.staged_payload;
        if staged_payload.is_none()
            && let Some(evidence) = binding_evidence
        {
            let evidence_lane = binding_evidence_lane as usize;
            if evidence_lane >= self.ports.len() || binding_evidence_lane != meta.lane {
                return Err(decode_phase_invariant());
            }
            let scratch_ptr = {
                let port = self.ports[evidence_lane]
                    .as_ref()
                    .ok_or_else(decode_phase_invariant)?;
                lane_port::scratch_ptr(port)
            };
            let payload = lane_port::recv_from_binding(
                core::ptr::from_mut(&mut self.binding),
                evidence.channel,
                scratch_ptr,
            )
            .map_err(|_| decode_phase_invariant())?;
            staged_payload = Some(super::core::StagedPayload::Binding {
                lane: binding_evidence_lane,
                payload,
            });
        } else if staged_payload.is_none() {
            return Err(decode_phase_invariant());
        }

        let staged_payload = staged_payload.ok_or_else(decode_phase_invariant)?;
        if matches!(
            staged_payload,
            super::core::StagedPayload::Binding { lane, .. } if lane != meta.lane
        ) {
            branch.binding_evidence = PackedIngressEvidence::from_option(binding_evidence);
            branch.binding_evidence_lane = binding_evidence_lane;
            branch.staged_payload = Some(staged_payload);
            return Err(decode_phase_invariant());
        }
        let committed_payload = staged_payload;
        let payload = committed_payload.payload();
        if let Err(err) = desc.validate_payload(lane_port::shrink_payload(payload)) {
            branch.binding_evidence = PackedIngressEvidence::from_option(binding_evidence);
            branch.binding_evidence_lane = binding_evidence_lane;
            branch.staged_payload = Some(committed_payload);
            return Err(RecvError::Codec(err));
        }

        let branch_view = BranchPreviewView::from_materialized(branch);

        branch.binding_evidence = PackedIngressEvidence::from_option(binding_evidence);
        branch.binding_evidence_lane = binding_evidence_lane;
        branch.staged_payload = Some(committed_payload);
        let next_index = self
            .cursor
            .try_next_index_past_jumps()
            .map_err(|_| RecvError::PhaseInvariant)?;
        let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
        let branch_meta = branch_plan.meta().ok_or_else(decode_phase_invariant)?;
        let branch_route_proof = branch_plan.route_arm_proof();
        let audit = self.build_endpoint_rx_audit_plan(branch_view);
        let publish_plan = self.with_decode_commit_txn(|mut txn| {
            let plan = txn.build_decode_commit_plan(
                branch_plan,
                branch_route_proof,
                branch_view,
                meta,
                label,
                next_index,
                branch_meta,
                loop_ack_plan,
                audit,
                DecodeCommittedPayload::Staged(committed_payload),
            )?;
            Ok(txn.publish_decode_commit_plan(plan))
        })?;
        let committed_payload = self.publish_decode_commit_plan(publish_plan);
        branch.staged_payload = None;
        branch.binding_evidence = PackedIngressEvidence::EMPTY;
        branch.binding_evidence_lane = u8::MAX;
        let payload = committed_payload.payload();
        Ok(payload)
    }

    fn with_decode_commit_txn<R>(
        &mut self,
        f: impl for<'txn> FnOnce(
            DecodeCommitTxn<'txn, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        ) -> RecvResult<R>,
    ) -> RecvResult<R> {
        let required = self.route_scope_depth_bound();
        let Self {
            cursor,
            route_state,
            route_commit_proofs,
            ..
        } = self;
        let route_arm_proofs = route_commit_proofs.begin(required)?;
        f(DecodeCommitTxn {
            cursor,
            route_state,
            route_arm_proofs: Some(route_arm_proofs),
            _role: core::marker::PhantomData,
        })
    }

    fn build_synthetic_decode_progress(
        &self,
        branch: BranchPreviewView,
        kind: super::offer::BranchKind,
    ) -> RecvResult<DecodeProgressPlan> {
        let branch_meta = branch.branch_meta;
        match kind {
            super::offer::BranchKind::LocalControl | super::offer::BranchKind::ArmSendHint => {
                let next_index = self
                    .cursor
                    .try_next_index_past_jumps()
                    .map_err(|_| RecvError::PhaseInvariant)?;
                let progress_eff = self
                    .cursor
                    .scope_lane_last_eff_for_arm(
                        branch_meta.scope_id,
                        branch_meta.selected_arm,
                        branch_meta.lane_wire,
                    )
                    .or_else(|| {
                        self.cursor
                            .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                    })
                    .unwrap_or(branch_meta.eff_index);
                let extra_linger_eff = if branch_meta.selected_arm > 0
                    && self
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                {
                    self.cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                } else {
                    None
                };
                Ok(DecodeProgressPlan::Branch {
                    scope: branch_meta.scope_id,
                    lane: branch_meta.lane_wire,
                    selected_arm: branch_meta.selected_arm,
                    progress_eff,
                    next_index,
                    extra_linger_eff,
                    align_to_lane_progress: true,
                })
            }
            super::offer::BranchKind::EmptyArmTerminal => {
                let next_index = self
                    .cursor
                    .try_follow_jumps_from_index(StateIndex::from_usize(self.cursor.index()))
                    .map_err(|_| RecvError::PhaseInvariant)?;
                let progress_eff = self
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                    .unwrap_or(branch_meta.eff_index);
                Ok(DecodeProgressPlan::Empty {
                    scope: branch_meta.scope_id,
                    lane: branch_meta.lane_wire,
                    selected_arm: branch_meta.selected_arm,
                    progress_eff,
                    next_index,
                })
            }
            super::offer::BranchKind::WireRecv => Err(decode_phase_invariant()),
        }
    }

    fn collect_decode_linger_route_arm_proofs_from_parts(
        cursor: &PhaseCursor,
        route_state: &RouteState,
        branch_route_proof: Option<super::route_state::RouteArmCommitProof>,
        meta: RecvMeta,
        label: u8,
        branch_scope: crate::global::const_dsl::ScopeId,
        plan: &mut RouteCommitProofList,
    ) -> RecvResult<()> {
        let mut linger_scope = meta.scope;
        let mut depth = 0usize;
        let depth_bound = cursor.route_scope_count().saturating_add(1);
        while depth < depth_bound {
            if linger_scope != branch_scope
                && linger_scope.kind() == ScopeKind::Route
                && is_linger_route_from_cursor(cursor, linger_scope)
                && Self::route_arm_for_from_parts(route_state, cursor, meta.lane, linger_scope)
                    .is_none()
                && branch_route_proof
                    .map(|proof| proof.scope() == linger_scope)
                    .unwrap_or(false)
                    == false
                && plan.arm_for_scope(linger_scope).is_none()
            {
                let selected = Self::static_poll_route_arm_for_label(cursor, linger_scope, label)
                    .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm })
                    .ok_or_else(decode_phase_invariant)?;
                let proof = preflight_route_arm_commit_from_parts(
                    route_state,
                    cursor,
                    meta.lane,
                    linger_scope,
                    selected,
                )
                .ok_or_else(decode_phase_invariant)?;
                plan.push_unique(proof)?;
            }
            let Some(parent) = cursor.scope_parent(linger_scope) else {
                return Ok(());
            };
            linger_scope = parent;
            depth += 1;
        }
        Err(decode_phase_invariant())
    }

    #[inline]
    fn static_poll_route_arm_for_label(
        cursor: &PhaseCursor,
        scope: crate::global::const_dsl::ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        cursor.first_recv_target(scope, label)
    }

    fn route_arm_for_from_parts(
        route_state: &RouteState,
        cursor: &PhaseCursor,
        lane: u8,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= cursor.logical_lane_count() {
            return None;
        }
        if let Some(scope_slot) = scope_slot_for_route_from_cursor(cursor, scope) {
            if let Some(arm) = route_state.selected_arm_for_scope_slot(scope_slot) {
                return Some(arm);
            }
        }
        route_state.route_arm_for(lane_idx, scope)
    }

    fn authorized_route_arm_for_decode(
        route_state: &RouteState,
        cursor: &PhaseCursor,
        branch_route_proof: Option<super::route_state::RouteArmCommitProof>,
        proofs: &RouteCommitProofList,
        lane: u8,
        scope: crate::global::const_dsl::ScopeId,
        label: u8,
    ) -> Option<u8> {
        if let Some(arm) = proofs.arm_for_scope(scope) {
            return Some(arm);
        }
        if let Some(proof) = branch_route_proof
            && proof.scope() == scope
        {
            return Some(proof.arm());
        }
        Self::route_arm_for_from_parts(route_state, cursor, lane, scope).or_else(|| {
            Self::static_poll_route_arm_for_label(cursor, scope, label)
                .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm })
        })
    }

    fn scope_region_for_index(
        cursor: &PhaseCursor,
        idx: StateIndex,
    ) -> Option<crate::global::typestate::ScopeRegion> {
        let index = crate::global::typestate::state_index_to_usize(idx);
        let scope = cursor.typestate_node(index).scope();
        if scope.is_none() {
            None
        } else {
            cursor.scope_region_by_id(scope)
        }
    }

    fn build_decode_linger_cursor_plan_from_parts(
        cursor: &PhaseCursor,
        route_state: &RouteState,
        branch_route_proof: Option<super::route_state::RouteArmCommitProof>,
        proofs: &RouteCommitProofList,
        meta: RecvMeta,
        label: u8,
        next_index: StateIndex,
    ) -> DecodeLingerCursorPlan {
        let mut linger_scope = meta.scope;
        loop {
            if is_linger_route_from_cursor(cursor, linger_scope)
                && let Some(arm) = Self::authorized_route_arm_for_decode(
                    route_state,
                    cursor,
                    branch_route_proof,
                    proofs,
                    meta.lane,
                    linger_scope,
                    label,
                )
                && arm == 0
                && let Some(last_eff) =
                    cursor.scope_lane_last_eff_for_arm(linger_scope, arm, meta.lane)
                && last_eff == meta.eff_index
                && let Some(first_eff) = cursor.scope_lane_first_eff(linger_scope, meta.lane)
            {
                return DecodeLingerCursorPlan::SetLaneToEff {
                    lane: meta.lane,
                    eff: first_eff,
                };
            }
            let Some(parent) = cursor.scope_parent(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }

        if let Some(region) = Self::scope_region_for_index(cursor, next_index)
            && region.kind == ScopeKind::Route
            && region.linger
        {
            let next_usize = crate::global::typestate::state_index_to_usize(next_index);
            let at_scope_start = next_usize == region.start;
            let at_passive_branch = cursor.jump_reason_at(next_usize)
                == Some(JumpReason::PassiveObserverBranch)
                && cursor.typestate_node(next_usize).scope() == region.scope_id;
            if (at_scope_start || at_passive_branch)
                && let Some(arm) = Self::authorized_route_arm_for_decode(
                    route_state,
                    cursor,
                    branch_route_proof,
                    proofs,
                    meta.lane,
                    region.scope_id,
                    label,
                )
                && arm == 0
                && let Some(first_eff) = cursor.scope_lane_first_eff(region.scope_id, meta.lane)
            {
                return DecodeLingerCursorPlan::SetLaneToEff {
                    lane: meta.lane,
                    eff: first_eff,
                };
            }
        }
        DecodeLingerCursorPlan::None
    }

    fn publish_decode_linger_cursor_plan(&mut self, plan: DecodeLingerCursorPlan) {
        match plan {
            DecodeLingerCursorPlan::None => {}
            DecodeLingerCursorPlan::SetLaneToEff { lane, eff } => {
                self.set_lane_cursor_to_eff_index(lane as usize, eff);
            }
        }
    }

    fn build_endpoint_rx_audit_plan(&self, branch: BranchPreviewView) -> EndpointRxAuditPlan {
        EndpointRxAuditPlan {
            lane: branch.branch_meta.lane_wire,
            label: branch.label,
        }
    }

    fn publish_endpoint_rx_audit(&self, plan: EndpointRxAuditPlan) {
        let lane = crate::control::types::Lane::new(plan.lane as u32);
        self.emit_endpoint_policy_audit(
            crate::policy_runtime::PolicySlot::EndpointRx,
            crate::observe::ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_policy_args(
                lane,
                plan.label,
                crate::transport::wire::FrameFlags::empty(),
            ),
            lane,
        );
    }
}

impl<'txn, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    DecodeCommitTxn<'txn, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    fn build_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        branch_route_proof: Option<RouteArmCommitProof>,
        branch: BranchPreviewView,
        meta: RecvMeta,
        label: u8,
        next_index: StateIndex,
        branch_meta: RecvMeta,
        loop_ack: Option<LoopAckPlan>,
        audit: EndpointRxAuditPlan,
        committed_payload: DecodeCommittedPayload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'txn, 'r>> {
        let mut route_arm_proofs = self
            .route_arm_proofs
            .take()
            .ok_or_else(decode_phase_invariant)?;
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::collect_decode_linger_route_arm_proofs_from_parts(
            self.cursor,
            self.route_state,
            branch_route_proof,
            meta,
            label,
            branch.branch_meta.scope_id,
            &mut route_arm_proofs,
        )?;
        let linger_cursor = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::build_decode_linger_cursor_plan_from_parts(
            self.cursor,
            self.route_state,
            branch_route_proof,
            &route_arm_proofs,
            meta,
            label,
            next_index,
        );
        Ok(DecodeCommitPlan {
            branch: branch_plan,
            loop_ack,
            audit,
            route_arm_proofs,
            progress: DecodeProgressPlan::Wire {
                meta: branch_meta,
                next_index,
                branch_scope: branch.branch_meta.scope_id,
                branch_selected_arm: branch.branch_meta.selected_arm,
                branch_lane: branch.branch_meta.lane_wire,
            },
            linger_cursor,
            committed_payload,
        })
    }

    fn build_synthetic_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        audit: EndpointRxAuditPlan,
        progress: DecodeProgressPlan,
        payload: Payload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'txn, 'r>> {
        let route_arm_proofs = self
            .route_arm_proofs
            .take()
            .ok_or_else(decode_phase_invariant)?;
        Ok(DecodeCommitPlan {
            branch: branch_plan,
            loop_ack: None,
            audit,
            route_arm_proofs,
            progress,
            linger_cursor: DecodeLingerCursorPlan::None,
            committed_payload: DecodeCommittedPayload::Synthetic(payload),
        })
    }

    fn publish_decode_commit_plan(self, plan: DecodeCommitPlan<'txn, 'r>) -> DecodePublishPlan<'r> {
        for proof in plan.route_arm_proofs.iter() {
            self.route_state.commit_route_arm_after_preflight(proof);
        }
        DecodePublishPlan {
            branch: plan.branch,
            loop_ack: plan.loop_ack,
            audit: plan.audit,
            progress: plan.progress,
            linger_cursor: plan.linger_cursor,
            committed_payload: plan.committed_payload,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    fn publish_decode_commit_plan(
        &mut self,
        plan: DecodePublishPlan<'r>,
    ) -> DecodeCommittedPayload<'r> {
        let _published_meta = self.publish_branch_preview_commit_plan(plan.branch);
        if let Some(loop_ack) = plan.loop_ack {
            self.publish_decode_loop_ack(loop_ack);
        }
        self.publish_endpoint_rx_audit(plan.audit);
        match plan.progress {
            DecodeProgressPlan::Wire {
                meta,
                next_index,
                branch_scope,
                branch_selected_arm,
                branch_lane,
            } => {
                self.set_cursor_index(next_index.as_usize());
                let decode_lane_idx = meta.lane as usize;
                self.advance_lane_cursor(decode_lane_idx, meta.eff_index);
                self.maybe_skip_remaining_route_arm(
                    meta.scope,
                    meta.lane,
                    meta.route_arm,
                    meta.eff_index,
                );
                self.publish_scope_settlement(
                    meta.scope,
                    meta.route_arm,
                    Some(meta.eff_index),
                    meta.lane,
                );
                if branch_scope != meta.scope {
                    self.publish_scope_settlement(
                        branch_scope,
                        Some(branch_selected_arm),
                        Some(meta.eff_index),
                        branch_lane,
                    );
                }
                self.publish_decode_linger_cursor_plan(plan.linger_cursor);
            }
            DecodeProgressPlan::Branch {
                scope,
                lane,
                selected_arm,
                progress_eff,
                next_index,
                extra_linger_eff,
                align_to_lane_progress,
            } => {
                let lane_idx = lane as usize;
                self.advance_lane_cursor(lane_idx, progress_eff);
                if let Some(scope_last_eff) = extra_linger_eff {
                    self.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if align_to_lane_progress && !self.align_cursor_to_lane_progress(lane_idx) {
                    self.set_cursor_index(next_index.as_usize());
                }
                self.maybe_skip_remaining_route_arm(scope, lane, Some(selected_arm), progress_eff);
                self.publish_scope_settlement(scope, Some(selected_arm), None, lane);
            }
            DecodeProgressPlan::Empty {
                scope,
                lane,
                selected_arm,
                progress_eff,
                next_index,
            } => {
                self.set_cursor_index(next_index.as_usize());
                self.advance_lane_cursor(lane as usize, progress_eff);
                self.publish_scope_settlement(scope, Some(selected_arm), None, lane);
            }
        }
        self.maybe_advance_phase();
        plan.committed_payload
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    super::core::DecodeKernelEndpoint<'r> for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    fn prepare_decode_kernel_transport_wait(
        &mut self,
        desc: DecodeRuntimeDesc,
        branch: &MaterializedRouteBranch<'r>,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        self.prepare_decode_transport_wait(branch, desc)
    }

    #[inline]
    fn poll_decode_kernel_transport_payload(
        &mut self,
        meta: crate::global::typestate::RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        let port = self.port_for_lane(meta.lane as usize);
        match lane_port::poll_recv(pending_recv, port, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => Poll::Ready(Ok(payload)),
            Poll::Ready(Err(err)) => Poll::Ready(Err(RecvError::Transport(err))),
        }
    }

    #[inline]
    fn finish_decode_kernel(
        &mut self,
        desc: DecodeRuntimeDesc,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_route_branch_decode(desc, prepared_meta, branch)
    }
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
