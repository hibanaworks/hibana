//! Decode-path helpers for `RouteBranch`.

mod finish;
mod state;

use core::task::Poll;

pub(crate) use state::DecodeState;

use super::decision_state::RouteState;
use super::{
    core::{
        BranchPreviewView, CursorEndpoint, DecodeRuntimeDesc, MaterializedRouteBranch,
        StagedPayload, is_linger_route_from_cursor, preflight_route_arm_commit_from_parts,
        scope_slot_for_route_from_cursor,
    },
    decision_state::{RouteArmCommitProof, RouteCommitProofList},
    inbox::PackedIngressEvidence,
    lane_port,
    offer::{BranchCommitPlan, BranchKind},
};
use crate::{
    binding::EndpointSlot,
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
enum DecodeProgressPlan {
    Wire {
        meta: RecvMeta,
        next_index: StateIndex,
        branch_scope: crate::global::const_dsl::ScopeId,
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
    committed_payload: Payload<'r>,
}

struct DecodePublishPlan<'r> {
    branch: BranchCommitPlan,
    loop_ack: Option<LoopAckPlan>,
    audit: EndpointRxAuditPlan,
    progress: DecodeProgressPlan,
    linger_cursor: DecodeLingerCursorPlan,
    committed_payload: Payload<'r>,
}

struct DecodeCommitTxn<'txn, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    cursor: &'txn PhaseCursor,
    decision_state: &'txn mut RouteState,
    route_arm_proofs: Option<RouteCommitProofList<'txn>>,
    _role: core::marker::PhantomData<(&'r T, U, C, E, Mint, B)>,
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
