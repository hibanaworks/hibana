use super::*;
use crate::{
    global::const_dsl::{ScopeId, ScopeKind},
    runtime_core::resources::RuntimeResources,
    session::types::RendezvousId,
    transport::{Outgoing, PortOpen, ReceivedFrame, TransportError},
};
use core::{
    cell::Cell,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

struct WakerCounts {
    clones: Cell<usize>,
    wakes: Cell<usize>,
}

unsafe fn clone_counting_waker(data: *const ()) -> RawWaker {
    let counts = /* SAFETY: counting_waker stores a live WakerCounts pointer. */ unsafe {
        &*data.cast::<WakerCounts>()
    };
    counts.clones.set(counts.clones.get() + 1);
    RawWaker::new(data, &COUNTING_WAKER_VTABLE)
}

unsafe fn wake_counting_waker(data: *const ()) {
    let counts = /* SAFETY: counting_waker stores a live WakerCounts pointer. */ unsafe {
        &*data.cast::<WakerCounts>()
    };
    counts.wakes.set(counts.wakes.get() + 1);
}

unsafe fn drop_counting_waker(_: *const ()) {}

static COUNTING_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_counting_waker,
    wake_counting_waker,
    wake_counting_waker,
    drop_counting_waker,
);

fn counting_waker(counts: &WakerCounts) -> Waker {
    let data = core::ptr::from_ref(counts).cast::<()>();
    /* SAFETY: every test Waker is dropped before its stack-resident counts. */
    unsafe { Waker::from_raw(RawWaker::new(data, &COUNTING_WAKER_VTABLE)) }
}

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
#[should_panic]
fn nonempty_sidecar_before_slab_fails_closed() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let (slab_ptr, _) = rv.slab_ptr_and_len();
    let malformed = Sidecar::from_raw_parts(slab_ptr.wrapping_sub(1), 1);

    core::hint::black_box(rv.sidecar_range(malformed));
}

#[test]
fn assoc_replacement_retires_old_root_without_history_growth() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("initial assoc storage");
    let mut slots = 2usize;
    while slots <= 24 {
        let before = rv.assoc_storage.get();
        rv.ensure_core_lane_tables_for_assoc_entries(slots, slots)
            .expect("grown assoc storage");
        let current = rv.assoc_storage.get();
        assert_eq!(current.ptr(), before.ptr());
        assert!(current.bytes() > before.bytes());
        let replacement_bound = current
            .bytes()
            .checked_add(AssocTable::storage_align() - 1)
            .expect("replacement bound");
        assert!(
            rv.image_frontier.get() as usize <= replacement_bound,
            "assoc frontier must depend on live/replacement bytes, not growth count"
        );
        slots += 1;
    }
    assert_eq!(rv.lane_end.get() - rv.lane_base.get(), 24);
}

#[test]
fn route_replacement_retires_old_root_without_history_growth() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(1, 1)
        .expect("initial route storage");
    let mut slots = 2usize;
    while slots <= 24 {
        let before = rv.route_storage.get();
        rv.ensure_route_table_capacity(slots, 1)
            .expect("grown route storage");
        let current = rv.route_storage.get();
        assert_eq!(current.ptr(), before.ptr());
        assert!(current.bytes() > before.bytes());
        let replacement_bound = current
            .bytes()
            .checked_add(RouteTable::storage_align() - 1)
            .expect("replacement bound");
        assert!(
            rv.image_frontier.get() as usize <= replacement_bound,
            "route frontier must depend on live/replacement bytes, not growth count"
        );
        slots += 1;
    }
    assert_eq!(rv.routes.route_slots(), 24);
    assert_eq!(rv.routes.lane_slots(), 1);
}

#[test]
fn assoc_growth_moves_waiter_without_clone_or_loss() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(2, 2)
        .expect("assoc storage");
    let sid0 = crate::session::types::SessionId::new(1);
    let sid1 = crate::session::types::SessionId::new(2);
    rv.activate_lane_attachment(sid0, crate::session::types::Lane::new(0))
        .expect("first association");
    rv.activate_lane_attachment(sid1, crate::session::types::Lane::new(1))
        .expect("overflow association");
    let counts = WakerCounts {
        clones: Cell::new(0),
        wakes: Cell::new(0),
    };
    let waker = counting_waker(&counts);
    rv.register_session_waiter(sid1, crate::session::types::Lane::new(1), &waker);
    assert_eq!(counts.clones.get(), 1);

    rv.ensure_core_lane_tables_for_assoc_entries(3, 3)
        .expect("grown assoc storage");
    assert_eq!(counts.clones.get(), 1, "migration must move, not clone");
    rv.poison_session(
        sid1,
        crate::rendezvous::SessionFaultKind::ProgressInvariantViolated,
    );
    assert_eq!(counts.wakes.get(), 1);
}

#[test]
fn route_growth_moves_waiter_without_clone_or_loss() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(1, 1).expect("route storage");
    let counts = WakerCounts {
        clones: Cell::new(0),
        wakes: Cell::new(0),
    };
    let waker = counting_waker(&counts);
    let mut context = Context::from_waker(&waker);
    let lane = crate::session::types::Lane::new(0);
    let scope = ScopeId::new(ScopeKind::Route, 0);
    assert!(
        rv.routes
            .poll_with_role_count(lane, 2, 0, scope, &mut context)
            .is_pending()
    );
    assert_eq!(counts.clones.get(), 1);

    rv.ensure_route_table_capacity(2, 1)
        .expect("grown route storage");
    assert_eq!(counts.clones.get(), 1, "migration must move, not clone");
    rv.routes.record_with_role_count(lane, 2, 1, scope, 0);
    assert_eq!(counts.wakes.get(), 1);
}
