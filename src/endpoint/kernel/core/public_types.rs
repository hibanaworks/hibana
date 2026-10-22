use super::public_operation::PublicActiveOp;
use super::{
    BranchMeta, EndpointLeaseId, EventCursor, FrontierState, LaneGuard, LaneSlotArray, LeasedState,
    OfferState, Port, RendezvousId, RouteCommitRowSetBuilder, RouteState, SendMeta,
    SendRouteAuthority, SendState, SessionCtx, SessionId, StateIndex, Transport, lane_port,
};
use crate::endpoint::kernel::{branch_recv, recv};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

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
    pub(crate) public_header: crate::endpoint::carrier::KernelActorHeader,
    pub(crate) context: ActorContext<'r, T>,
    pub(crate) progress: core::cell::RefCell<core::mem::MaybeUninit<ActorProgress>>,
    pub(crate) views: ActorViews<'r, T>,
}

/// One logical view's immutable carrier and independent pending operations.
#[repr(C)]
pub(crate) struct ResidentView<'r, T: Transport + 'r> {
    pub(crate) public_header: crate::endpoint::carrier::KernelEndpointHeader,
    pub(in crate::endpoint::kernel) primary_lane: usize,
    pub(crate) retired_next: core::cell::Cell<Option<core::ptr::NonNull<ResidentView<'r, T>>>>,
    pub(crate) io: core::cell::UnsafeCell<EndpointIo<'r, T>>,
    pub(in crate::endpoint::kernel) operations: LaneSlotArray<ResidentOperation<'r>>,
}

/// Resident I/O for one independently borrowed operation region. Protocol
/// progress and transport handles remain owned once by the actor and view.
pub(crate) struct ResidentOperation<'r> {
    pub(crate) waiter: crate::rendezvous::core::EndpointWaiter,
    pub(super) retired: core::cell::Cell<Option<RetiredOperation>>,
    pub(super) retired_next: core::cell::Cell<Option<core::ptr::NonNull<Self>>>,
    pub(crate) state: core::cell::UnsafeCell<OperationIo<'r>>,
}

/// Destruction waiting for the current synchronous cursor borrow to end.
/// The region identifies a borrow to release, never another progress cursor.
#[derive(Clone, Copy)]
pub(super) struct RetiredOperation {
    pub(super) role: u8,
    pub(super) region: crate::endpoint::carrier::OperationRegion,
    pub(super) kind: OperationRetirement,
}

/// Deferred RAII work, independent of the canonical operation phase.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationRetirement {
    Release,
    Parallel,
}

impl<'r> ResidentOperation<'r> {
    pub(crate) const fn new() -> Self {
        Self {
            waiter: crate::rendezvous::core::EndpointWaiter::empty(),
            retired: core::cell::Cell::new(None),
            retired_next: core::cell::Cell::new(None),
            state: core::cell::UnsafeCell::new(OperationIo {
                public_active_op: PublicActiveOp::Idle,
                public_offer_state: OfferState::new(),
                public_route_branch: None,
                public_recv_state: recv::RecvState::new(),
                public_branch_recv_state: branch_recv::BranchRecvState::empty(),
                public_send_state: SendState::Done,
            }),
        }
    }
}

impl<'r, T: Transport + 'r> ResidentView<'r, T> {
    pub(crate) fn operation(&self, lane: usize) -> Option<&ResidentOperation<'r>> {
        self.operations.get(lane).and_then(Option::as_ref)
    }

    pub(crate) fn operations(&self) -> impl Iterator<Item = &ResidentOperation<'r>> {
        self.operations
            .iter()
            .map(|slot| crate::invariant_some(slot.as_ref()))
    }

    pub(crate) fn primary_operation(&self) -> &ResidentOperation<'r> {
        crate::invariant_some(self.operation(self.primary_lane))
    }
}

/// Sparse view records carved from the actor's caller-provided allocation.
pub(crate) struct ActorViews<'r, T: Transport + 'r> {
    ptr: core::ptr::NonNull<ResidentView<'r, T>>,
    len: u16,
}

impl<'r, T: Transport + 'r> ActorViews<'r, T> {
    pub(crate) unsafe fn from_parts(ptr: *mut ResidentView<'r, T>, len: usize) -> Self {
        Self {
            ptr: crate::invariant_some(core::ptr::NonNull::new(ptr)),
            len: crate::invariant_some(u16::try_from(len).ok()),
        }
    }

    pub(crate) fn as_slice(&self) -> &[ResidentView<'r, T>] {
        // SAFETY: initialization fills every record before publication. The
        // allocation stays pinned until all issued views have been destroyed.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len as usize) }
    }

    pub(crate) fn get(&self, role: u8) -> Option<&ResidentView<'r, T>> {
        let views = self.as_slice();
        views
            .binary_search_by_key(&role, |view| view.public_header.role())
            .ok()
            .map(|index| &views[index])
    }
}

/// Pinned run identity and shared rendezvous access, independent of cursor mutation.
pub(crate) struct ActorContext<'r, T: Transport + 'r> {
    pub(crate) program: &'static crate::global::compiled::images::CompiledProgramRef,
    pub(crate) sid: SessionId,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: core::cell::Cell<u32>,
    pub(crate) view_generation: core::cell::Cell<u32>,
    pub(crate) live_views: core::cell::Cell<u16>,
    pub(crate) retired_views: core::cell::Cell<Option<core::ptr::NonNull<ResidentView<'r, T>>>>,
    pub(in crate::endpoint::kernel) retired_operations:
        core::cell::Cell<Option<core::ptr::NonNull<ResidentOperation<'r>>>>,
    pub(crate) public_slot_ownership: core::cell::Cell<PublicSlotOwnership>,
    pub(crate) session: SessionCtx<'r, T>,
}

impl<'r, T: Transport + 'r> ActorContext<'r, T> {
    pub(crate) fn try_operation_lease(
        &self,
    ) -> Option<crate::rendezvous::core::EndpointOperationLease<'r>> {
        crate::rendezvous::core::EndpointOperationLease::try_acquire(
            self.session.rendezvous().endpoint_operation_access_state(),
            self.public_slot,
        )
    }

    fn is_protected_by(&self, lease: &crate::rendezvous::core::EndpointOperationLease<'_>) -> bool {
        lease.protects(
            self.session.rendezvous().endpoint_operation_access_state(),
            self.public_slot,
        )
    }
}

/// Mutable protocol progress, with no view or allocation ownership metadata.
pub(crate) struct ActorProgress {
    pub(crate) cursor: EventCursor,
    pub(in crate::endpoint::kernel) decision_state: LeasedState<RouteState>,
    pub(in crate::endpoint::kernel) route_commit_rows: LeasedState<RouteCommitRowSetBuilder>,
    pub(in crate::endpoint::kernel) frontier_state: LeasedState<FrontierState>,
}

/// Transport handles of one logical Endpoint view, independent of pending
/// operation-state owners. These fields contain no cursor or choice authority.
pub(crate) struct EndpointIo<'r, T: Transport + 'r> {
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(in crate::endpoint::kernel) ports: LaneSlotArray<Port<'r, T>>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(in crate::endpoint::kernel) guards: LaneSlotArray<LaneGuard<'r, T>>,
}

pub(crate) struct OperationIo<'r> {
    pub(in crate::endpoint) public_active_op: PublicActiveOp,
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: recv::RecvState,
    pub(in crate::endpoint) public_branch_recv_state: branch_recv::BranchRecvState,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
}

#[cfg(all(test, hibana_repo_tests))]
impl OperationIo<'_> {
    /// Read the existing pending plan; this observation never prepares a send.
    pub(crate) fn proof_prepared_send(&self) -> Option<(usize, u8, u8)> {
        let SendState::Sending { pending } = &self.public_send_state else {
            return None;
        };
        let event = pending.commit_plan.as_ref()?.proof.progress.delta.event()?;
        Some((
            event.eff_index().dense_ordinal(),
            event.event_label(),
            event.lane(),
        ))
    }
}

/// One synchronous kernel borrow. It owns neither the actor nor its I/O.
/// The borrow ends before a public future returns Pending.
pub(crate) struct OperationKernel<'op, 'r, T: Transport + 'r> {
    pub(crate) public_header: &'op crate::endpoint::carrier::KernelEndpointHeader,
    pub(crate) waiter: &'op crate::rendezvous::core::EndpointWaiter,
    pub(crate) context: &'op ActorContext<'r, T>,
    pub(crate) progress: &'op mut ActorProgress,
    pub(crate) io: &'op mut EndpointIo<'r, T>,
    pub(crate) operation: &'op mut OperationIo<'r>,
    pub(in crate::endpoint::kernel) operations: &'op LaneSlotArray<ResidentOperation<'r>>,
    pub(in crate::endpoint::kernel) views: &'op ActorViews<'r, T>,
    pub(crate) region: crate::endpoint::carrier::OperationRegion,
}

impl<'r, T: Transport + 'r> ResidentActor<'r, T> {
    /// Initial issuance is possible once for each statically assigned view.
    /// Retired views can only return through a projected owned receive.
    pub(crate) fn issue_initial_view(&self, role: u8) -> bool {
        let Some(view) = self.views.get(role) else {
            return false;
        };
        let initial_actor = match self.context.program.initial_actor_for(role) {
            Some(actor) => actor,
            None => role,
        };
        let progress = match self.progress.try_borrow_mut() {
            Ok(progress) => progress,
            Err(_) => return false,
        };
        // SAFETY: published actor progress is initialized and exclusively
        // borrowed for this callback-free issuance transaction.
        if unsafe { progress.assume_init_ref() }.cursor.role() != initial_actor {
            return false;
        }
        let Some(generation) =
            crate::endpoint::carrier::next_view_generation(self.context.view_generation.get())
        else {
            return false;
        };
        // Reserve the even successor for retirement; no live nonce may wrap.
        if !view.public_header.issue_initial(generation) {
            return false;
        }
        self.context.view_generation.set(generation);
        self.context.live_views.set(crate::invariant_some(
            self.context.live_views.get().checked_add(1),
        ));
        true
    }
    pub(crate) fn retire_view(&self, view: &ResidentView<'r, T>) {
        if view.public_header.actor() != core::ptr::NonNull::from(&self.public_header)
            || view.public_header.generation() == 0
        {
            crate::invariant();
        }
        view.public_header.retire_generation();
        let next = self
            .context
            .retired_views
            .replace(Some(core::ptr::NonNull::from(view)));
        view.retired_next.set(next);
        self.context.live_views.set(crate::invariant_some(
            self.context.live_views.get().checked_sub(1),
        ));
    }

    pub(crate) fn finish_all_view_io(&self) {
        for view in self.views.as_slice() {
            let mut waiters = crate::endpoint::carrier::WaiterTransfer::empty();
            crate::invariant_some(self.try_with_kernel(
                view,
                crate::endpoint::carrier::OperationRegion::WHOLE,
                |kernel| {
                    kernel.clear_endpoint_waiter(&mut waiters);
                    kernel.finish_view_io();
                },
            ));
            drop(waiters);
        }
    }

    /// The unique lease is borrowed for exactly one synchronous kernel entry.
    pub(crate) fn with_kernel<R>(
        &self,
        lease: &mut crate::rendezvous::core::EndpointOperationLease<'r>,
        view: &ResidentView<'r, T>,
        region: crate::endpoint::carrier::OperationRegion,
        f: impl FnOnce(&mut OperationKernel<'_, 'r, T>) -> R,
    ) -> R {
        if !self.context.is_protected_by(lease)
            || view.public_header.actor() != core::ptr::NonNull::from(&self.public_header)
        {
            crate::invariant();
        }
        crate::invariant_some(self.try_with_kernel(view, region, f))
    }

    pub(crate) fn try_with_kernel<R>(
        &self,
        view: &ResidentView<'r, T>,
        region: crate::endpoint::carrier::OperationRegion,
        f: impl FnOnce(&mut OperationKernel<'_, 'r, T>) -> R,
    ) -> Option<R> {
        if view.public_header.actor() != core::ptr::NonNull::from(&self.public_header)
            || view.public_header.view_generation()
                > self.context.view_generation.get().saturating_add(1)
        {
            crate::invariant();
        }
        let progress = self.progress.try_borrow_mut().ok()?;
        // SAFETY: progress is streamed into this cell before publication.
        // RefMut protects it during normal polls and callback-driven Drop.
        let mut progress =
            core::cell::RefMut::map(progress, |progress| unsafe { progress.assume_init_mut() });
        let operation = crate::invariant_some(view.operation(region.lane(view.primary_lane)));
        let mut kernel = OperationKernel {
            public_header: &view.public_header,
            waiter: &operation.waiter,
            context: &self.context,
            progress: &mut progress,
            // SAFETY: the common progress borrow excludes another kernel;
            // the uniquely issued Endpoint selects this one view's I/O.
            io: unsafe { &mut *view.io.get() },
            // SAFETY: the same actor progress borrow excludes another kernel
            // entry while this operation state's mutable borrow is active.
            operation: unsafe { &mut *operation.state.get() },
            operations: &view.operations,
            views: &self.views,
            region,
        };
        Some(f(&mut kernel))
    }
}

impl<'r, T: Transport + 'r> Drop for ResidentActor<'r, T> {
    fn drop(&mut self) {
        for view in self.views.as_slice() {
            view.public_header.retire_generation();
        }
        // All headers are fenced before any arbitrary Waker destructor runs.
        // Forgotten parallel futures must not retain wake owners in the slab.
        for view in self.views.as_slice() {
            for operation in view.operations() {
                drop(operation.waiter.take());
            }
        }
        let len = self.views.len as usize;
        let mut index = 0;
        while index < len {
            // SAFETY: this record has not yet been destroyed; earlier records
            // are not reborrowed while later I/O owners are retired.
            let view = unsafe { &*self.views.ptr.as_ptr().add(index) };
            crate::invariant_some(self.try_with_kernel(
                view,
                crate::endpoint::carrier::OperationRegion::WHOLE,
                |kernel| kernel.retire_actor(),
            ));
            // SAFETY: terminal retirement has removed callbacks and pending
            // frames. This is the unique final destruction of this I/O owner.
            unsafe {
                core::ptr::drop_in_place(self.views.ptr.as_ptr().add(index));
            }
            index += 1;
        }
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
    pub(crate) const fn transport_frame_label(&self) -> u16 {
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
    pub(crate) const fn frame_label(self) -> u16 {
        self.meta.frame_label
    }

    #[inline]
    pub(crate) const fn into_parts(self) -> (SendMeta, StateIndex, SendRouteAuthority) {
        (self.meta, self.cursor_index, self.route_authority)
    }
}
