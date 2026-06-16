use super::{
    AssocTable, Cell, Config, EndpointLeaseId, FREE_REGION_CAPACITY, FreeRegion, Lane, LaneRelease,
    PhantomData, Rendezvous, RendezvousId, RouteTable, SessionId, Sidecar, TapRing, Transport,
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
        let header_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe {
            slab.as_mut_ptr().add(layout.header_start).cast::<Self>()
        };
        let tap_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe {
            slab.as_mut_ptr().add(layout.tap_start).cast::<TapEvent>()
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
        let runtime_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { slab.as_mut_ptr().add(layout.runtime_start) };
        let runtime_len = slab.len() - layout.runtime_start;
        let runtime_slab = /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_ptr, tap_storage, runtime_slab))
    }

    /// Materialize a rendezvous directly into the borrowed slab prefix using a
    /// public-path endpoint capacity derived from the runtime slab owner.
    ///
    /// # Safety
    /// The returned pointer is valid for as long as the borrowed slab storage lives.
    pub(crate) unsafe fn init_in_slab_auto(
        rv_id: RendezvousId,
        config: Config<'cfg>,
        transport: T,
    ) -> Option<*mut Self> {
        let Config { slab } = config;
        let lane_range = Config::initial_lane_range();
        let (dst, tap_storage, runtime_slab) = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { Self::carve_resident_storage(slab) }?;
        let image_frontier = 0u32;
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_storage));
            core::ptr::addr_of_mut!((*dst).tap_counter).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).slab).write(runtime_slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).frontier_workspace_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_storage).write(Sidecar::EMPTY);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(EndpointLeaseId::ZERO);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            core::ptr::addr_of_mut!((*dst).assoc_storage).write(Sidecar::EMPTY);
            core::ptr::addr_of_mut!((*dst).route_storage).write(Sidecar::EMPTY);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Outgoing, PortOpen, ReceivedFrame, TransportError};

    #[derive(Clone)]
    struct LayoutTransport;

    impl Transport for LayoutTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;

        fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: Outgoing<'_>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            core::task::Poll::Ready(Ok(()))
        }

        fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

        fn poll_recv<'a>(
            &'a self,
            _rx: &'a mut Self::Rx<'a>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<ReceivedFrame<'a>, Self::Error>> {
            core::task::Poll::Ready(Err(TransportError::Failed))
        }

        fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
            Err(TransportError::Failed)
        }
    }

    type TestRv<'a> = Rendezvous<'a, 'a, LayoutTransport>;

    #[test]
    fn single_slab_carve_layout_accounts_for_alignment_and_tap_ring() {
        let mut storage = [0u8; 8192];
        let slab = &mut storage[1..];
        let layout = TestRv::resident_carve_layout(slab).expect("layout");
        let base = slab.as_ptr() as usize;
        let header_ptr = base + layout.header_start;
        let runtime_ptr = base + layout.runtime_start;
        let tap_ptr = base + layout.tap_start;

        assert_eq!(header_ptr % core::mem::align_of::<TestRv<'_>>(), 0);
        assert_eq!(tap_ptr % core::mem::align_of::<TapEvent>(), 0);
        assert_eq!(runtime_ptr % core::mem::align_of::<TapEvent>(), 0);
        assert!(
            layout.tap_start > 0,
            "misaligned slab must add prefix padding"
        );
        assert!(
            layout.runtime_start
                >= core::mem::size_of::<TestRv<'_>>()
                    + core::mem::size_of::<[TapEvent; TAP_EVENTS]>(),
            "resident prefix must include header and tap ring bytes"
        );
    }
}
