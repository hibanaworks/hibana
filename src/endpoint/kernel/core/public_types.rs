use super::public_operation::PublicActiveOp;
use super::{
    BranchMeta, EndpointLeaseId, EventCursor, FrontierState, LaneGuard, LaneSlotArray, LeasedState,
    OfferState, Owner, Port, RendezvousId, RouteCommitRowSetBuilder, RouteState, SendMeta,
    SendRouteAuthority, SendState, SessionCtx, SessionId, StateIndex, Transport, lane_port,
};
use crate::endpoint::kernel::{branch_recv, recv};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicSlotOwnership {
    Borrowed = 0,
    Owned = 1,
}

/// Pinned actor allocation. Shared progress is separate from logical-view I/O.
/// Only a carrier operation lease may expose the interior mutable owners.
#[repr(C)]
pub(crate) struct ResidentActor<'r, T: Transport + 'r>
where
    T: Transport + 'r,
{
    pub(crate) public_header: crate::endpoint::carrier::KernelEndpointHeader<'r>,
    pub(crate) progress: core::cell::UnsafeCell<ActorProgress<'r, T>>,
    pub(crate) io: core::cell::UnsafeCell<EndpointIo<'r, T>>,
}

/// One semantic continuation, regardless of the logical view used to drive it.
pub(crate) struct ActorProgress<'r, T: Transport + 'r> {
    pub(crate) sid: SessionId,
    pub(crate) _owner: Owner<'r>,
    pub(crate) cursor: EventCursor,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: u32,
    pub(crate) public_slot_ownership: PublicSlotOwnership,
    pub(crate) session: SessionCtx<'r, T>,
    pub(in crate::endpoint::kernel) decision_state: LeasedState<RouteState>,
    pub(in crate::endpoint::kernel) route_commit_rows: LeasedState<RouteCommitRowSetBuilder>,
    pub(in crate::endpoint::kernel) frontier_state: LeasedState<FrontierState>,
}

/// Pending operations and transport handles of one logical Endpoint view.
/// These fields do not contain a second cursor or branch-selection authority.
pub(crate) struct EndpointIo<'r, T: Transport + 'r> {
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(in crate::endpoint::kernel) ports: LaneSlotArray<Port<'r, T>>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(in crate::endpoint::kernel) guards: LaneSlotArray<LaneGuard<'r, T>>,
    /// Primary lane index (first live application lane, not always lane 0).
    pub(crate) primary_lane: usize,
    pub(in crate::endpoint) public_active_op: PublicActiveOp,
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: recv::RecvState,
    pub(in crate::endpoint) public_branch_recv_state: branch_recv::BranchRecvState,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
}

/// One synchronous kernel borrow. It owns neither the actor nor its I/O.
/// The borrow ends before a public future returns Pending.
pub(crate) struct OperationKernel<'op, 'r, T: Transport + 'r> {
    pub(crate) public_header: &'op crate::endpoint::carrier::KernelEndpointHeader<'r>,
    pub(crate) progress: &'op mut ActorProgress<'r, T>,
    pub(crate) io: &'op mut EndpointIo<'r, T>,
}

impl<'r, T: Transport + 'r> ResidentActor<'r, T> {
    /// The unique lease is borrowed for exactly one synchronous kernel entry.
    pub(crate) fn borrow_kernel<'op>(
        &'op self,
        lease: &'op mut crate::rendezvous::core::EndpointOperationLease<'r>,
    ) -> OperationKernel<'op, 'r, T> {
        if !self.public_header.is_protected_by(lease) {
            crate::invariant();
        }
        OperationKernel {
            public_header: &self.public_header,
            // SAFETY: the checked lease excludes another carrier entry. Its
            // unique borrow prevents creating two kernels from this lease.
            progress: unsafe { &mut *self.progress.get() },
            io: unsafe { &mut *self.io.get() },
        }
    }

    /// Exclusive access is used only before publication and during final Drop.
    pub(crate) fn kernel_mut(&mut self) -> OperationKernel<'_, 'r, T> {
        OperationKernel {
            public_header: &self.public_header,
            progress: self.progress.get_mut(),
            io: self.io.get_mut(),
        }
    }
}

impl<'r, T: Transport + 'r> Drop for ResidentActor<'r, T> {
    fn drop(&mut self) {
        self.kernel_mut().retire_actor();
    }
}

pub(crate) struct MaterializedRouteBranch<'r> {
    pub(crate) offered_frame: Option<OfferedFrame<'r>>,
    pub(crate) branch_meta: BranchMeta,
}

impl MaterializedRouteBranch<'_> {
    #[inline]
    pub(crate) fn discard_terminal(mut self) {
        if let Some(payload) = self.offered_frame.take() {
            payload.discard_terminal();
        }
    }
}

pub(crate) struct OfferedFrame<'a> {
    frame: lane_port::ReceivedFrame<'a>,
}

impl<'a> OfferedFrame<'a> {
    #[inline]
    pub(crate) const fn new(frame: lane_port::ReceivedFrame<'a>) -> Self {
        Self { frame }
    }

    #[inline]
    pub(crate) fn into_frame(self) -> lane_port::ReceivedFrame<'a> {
        self.frame
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
    pub(crate) fn discard_terminal(self) {
        self.frame.discard_uncommitted()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SendPreview {
    meta: SendMeta,
    cursor_index: StateIndex,
    route_authority: SendRouteAuthority,
}

impl SendPreview {
    #[inline]
    pub(crate) const fn materialized_branch(meta: SendMeta, cursor_index: StateIndex) -> Self {
        Self {
            meta,
            cursor_index,
            route_authority: SendRouteAuthority::materialized_branch(),
        }
    }

    #[inline]
    pub(crate) const fn with_route_authority(
        meta: SendMeta,
        cursor_index: StateIndex,
        route_authority: SendRouteAuthority,
    ) -> Self {
        Self {
            meta,
            cursor_index,
            route_authority,
        }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> u8 {
        self.meta.frame_label
    }

    #[inline]
    pub(crate) const fn into_parts(self) -> (SendMeta, StateIndex, SendRouteAuthority) {
        (self.meta, self.cursor_index, self.route_authority)
    }
}
