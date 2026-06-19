//! Branch-recv path helpers for `RouteBranch::recv`.

mod finish;
mod state;

use core::task::Poll;

pub(crate) use state::{BranchRecvRestore, BranchRecvState};

use super::decision_state::RouteState;
use super::{
    core::{
        BranchPreviewView, BranchRecvRuntimeDesc, CursorEndpoint, MaterializedRouteBranch,
        prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range,
        scope_slot_for_route_from_cursor,
    },
    decision_state::{SelectedRouteCommitRows, SelectedRouteCommitRowsRef},
    lane_port,
    offer::{BranchCommitPlan, BranchKind},
    recv_commit_plan::{
        BranchRecvCommitInput, EndpointRxEventPlan, RecvCommitPayload, RecvCommitPlan,
    },
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::{EventCursor, RecvMeta, StateIndex, state_index_to_usize},
    transport::{Transport, wire::Payload},
};

struct BranchRecvCommitBuilder<'build, 'r, const ROLE: u8, T>
where
    T: Transport + 'r,
{
    cursor: &'build EventCursor,
    decision_state: &'build mut RouteState,
    route_rows: Option<SelectedRouteCommitRows>,
    _role: core::marker::PhantomData<&'r T>,
}

#[inline]
pub(super) fn branch_recv_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
