//! Decode-path helpers for `RouteBranch`.

mod audit;
mod finish;
mod state;

use core::task::Poll;

pub(crate) use state::DecodeState;

use super::decision_state::RouteState;
use super::{
    core::{
        BranchPreviewView, CommitDelta, CursorEndpoint, DecodeRuntimeDesc, MaterializedRouteBranch,
        PreparedCommitDelta,
        prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range,
        scope_slot_for_route_from_cursor,
    },
    decision_state::{SelectedRouteCommitRows, SelectedRouteCommitRowsRef},
    lane_port,
    offer::{BranchCommitPlan, BranchKind},
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::{
        EventCursor, RecvMeta, RelocatableResidentLaneStep, StateIndex, state_index_to_usize,
    },
    runtime_core::config::Clock,
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
    NonWire { delta: CommitDelta },
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
    NonWire { delta: PreparedCommitDelta },
}

struct PreparedDecodePublishPlan<'r> {
    branch: BranchCommitPlan,
    audit: EndpointRxAuditPlan,
    progress: PreparedDecodeProgressPlan,
    committed_payload: Payload<'r>,
}

struct DecodeCommitBuilder<'build, 'r, const ROLE: u8, T, C, const MAX_RV: usize>
where
    T: Transport + 'r,
    C: Clock,
{
    cursor: &'build EventCursor,
    decision_state: &'build mut RouteState,
    route_rows: Option<SelectedRouteCommitRows>,
    _role: core::marker::PhantomData<(&'r T, C)>,
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
