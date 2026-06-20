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
    ) -> Poll<Result<(), TransportError>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Ready(Err(TransportError::Failed))
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
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

#[test]
fn sidecar_release_reclaims_only_frontier_suffix() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);

    let first = rv
        .allocate_external_persistent_sidecar_bytes(8, 1)
        .expect("first sidecar");
    let second = rv
        .allocate_external_persistent_sidecar_bytes(8, 1)
        .expect("second sidecar");
    let after_second = rv.image_frontier;

    rv.release_external_persistent_sidecar(first);
    assert_eq!(
        rv.image_frontier, after_second,
        "interior sidecar release must not create a hidden free-list fragment"
    );

    rv.release_external_persistent_sidecar(second);
    assert!(
        rv.image_frontier < after_second,
        "frontier suffix release must shrink the monotonic sidecar arena"
    );
}

#[test]
fn assoc_replacement_publishes_without_free_list_branch() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("initial assoc storage");
    let before_storage = rv.assoc_storage;

    rv.ensure_core_lane_tables_for_assoc_entries(2, 2)
        .expect("grown assoc storage");

    assert_ne!(rv.assoc_storage.ptr(), before_storage.ptr());
    assert!(rv.assoc_storage.bytes() > before_storage.bytes());
    assert_eq!(rv.lane_range.end - rv.lane_range.start, 2);
}

#[test]
fn route_replacement_publishes_without_free_list_branch() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(1, 1)
        .expect("initial route storage");
    let before_storage = rv.route_storage;

    rv.ensure_route_table_capacity(2, 1)
        .expect("grown route storage");

    assert_ne!(rv.route_storage.ptr(), before_storage.ptr());
    assert!(rv.route_storage.bytes() > before_storage.bytes());
    assert_eq!(rv.routes.route_slots(), 2);
    assert_eq!(rv.routes.lane_slots(), 1);
}
