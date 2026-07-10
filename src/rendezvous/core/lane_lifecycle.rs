use super::{
    AssocTable, Cell, Lane, LaneRelease, PhantomData, Rendezvous, RendezvousAccessState,
    RendezvousId, RouteTable, RuntimeResources, SessionId, Sidecar, TapRing, Transport, UnsafeCell,
    Waker, emit, events,
};
use crate::{observe::core::TapEvent, runtime_core::consts::TAP_EVENTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResidentCarveLayout {
    header_start: usize,
    tap_start: usize,
    runtime_start: usize,
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    fn resident_carve_layout(slab: &[u8]) -> Option<ResidentCarveLayout> {
        let base = slab.as_ptr() as usize;
        let len = slab.len();
        let header_offset = Self::align_up(base, core::mem::align_of::<Self>());
        let header_end = header_offset.checked_add(core::mem::size_of::<Self>())?;
        let tap_offset = Self::align_up(header_end, core::mem::align_of::<TapEvent>());
        let tap_end = tap_offset.checked_add(core::mem::size_of::<[TapEvent; TAP_EVENTS]>())?;
        let header_start = header_offset.checked_sub(base)?;
        let tap_start = tap_offset.checked_sub(base)?;
        let runtime_start = tap_end.checked_sub(base)?;
        if runtime_start > len {
            return None;
        }
        Some(ResidentCarveLayout {
            header_start,
            tap_start,
            runtime_start,
        })
    }

    unsafe fn carve_resident_storage(
        slab: &mut [u8],
    ) -> Option<(*mut Self, &mut [TapEvent; TAP_EVENTS], &mut [u8])> {
        let layout = Self::resident_carve_layout(slab)?;
        let slab_len = slab.len();
        let slab_ptr = slab.as_mut_ptr();
        let header_ptr = /* SAFETY: `resident_carve_layout` proved the
        unpublished header range is inside `slab` and aligned for `Rendezvous`. */ unsafe {
            slab_ptr.add(layout.header_start).cast::<Self>()
        };
        let tap_ptr = /* SAFETY: `resident_carve_layout` proved `tap_start` is
        inside `slab` and aligned for `TapEvent`; `TAP_EVENTS` initialized
        cells fit before the runtime slice. */ unsafe {
            slab_ptr.add(layout.tap_start).cast::<TapEvent>()
        };
        let mut idx = 0usize;
        while idx < TAP_EVENTS {
            /* SAFETY: tap storage is carved from the caller slab with TapEvent alignment and initialized exactly once. */
            unsafe {
                tap_ptr.add(idx).write(TapEvent::zero());
            }
            idx += 1;
        }
        let tap_storage = /* SAFETY: tap storage was carved at TapEvent alignment and initialized for exactly TAP_EVENTS elements. */ unsafe {
            &mut *(tap_ptr.cast::<[TapEvent; TAP_EVENTS]>())
        };
        let runtime_ptr = /* SAFETY: `runtime_start <= slab.len()` was checked
        by `resident_carve_layout`, so this points at the runtime suffix. */ unsafe { slab_ptr.add(layout.runtime_start) };
        let runtime_len = slab_len - layout.runtime_start;
        let runtime_slab = /* SAFETY: `runtime_ptr` and `runtime_len` describe
        the suffix of the same caller slab after the rendezvous header and tap
        ring; it is disjoint from both initialized prefixes. */ unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_ptr, tap_storage, runtime_slab))
    }

    /// Materialize a rendezvous directly into the borrowed slab prefix using a
    /// public-path endpoint capacity derived from the runtime slab owner.
    ///
    /// # Safety
    /// The returned pointer is valid for as long as the borrowed slab storage lives.
    pub(crate) unsafe fn init_in_slab_auto(
        rv_id: RendezvousId,
        resources: RuntimeResources<'cfg>,
        transport: T,
    ) -> Option<*mut Self> {
        let RuntimeResources { slab } = resources;
        let lane_range = RuntimeResources::initial_lane_range();
        let (dst, tap_storage, runtime_slab) = /* SAFETY: `slab` is the
        exclusive runtime resource for this rendezvous; carving initializes the
        tap ring and returns disjoint header/runtime regions. */ unsafe { Self::carve_resident_storage(slab) }?;
        let runtime_slab_len = runtime_slab.len();
        let runtime_slab_ptr = runtime_slab.as_mut_ptr();
        let image_frontier = 0u32;
        /* SAFETY: `dst` is the unpublished rendezvous header carved from the
        caller slab. All resident fields, tables, lane range, and transport
        owners are written before the pointer is returned to the session kit. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).access_state)
                .write(Cell::new(RendezvousAccessState::Available));
            core::ptr::addr_of_mut!((*dst).registry_next).write(Cell::new(None));
            core::ptr::addr_of_mut!((*dst).resolver_bucket).write(UnsafeCell::new(
                crate::session::cluster::core::ResolverBucket::empty(),
            ));
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_storage));
            core::ptr::addr_of_mut!((*dst).tap_counter).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).slab_ptr).write(runtime_slab_ptr);
            core::ptr::addr_of_mut!((*dst).slab_len).write(runtime_slab_len);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(Cell::new(image_frontier));
            core::ptr::addr_of_mut!((*dst).frontier_workspace_bytes).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).endpoint_lease_storage).write(Cell::new(Sidecar::EMPTY));
            core::ptr::addr_of_mut!((*dst).lane_base).write(Cell::new(lane_range.start as u32));
            core::ptr::addr_of_mut!((*dst).lane_end).write(Cell::new(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            core::ptr::addr_of_mut!((*dst).assoc_storage).write(Cell::new(Sidecar::EMPTY));
            core::ptr::addr_of_mut!((*dst).route_storage).write(Cell::new(Sidecar::EMPTY));
            AssocTable::init_empty(core::ptr::addr_of_mut!((*dst).assoc));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
        }
        Some(dst)
    }
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
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
        let mut after = None;
        while let Some(lane) = self.assoc.next_session_lane(sid, after) {
            self.routes.wake_lane_waiters(lane);
            after = Some(lane.raw());
        }
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

    pub(crate) fn release_lane(&self, sid: SessionId, lane: Lane) -> LaneRelease {
        let remaining = crate::invariant_some(self.assoc.decrement(lane, sid));
        if remaining > 0 {
            return LaneRelease::StillHeld;
        }
        if !self.assoc.lane_has_entries(lane) {
            self.reset_lane_recycled_state(lane);
        }
        LaneRelease::Released
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
                lane.as_wire() as u16,
            ),
        );
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
