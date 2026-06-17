use super::*;
use crate::{
    runtime_core::resources::RuntimeResources,
    session::types::RendezvousId,
    transport::{Outgoing, PortOpen, ReceivedFrame, TransportError},
};
use core::task::{Context, Poll};

#[derive(Clone, Copy)]
struct FailingTransport;

struct FailingTx;
struct FailingRx;

impl Transport for FailingTransport {
    type Error = TransportError;
    type Tx<'a> = FailingTx;
    type Rx<'a> = FailingRx;

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (FailingTx, FailingRx)
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Err(TransportError::Failed))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        Poll::Ready(Err(TransportError::Failed))
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        Err(TransportError::Failed)
    }
}

fn init_test_rendezvous(slab: &mut [u8]) -> &mut Rendezvous<'_, '_, FailingTransport> {
    let rv_ptr = unsafe {
        // SAFETY: the test owns the whole slab for the duration of the rendezvous.
        Rendezvous::init_in_slab_auto(
            RendezvousId::new(1),
            RuntimeResources::new(slab),
            FailingTransport,
        )
        .expect("rendezvous")
    };
    // SAFETY: init_in_slab_auto returned a unique resident pointer backed by slab.
    unsafe { &mut *rv_ptr }
}

fn saturate_free_regions(rv: &mut Rendezvous<'_, '_, FailingTransport>) {
    let mut idx = 0usize;
    while idx < FREE_REGION_CAPACITY {
        rv.free_regions[idx] = FreeRegion::recorded(1024 + (idx as u32) * 8, 1);
        idx += 1;
    }
}

#[test]
fn assoc_sidecar_reclaims_alignment_padding() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);

    let prefix = rv
        .allocate_external_persistent_sidecar_bytes(1, 1)
        .expect("prefix sidecar");
    assert_eq!(prefix.bytes(), 1);
    rv.ensure_core_lane_tables_for_lane_slots(1)
        .expect("initial assoc storage");
    let first_assoc = rv.assoc_storage;
    assert!(
        first_assoc.reclaim_delta() > 0,
        "prefix allocation must force assoc alignment padding"
    );
    let expected_reclaim_offset = rv.reclaim_offset_for_sidecar(first_assoc);
    rv.ensure_core_lane_tables_for_lane_slots(2)
        .expect("grown assoc storage");

    assert!(
        rv.free_regions.iter().any(|region| {
            region.is_recorded()
                && region.offset == expected_reclaim_offset
                && region.len as usize == first_assoc.bytes() + first_assoc.reclaim_delta()
        }),
        "assoc resize must reclaim payload bytes plus alignment padding"
    );
}

#[test]
fn sidecar_release_saturation_fails_closed() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.set_image_frontier(4096);

    let mut idx = 0usize;
    while idx < FREE_REGION_CAPACITY {
        rv.free_regions[idx] = FreeRegion::recorded(16 + (idx as u32) * 16, 1);
        idx += 1;
    }
    let before = rv.free_regions;

    assert!(
        rv.release_persistent_region(2048, 8).is_err(),
        "saturated non-frontier release must fail closed"
    );
    assert_eq!(
        rv.free_regions, before,
        "failed release must not mutate free-region authority"
    );
}

#[test]
fn assoc_replacement_release_failure_keeps_published_storage() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_lane_slots(1)
        .expect("initial assoc storage");
    rv.set_image_frontier(2048);
    saturate_free_regions(rv);

    let before_storage = rv.assoc_storage;
    let before_range = rv.lane_range.clone();
    let before_frontier = rv.image_frontier;

    assert_eq!(
        rv.ensure_core_lane_tables_for_lane_slots(2),
        Err(ResourceScope::LaneStorage),
        "assoc growth must fail closed when source sidecar release cannot be recorded"
    );
    assert_eq!(rv.assoc_storage.ptr(), before_storage.ptr());
    assert_eq!(rv.assoc_storage.bytes(), before_storage.bytes());
    assert_eq!(
        rv.assoc_storage.reclaim_delta(),
        before_storage.reclaim_delta()
    );
    assert_eq!(rv.lane_range, before_range);
    assert_eq!(rv.image_frontier, before_frontier);
}

#[test]
fn route_replacement_release_failure_keeps_published_storage() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(1, 1)
        .expect("initial route storage");
    rv.set_image_frontier(2048);
    saturate_free_regions(rv);

    let before_storage = rv.route_storage;
    let before_route_slots = rv.routes.route_slots();
    let before_lane_slots = rv.routes.lane_slots();
    let before_frontier = rv.image_frontier;

    assert_eq!(
        rv.ensure_route_table_capacity(2, 1),
        Err(ResourceScope::RouteTable),
        "route growth must fail closed when source sidecar release cannot be recorded"
    );
    assert_eq!(rv.route_storage.ptr(), before_storage.ptr());
    assert_eq!(rv.route_storage.bytes(), before_storage.bytes());
    assert_eq!(
        rv.route_storage.reclaim_delta(),
        before_storage.reclaim_delta()
    );
    assert_eq!(rv.routes.route_slots(), before_route_slots);
    assert_eq!(rv.routes.lane_slots(), before_lane_slots);
    assert_eq!(rv.image_frontier, before_frontier);
}
