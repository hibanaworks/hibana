use super::{
    AssocTable, Clock, Config, EndpointLeaseId, FREE_REGION_CAPACITY, FreeRegion, Lane,
    LaneRelease, PhantomData, Rendezvous, RendezvousId, RouteTable, SessionId, TapRing, Transport,
    Waker, emit, events,
};
impl<'rv, 'cfg, T: Transport, C: Clock> Rendezvous<'rv, 'cfg, T, C>
where
    'cfg: 'rv,
{
    unsafe fn carve_resident_storage(slab: &mut [u8]) -> Option<(*mut Self, &mut [u8])> {
        let base = slab.as_mut_ptr() as usize;
        let len = slab.len();
        let header_offset = Self::align_up(base, core::mem::align_of::<Self>());
        let header_end = header_offset.checked_add(core::mem::size_of::<Self>())?;
        let runtime_offset = header_end.wrapping_sub(base);
        if runtime_offset > len {
            return None;
        }
        let runtime_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { slab.as_mut_ptr().add(runtime_offset) };
        let runtime_len = len - runtime_offset;
        let runtime_slab = /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_offset as *mut Self, runtime_slab))
    }

    /// Materialize a rendezvous directly into the borrowed slab prefix using a
    /// public-path endpoint capacity derived from the runtime slab owner.
    ///
    /// # Safety
    /// The returned pointer is valid for as long as the borrowed slab storage lives.
    pub(crate) unsafe fn init_in_slab_auto(
        rv_id: RendezvousId,
        config: Config<'cfg, C>,
        transport: T,
    ) -> Option<*mut Self> {
        let Config {
            tap_buf,
            slab,
            clock,
            ..
        } = config;
        let lane_range = Config::<C>::initial_lane_range();
        let (dst, runtime_slab) = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { Self::carve_resident_storage(slab) }?;
        let image_frontier = 0u32;
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(runtime_slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).frontier_workspace_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(EndpointLeaseId::ZERO);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_reclaim_delta).write(0);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            AssocTable::init_empty(core::ptr::addr_of_mut!((*dst).assoc));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
        }
        Some(dst)
    }
}

impl<'rv, 'cfg, T: Transport, C: Clock> Rendezvous<'rv, 'cfg, T, C>
where
    'cfg: 'rv,
{
    pub(crate) fn session_fault(
        &self,
        sid: SessionId,
    ) -> Option<crate::rendezvous::SessionFaultKind> {
        self.assoc.session_fault(sid)
    }

    #[inline]
    pub(crate) fn poison_session(
        &self,
        sid: SessionId,
        cause: crate::rendezvous::SessionFaultKind,
    ) -> crate::rendezvous::SessionFaultKind {
        let kind = self.assoc.poison_session(sid, cause);
        self.assoc.wake_session_waiters(sid);
        self.assoc.for_each_lane(sid, |lane| {
            self.routes.wake_lane_waiters(lane);
        });
        kind
    }

    #[inline]
    pub(crate) fn register_session_waiter(&self, sid: SessionId, lane: Lane, waker: &Waker) {
        self.assoc.register_waiter(sid, lane, waker);
    }

    #[inline]
    pub(crate) fn clear_session_waiter(&self, sid: SessionId, lane: Lane) {
        self.assoc.clear_waiter(sid, lane);
    }

    pub(crate) fn release_lane(&self, lane: Lane) -> LaneRelease {
        let sid = crate::invariant_some(self.assoc.get_sid(lane));
        let remaining = crate::invariant_some(self.assoc.decrement(lane, sid));
        if remaining > 0 {
            return LaneRelease::StillHeld;
        }
        self.reset_lane_recycled_state(lane);
        LaneRelease::Released(sid)
    }

    pub(crate) fn reset_lane_recycled_state(&self, lane: Lane) {
        self.routes.reset_lane(lane);
    }

    #[inline]
    pub(crate) fn emit_lane_release(&self, sid: SessionId, lane: Lane) {
        emit(
            self.tap(),
            events::lane_release(
                self.now32(),
                self.id.raw() as u32,
                sid.raw(),
                lane.raw() as u16,
            ),
        );
    }
}
