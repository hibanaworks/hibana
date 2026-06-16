use super::{
    BranchMeta, EndpointLeaseId, EventCursor, FrontierState, LaneGuard, LaneSlotArray, LeasedState,
    OfferState, Owner, Payload, Port, RendezvousId, RouteCommitRowSetBuilder, RouteState, SendMeta,
    SendState, SessionCtx, SessionId, StateIndex, Transport, lane_port,
};
use crate::endpoint::kernel::{decode, recv};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint) enum PublicActiveOp {
    Idle,
    Poisoned,
    Send,
    Recv,
    Offer,
    RouteBranch,
    Decode,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicOpLease {
    Rejected = 0,
    Held = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicSlotOwnership {
    Borrowed = 0,
    Owned = 1,
}

/// Endpoint kernel. Owns the rendezvous port as well as the lane
/// release handle. Dropping the endpoint releases the lane back to the
/// `SessionCluster` via the handle.
#[repr(C)]
pub(crate) struct CursorEndpoint<'r, const ROLE: u8, T: Transport + 'r>
where
    T: Transport + 'r,
{
    pub(crate) public_header: crate::endpoint::carrier::KernelEndpointHeader<'r>,
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(in crate::endpoint::kernel) ports: LaneSlotArray<Port<'r, T>>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(in crate::endpoint::kernel) guards: LaneSlotArray<LaneGuard<'r, T>>,
    /// Primary lane index (first live application lane, not always lane 0).
    pub(crate) primary_lane: usize,
    pub(crate) sid: SessionId,
    pub(crate) _owner: Owner<'r>,
    /// Event cursor for multi-lane affine progress.
    pub(crate) cursor: EventCursor,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: u32,
    pub(crate) public_slot_ownership: PublicSlotOwnership,
    pub(in crate::endpoint) public_active_op: PublicActiveOp,
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: recv::RecvState,
    pub(in crate::endpoint) public_decode_state: decode::DecodeState<'r>,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
    pub(crate) session: SessionCtx<'r, T>,
    pub(in crate::endpoint::kernel) decision_state: LeasedState<RouteState>,
    pub(in crate::endpoint::kernel) route_commit_rows: LeasedState<RouteCommitRowSetBuilder>,
    pub(in crate::endpoint::kernel) frontier_state: LeasedState<FrontierState>,
}

pub(crate) struct RouteBranch<'r, const ROLE: u8, T: Transport + 'r> {
    pub(crate) label: u8,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
    pub(crate) _cfg: core::marker::PhantomData<fn() -> &'r T>,
}

pub(crate) struct MaterializedRouteBranch<'r> {
    pub(crate) label: u8,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
}

impl<'r> MaterializedRouteBranch<'r> {
    #[inline]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub(crate) fn discard_terminal(mut self) {
        if let Some(payload) = self.staged_payload.take() {
            payload.discard_terminal();
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchPreviewView {
    pub(in crate::endpoint::kernel) label: u8,
    pub(in crate::endpoint::kernel) branch_meta: BranchMeta,
}

impl BranchPreviewView {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(label: u8, branch_meta: BranchMeta) -> Self {
        Self { label, branch_meta }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn from_materialized(
        branch: &MaterializedRouteBranch<'_>,
    ) -> Self {
        Self::new(branch.label, branch.branch_meta)
    }
}

pub(crate) struct StagedPayload<'a> {
    frame: lane_port::ReceivedFrame<'a>,
}

impl<'a> StagedPayload<'a> {
    #[inline]
    pub(crate) const fn new(frame: lane_port::ReceivedFrame<'a>) -> Self {
        Self { frame }
    }

    #[inline]
    pub(crate) fn into_frame(self) -> lane_port::ReceivedFrame<'a> {
        self.frame
    }

    #[inline]
    pub(crate) fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'a>, E>
    where
        F: FnOnce(Payload<'a>) -> Result<(), E>,
    {
        self.frame.validated_payload(validate)
    }

    #[inline]
    pub(crate) const fn lane(&self) -> u8 {
        self.frame.lane_wire()
    }

    #[inline]
    pub(crate) const fn transport_frame_label(&self) -> u8 {
        self.frame.frame_label_raw()
    }

    #[inline]
    pub(crate) fn commit(self) -> Payload<'a> {
        self.frame.into_payload()
    }

    #[inline]
    pub(crate) fn discard_terminal(self) {
        self.frame.discard_uncommitted()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SendPreview {
    meta: SendMeta,
    cursor_index: StateIndex,
}

impl SendPreview {
    #[inline]
    pub(crate) const fn new(meta: SendMeta, cursor_index: StateIndex) -> Self {
        Self { meta, cursor_index }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> u8 {
        self.meta.frame_label
    }

    #[inline]
    pub(crate) const fn into_parts(self) -> (SendMeta, StateIndex) {
        (self.meta, self.cursor_index)
    }
}

impl<'r, const ROLE: u8, T> From<RouteBranch<'r, ROLE, T>> for MaterializedRouteBranch<'r>
where
    T: Transport + 'r,
{
    #[inline]
    fn from(branch: RouteBranch<'r, ROLE, T>) -> Self {
        Self {
            label: branch.label,
            staged_payload: branch.staged_payload,
            branch_meta: branch.branch_meta,
        }
    }
}
