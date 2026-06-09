//! Decode-path helpers for `RouteBranch`.

mod audit;
mod finish;
mod state;

use core::task::Poll;

pub(crate) use state::DecodeState;

use super::decision_state::RouteState;
use super::{
    core::{
        BranchPreviewView, CommitDelta, CursorEndpoint, DecodeRuntimeDesc, LoopCommitRow,
        MaterializedRouteBranch, PreparedCommitDelta,
        prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range,
        scope_slot_for_route_from_cursor,
    },
    decision_state::{SelectedRouteCommitRows, SelectedRouteCommitRowsRef},
    lane_port,
    offer::{BranchCommitPlan, BranchKind},
};
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::typestate::{
        EventCursor, LoopMetadata, LoopRole, RecvMeta, RelocatableResidentLaneStep, StateIndex,
        state_index_to_usize,
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::Payload},
};

#[derive(Clone, Copy)]
struct EndpointRxAuditPlan {
    lane: u8,
    label: u8,
}

#[derive(Clone, Copy)]
enum DecodeProgressPlan {
    Wire { delta: CommitDelta },
    Branch { delta: CommitDelta },
    Empty { delta: CommitDelta },
}

#[derive(Clone, Copy)]
enum DecodeLingerCursorPlan {
    None,
    SetLane { step: RelocatableResidentLaneStep },
}

struct DecodeCommitPlan<'r> {
    branch: BranchCommitPlan,
    audit: EndpointRxAuditPlan,
    progress: DecodeProgressPlan,
    committed_payload: Payload<'r>,
}

enum PreparedDecodeProgressPlan {
    Wire { delta: PreparedCommitDelta },
    Branch { delta: PreparedCommitDelta },
    Empty { delta: PreparedCommitDelta },
}

struct PreparedDecodePublishPlan<'r> {
    branch: BranchCommitPlan,
    audit: EndpointRxAuditPlan,
    progress: PreparedDecodeProgressPlan,
    committed_payload: Payload<'r>,
}

struct DecodeCommitTxn<'txn, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    cursor: &'txn EventCursor,
    decision_state: &'txn mut RouteState,
    route_rows: Option<SelectedRouteCommitRows>,
    _role: core::marker::PhantomData<(&'r T, U, C, E, Mint)>,
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
