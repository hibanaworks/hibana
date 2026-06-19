//! Branch-recv path helpers for `RouteBranch::recv`.

mod finish;
mod payload;
mod state;

use core::task::Poll;

use payload::BranchRecvCommitPayload;
pub(crate) use state::{BranchRecvRestore, BranchRecvState};

use super::decision_state::RouteState;
use super::{
    core::{
        BranchPreviewView, BranchRecvRuntimeDesc, CommitDelta, CursorEndpoint,
        MaterializedRouteBranch, PreparedCommitDelta,
        prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range,
        scope_slot_for_route_from_cursor,
    },
    decision_state::{SelectedRouteCommitRows, SelectedRouteCommitRowsRef},
    lane_port,
    offer::{BranchCommitPlan, BranchKind},
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::{EventCursor, RecvMeta, StateIndex, state_index_to_usize},
    observe::ids,
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

#[derive(Clone, Copy)]
struct EndpointRxEventPlan {
    lane: u8,
    label: u8,
    event_id: u16,
}

impl EndpointRxEventPlan {
    #[inline]
    const fn from_branch(branch: BranchPreviewView) -> Self {
        Self {
            lane: branch.branch_meta.lane_wire,
            label: branch.label,
            event_id: if branch.branch_meta.origin.is_session() {
                ids::ENDPOINT_SESSION
            } else {
                ids::ENDPOINT_RECV
            },
        }
    }
}

#[derive(Clone, Copy)]
enum BranchRecvProgressPlan {
    Wire { delta: CommitDelta },
    NonWire { delta: CommitDelta },
}

struct RecvCommitPlan<'r> {
    branch: BranchCommitPlan,
    event: EndpointRxEventPlan,
    progress: BranchRecvProgressPlan,
    payload: BranchRecvCommitPayload<'r>,
}

enum PreparedBranchRecvProgressPlan {
    Wire { delta: PreparedCommitDelta },
    NonWire { delta: PreparedCommitDelta },
}

struct PreparedBranchRecvPublishPlan<'r> {
    branch: BranchCommitPlan,
    event: EndpointRxEventPlan,
    progress: PreparedBranchRecvProgressPlan,
    payload: BranchRecvCommitPayload<'r>,
}

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

impl<'r> RecvCommitPlan<'r> {
    fn wire<F>(
        branch: BranchCommitPlan,
        event: EndpointRxEventPlan,
        progress: BranchRecvProgressPlan,
        frame: lane_port::ReceivedFrame<'r>,
        validate: F,
    ) -> RecvResult<Self>
    where
        F: FnOnce(Payload<'r>) -> Result<(), CodecError>,
    {
        if let Err(err) = frame.validated_payload(validate) {
            frame.discard_uncommitted();
            return Err(RecvError::Codec(err));
        }
        Ok(Self {
            branch,
            event,
            progress,
            payload: BranchRecvCommitPayload::Wire(frame),
        })
    }

    fn non_wire<F>(
        branch: BranchCommitPlan,
        event: EndpointRxEventPlan,
        progress: BranchRecvProgressPlan,
        payload: Payload<'r>,
        validate: F,
    ) -> RecvResult<Self>
    where
        F: FnOnce(Payload<'r>) -> Result<(), CodecError>,
    {
        validate(payload).map_err(RecvError::Codec)?;
        Ok(Self {
            branch,
            event,
            progress,
            payload: BranchRecvCommitPayload::NonWire(payload),
        })
    }
}
