use core::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::rc::Rc;

use hibana::{
    g::{self, Msg},
    runtime::{
        RendezvousKit, SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
        wire::Payload,
    },
};

#[path = "miri_runtime_owner/callback_reentry.rs"]
mod callback_reentry;

const OUTER_RESOLVER: u16 = 701;
const INNER_RESOLVER: u16 = 702;
const SHARED_SITE_RESOLVER: u16 = 703;

fn choose_arm(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}

struct NoopTransport;
struct NoopTx;
struct NoopRx;

struct CountedMalformedTransport {
    drops: Rc<Cell<usize>>,
}

struct CountedTransportHandle {
    drops: Rc<Cell<usize>>,
}

impl Drop for CountedTransportHandle {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

impl Transport for CountedMalformedTransport {
    type Tx<'a> = CountedTransportHandle;
    type Rx<'a> = CountedTransportHandle;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            CountedTransportHandle {
                drops: Rc::clone(&self.drops),
            },
            CountedTransportHandle {
                drops: Rc::clone(&self.drops),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Ready(Ok(ReceivedFrame::deterministic(Payload::new(&[]))))
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

struct TransportHandleDropState {
    context: Cell<*const TransportHandleDropContext>,
    fired: Cell<bool>,
}

impl TransportHandleDropState {
    fn new() -> Self {
        Self {
            context: Cell::new(core::ptr::null()),
            fired: Cell::new(false),
        }
    }

    fn arm(&self, context: &TransportHandleDropContext) {
        if !self.context.get().is_null() || self.fired.get() {
            panic!("transport handle drop state armed twice");
        }
        self.context.set(core::ptr::from_ref(context));
    }

    fn fire_if_armed(&self) {
        let context = self.context.get();
        if context.is_null() || self.fired.replace(true) {
            return;
        }
        /* SAFETY: the test arms this state with a live stack context before
        dropping the endpoint and keeps the context live until both transport
        handles have run their destructors. */
        unsafe { (&*context).attempt_attach() };
    }
}

struct TransportHandleDropContext {
    rendezvous: *const (),
    program: *const RoleProgram<1>,
    sid: SessionId,
    rejected_busy: Cell<bool>,
}

impl TransportHandleDropContext {
    unsafe fn attempt_attach(&self) {
        let rendezvous = /* SAFETY: the test stores the matching live
        rendezvous witness and keeps its kit and slab resident. */ unsafe {
            &*self
                .rendezvous
                .cast::<RendezvousKit<'static, 'static, ReentrantHandleDropTransport>>()
        };
        let program = /* SAFETY: the test stores the live role-1 projection for
        the duration of transport-handle destruction. */ unsafe { &*self.program };
        match rendezvous.enter(self.sid, program) {
            Ok(endpoint) => {
                drop(endpoint);
                panic!("transport handle drop attached through a partial endpoint drop");
            }
            Err(error) => {
                self.rejected_busy
                    .set(format!("{error:?}").contains("rv-busy"));
            }
        }
    }
}

struct ReentrantHandleDropTransport {
    state: Rc<TransportHandleDropState>,
}

struct ReentrantTransportHandle {
    state: Rc<TransportHandleDropState>,
}

impl Drop for ReentrantTransportHandle {
    fn drop(&mut self) {
        self.state.fire_if_armed();
    }
}

impl Transport for ReentrantHandleDropTransport {
    type Tx<'a> = ReentrantTransportHandle;
    type Rx<'a> = ReentrantTransportHandle;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            ReentrantTransportHandle {
                state: Rc::clone(&self.state),
            },
            ReentrantTransportHandle {
                state: Rc::clone(&self.state),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

impl Transport for NoopTransport {
    type Tx<'a> = NoopTx;
    type Rx<'a> = NoopRx;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (NoopTx, NoopRx)
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::send::<0, 1, Msg<1, u32>>())
}

fn fanout_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::par(
        g::send::<0, 1, Msg<2, u32>>(),
        g::send::<0, 2, Msg<3, u32>>(),
    ))
}

#[test]
fn fatal_codec_error_retires_transport_handles_before_endpoint_drop() {
    let role1 = program::<1>();
    let drops = Rc::new(Cell::new(0));
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<CountedMalformedTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(
            &mut slab.0,
            CountedMalformedTransport {
                drops: Rc::clone(&drops),
            },
        )
        .expect("register rendezvous");
    let mut endpoint = rv
        .enter(SessionId::new(22), &role1)
        .expect("attach receiver");

    let error = futures::executor::block_on(endpoint.recv::<Msg<1, u32>>())
        .expect_err("empty u32 payload must fail validation");

    assert!(format!("{error:?}").contains("Truncated"));
    assert_eq!(
        drops.get(),
        2,
        "fatal error must retire both transport handles before endpoint drop"
    );
    drop(endpoint);
    assert_eq!(
        drops.get(),
        2,
        "endpoint drop must not retire handles twice"
    );
}

fn nested_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::route(
                g::send::<0, 1, Msg<41, u32>>(),
                g::send::<0, 1, Msg<42, u32>>(),
            )
            .resolve::<INNER_RESOLVER>(),
            g::send::<0, 1, Msg<43, u32>>(),
        )
        .resolve::<OUTER_RESOLVER>(),
    )
}

fn wide_roll_shared_site_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::seq(
        g::route(
            g::send::<0, 1, Msg<44, u32>>(),
            g::send::<0, 1, Msg<45, u32>>(),
        )
        .resolve::<SHARED_SITE_RESOLVER>(),
        g::seq(
            g::send::<0, 1, Msg<50, u32>>(),
            g::send::<0, 1, Msg<51, u32>>(),
        )
        .roll(),
    ))
}

fn narrow_roll_shared_site_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::seq(
        g::route(
            g::send::<0, 1, Msg<44, u32>>(),
            g::send::<0, 1, Msg<45, u32>>(),
        )
        .resolve::<SHARED_SITE_RESOLVER>(),
        g::seq(
            g::send::<0, 1, Msg<50, u32>>().roll(),
            g::send::<0, 1, Msg<51, u32>>(),
        ),
    ))
}

#[repr(align(16))]
struct AlignedSlab([u8; 65_536]);

#[test]
fn public_runtime_owner_stays_alias_clean_across_multiple_attaches() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let endpoint0 = rv.enter(SessionId::new(1), &role0).expect("attach role 0");
    let endpoint1 = rv.enter(SessionId::new(1), &role1).expect("attach role 1");
    drop(endpoint0);
    assert!(rv.enter(SessionId::new(1), &role0).is_err());
    let endpoint2 = rv
        .enter(SessionId::new(2), &role0)
        .expect("attach fresh session after rejected poisoned reentry");

    core::hint::black_box((&endpoint1, &endpoint2));
}

#[test]
fn session_generation_rejects_mixed_program_images_before_mutation() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let unrelated_role1 = fanout_program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(34);
    let mut origin = rv.enter(sid, &role0).expect("attach program origin");

    let mismatch = match rv.enter(sid, &unrelated_role1) {
        Ok(_) => panic!("one session generation must not mix program images"),
        Err(error) => error,
    };
    assert!(
        format!("{mismatch:?}").contains("session-program-mismatch 34"),
        "mixed-image attach must identify the session binding: {mismatch:?}"
    );

    let peer = rv
        .enter(sid, &role1)
        .expect("rejected attach must leave the original session binding unchanged");
    futures::executor::block_on(origin.send::<Msg<1, u32>>(&34))
        .expect("original endpoint must remain live after rejected attach");
    core::hint::black_box(peer);
}

#[test]
fn session_generation_cannot_split_across_rendezvous_owners() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut first_slab = AlignedSlab([0; 65_536]);
    let mut second_slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let first = kit
        .rendezvous(&mut first_slab.0, NoopTransport)
        .expect("register first rendezvous");
    let second = kit
        .rendezvous(&mut second_slab.0, NoopTransport)
        .expect("register second rendezvous");
    let sid = SessionId::new(35);
    let origin = first.enter(sid, &role0).expect("attach session owner");

    let mismatch = match second.enter(sid, &role1) {
        Ok(_) => panic!("one session generation must have one rendezvous owner"),
        Err(error) => error,
    };
    assert!(
        format!("{mismatch:?}").contains("rv-mismatch"),
        "cross-rendezvous attach must fail at owner identity: {mismatch:?}"
    );

    let peer = first
        .enter(sid, &role1)
        .expect("failed cross-owner attach must not consume the role lease");
    core::hint::black_box((origin, peer));
}

#[test]
fn transport_handle_drop_cannot_reenter_partial_endpoint_drop() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let state = Rc::new(TransportHandleDropState::new());
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<ReentrantHandleDropTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantHandleDropTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(36);
    let mut origin = rendezvous.enter(sid, &role0).expect("attach origin");
    futures::executor::block_on(origin.send::<Msg<1, u32>>(&36))
        .expect("complete origin before drop reentry");
    let context = TransportHandleDropContext {
        rendezvous: core::ptr::from_ref(&rendezvous).cast(),
        program: core::ptr::from_ref(&role1),
        sid,
        rejected_busy: Cell::new(false),
    };
    state.arm(&context);

    drop(origin);

    assert!(state.fired.get());
    assert!(context.rejected_busy.get());
    let peer = rendezvous
        .enter(sid, &role1)
        .expect("drop barrier must release after endpoint destruction");
    core::hint::black_box(peer);
}

#[test]
fn resolver_replacement_survives_sidecar_relocation_and_typed_dispatch() {
    let outer = DecisionArm::Left;
    let inner = DecisionArm::Right;
    let role0 = nested_resolver_program::<0>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");

    rv.set_resolver(
        &role0,
        ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, choose_arm),
    )
    .expect("install outer resolver before sidecar relocation");
    let mut endpoint = rv
        .enter(SessionId::new(3), &role0)
        .expect("attach endpoint and relocate resolver sidecar");
    rv.set_resolver(
        &role0,
        ResolverRef::<INNER_RESOLVER>::decision_state(&inner, choose_arm),
    )
    .expect("grow resolver table after endpoint sidecars exist");

    futures::executor::block_on(endpoint.send::<Msg<42, u32>>(&42))
        .expect("dispatch relocated outer and replaced inner resolver entries");
}

#[test]
fn resolver_registration_keeps_distinct_program_image_identity() {
    let left = DecisionArm::Left;
    let right = DecisionArm::Right;
    let first = wide_roll_shared_site_program::<0>();
    let first_registration = wide_roll_shared_site_program::<1>();
    let second = narrow_roll_shared_site_program::<0>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");

    rv.set_resolver(
        &first_registration,
        ResolverRef::<SHARED_SITE_RESOLVER>::decision_state(&left, choose_arm),
    )
    .expect("install first program resolver through another role projection");
    rv.set_resolver(
        &second,
        ResolverRef::<SHARED_SITE_RESOLVER>::decision_state(&right, choose_arm),
    )
    .expect("install second program resolver at the same scope and resolver id");

    let mut first_endpoint = rv
        .enter(SessionId::new(31), &first)
        .expect("attach first program");
    let mut second_endpoint = rv
        .enter(SessionId::new(32), &second)
        .expect("attach second program");
    futures::executor::block_on(first_endpoint.send::<Msg<44, u32>>(&44))
        .expect("first program keeps its left resolver");
    futures::executor::block_on(second_endpoint.send::<Msg<45, u32>>(&45))
        .expect("second program keeps its right resolver");
}

#[test]
fn failed_resolver_growth_preserves_existing_registration_and_dispatch() {
    let left = DecisionArm::Left;
    let right = DecisionArm::Right;
    let first = wide_roll_shared_site_program::<0>();
    let second = narrow_roll_shared_site_program::<0>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0[..1953], NoopTransport)
        .expect("register constrained rendezvous");
    rv.set_resolver(
        &first,
        ResolverRef::<SHARED_SITE_RESOLVER>::decision_state(&left, choose_arm),
    )
    .expect("install first resolver before exhausting resident storage");
    let mut endpoint = rv
        .enter(SessionId::new(33), &first)
        .expect("attach endpoint before exhausting resident storage");
    let growth_error = rv
        .set_resolver(
            &second,
            ResolverRef::<SHARED_SITE_RESOLVER>::decision_state(&right, choose_arm),
        )
        .expect_err("second resolver registration must reach the capacity failure boundary");
    assert!(
        format!("{growth_error:?}").contains("Cluster(exhausted resolver)"),
        "fixture must fail at resolver storage exhaustion: {growth_error:?}"
    );

    futures::executor::block_on(endpoint.send::<Msg<44, u32>>(&44))
        .expect("failed growth must preserve the first resolver registration");
}

#[test]
fn failed_peer_attach_abort_keeps_existing_endpoint_live() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut exercised = false;

    for slab_bytes in (1024usize..=8192).step_by(128) {
        let mut slab = AlignedSlab([0; 65_536]);
        let mut storage = SessionKitStorage::<NoopTransport>::uninit();
        let kit = storage.init();
        let rv = kit
            .rendezvous(&mut slab.0[..slab_bytes], NoopTransport)
            .expect("register constrained rendezvous");
        let sid = SessionId::new(4);
        let Ok(mut endpoint0) = rv.enter(sid, &role0) else {
            continue;
        };
        if rv.enter(sid, &role1).is_ok() {
            continue;
        }

        futures::executor::block_on(endpoint0.send::<Msg<1, u32>>(&4))
            .expect("existing endpoint survives failed peer attach abort");
        exercised = true;
        break;
    }

    assert!(
        exercised,
        "Miri fixture must reach a failed peer attach after the first endpoint is live"
    );
}

#[test]
fn payload_schema_mismatch_poison_session_without_publishing() {
    let role0 = program::<0>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register send rendezvous");
    let mut endpoint = rv.enter(SessionId::new(6), &role0).expect("attach sender");

    let mismatch = futures::executor::block_on(endpoint.send::<Msg<1, i32>>(&7))
        .expect_err("same-width payload with the wrong schema must be rejected");
    assert!(
        format!("{mismatch:?}").contains("SchemaMismatch"),
        "wrong payload schema must remain distinguishable: {mismatch:?}"
    );
    let poisoned = futures::executor::block_on(endpoint.send::<Msg<1, u32>>(&7))
        .expect_err("schema rejection must poison the affine session");
    assert!(
        format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"),
        "the first schema mismatch must remain the session fault: {poisoned:?}"
    );
}

struct ReceiveState {
    bytes: [u8; 4],
    polls: Cell<usize>,
    requeues: Cell<usize>,
}

struct ReceiveTransport {
    state: Rc<ReceiveState>,
}

struct ReceiveRx {
    delivered: bool,
}

impl Transport for ReceiveTransport {
    type Tx<'a> = NoopTx;
    type Rx<'a> = ReceiveRx;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (NoopTx, ReceiveRx { delivered: false })
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.state.polls.set(self.state.polls.get() + 1);
        if rx.delivered {
            return Poll::Pending;
        }
        rx.delivered = true;
        Poll::Ready(Ok(ReceivedFrame::deterministic(Payload::new(
            &self.state.bytes,
        ))))
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        if !rx.delivered {
            panic!("receive transport requeued without a delivered frame");
        }
        rx.delivered = false;
        self.state.requeues.set(self.state.requeues.get() + 1);
        Ok(())
    }
}

#[test]
fn receive_receipt_resolves_valid_borrowed_frame() {
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(ReceiveState {
        bytes: 0x1122_3344u32.to_be_bytes(),
        polls: Cell::new(0),
        requeues: Cell::new(0),
    });
    let mut storage = SessionKitStorage::<ReceiveTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(
            &mut slab.0,
            ReceiveTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register receive rendezvous");
    let mut endpoint = rv
        .enter(SessionId::new(5), &role1)
        .expect("attach receiver");

    assert_eq!(
        futures::executor::block_on(endpoint.recv::<Msg<1, u32>>())
            .expect("borrowed receive frame is valid"),
        0x1122_3344
    );
    assert_eq!(state.polls.get(), 1);
    assert_eq!(state.requeues.get(), 0);
}

#[test]
fn payload_schema_mismatch_precedes_receive_transport_poll() {
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(ReceiveState {
        bytes: 0x1122_3344u32.to_be_bytes(),
        polls: Cell::new(0),
        requeues: Cell::new(0),
    });
    let mut storage = SessionKitStorage::<ReceiveTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(
            &mut slab.0,
            ReceiveTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register receive rendezvous");
    let mut endpoint = rv
        .enter(SessionId::new(7), &role1)
        .expect("attach receiver");

    let mismatch = futures::executor::block_on(endpoint.recv::<Msg<1, i32>>())
        .expect_err("same-width payload with the wrong schema must be rejected");
    assert!(
        format!("{mismatch:?}").contains("SchemaMismatch"),
        "wrong payload schema must remain distinguishable: {mismatch:?}"
    );
    assert_eq!(
        state.polls.get(),
        0,
        "schema validation must precede the transport callback"
    );
    assert_eq!(
        state.requeues.get(),
        0,
        "schema validation must not consume or requeue a frame"
    );
}

struct DeferredState {
    ready: Cell<bool>,
    first: Cell<[u8; 4]>,
    second: Cell<[u8; 4]>,
}

struct DeferredTransport {
    state: Rc<DeferredState>,
}

struct DeferredTx {
    sid: u32,
}

impl Transport for DeferredTransport {
    type Tx<'a> = DeferredTx;
    type Rx<'a> = NoopRx;

    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            DeferredTx {
                sid: port.session_id().raw(),
            },
            NoopRx,
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        if !self.state.ready.get() {
            return Poll::Pending;
        }
        let bytes: [u8; 4] = outgoing
            .payload()
            .as_bytes()
            .try_into()
            .expect("u32 payload");
        match tx.sid {
            10 => self.state.first.set(bytes),
            11 => self.state.second.set(bytes),
            _ => panic!("unexpected session"),
        }
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

#[test]
fn pending_sends_do_not_borrow_shared_scratch_across_polls() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DeferredState {
        ready: Cell::new(false),
        first: Cell::new([0; 4]),
        second: Cell::new([0; 4]),
    });
    let mut storage = SessionKitStorage::<DeferredTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(
            &mut slab.0,
            DeferredTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register rendezvous");
    let mut sender0 = rv.enter(SessionId::new(10), &role0).expect("sender 0");
    let receiver0 = rv.enter(SessionId::new(10), &role1).expect("receiver 0");
    let mut sender1 = rv.enter(SessionId::new(11), &role0).expect("sender 1");
    let receiver1 = rv.enter(SessionId::new(11), &role1).expect("receiver 1");
    let first = 0x1122_3344u32;
    let second = 0xaabb_ccddu32;
    let mut send0 = pin!(sender0.send::<Msg<1, u32>>(&first));
    let mut send1 = pin!(sender1.send::<Msg<1, u32>>(&second));
    let waker = futures::task::noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    assert!(send0.as_mut().poll(&mut cx).is_pending());
    assert!(send1.as_mut().poll(&mut cx).is_pending());
    state.ready.set(true);
    assert!(matches!(send0.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    assert!(matches!(send1.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(state.first.get(), first.to_be_bytes());
    assert_eq!(state.second.get(), second.to_be_bytes());
    core::hint::black_box((receiver0, receiver1));
}

unsafe fn clone_counting_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &COUNTING_WAKER_VTABLE)
}

unsafe fn wake_counting_waker(data: *const ()) {
    let count = unsafe {
        // SAFETY: `counting_waker` stores a live `Cell` pointer and the test
        // drops every derived Waker before that stack cell leaves scope.
        &*data.cast::<Cell<usize>>()
    };
    count.set(count.get() + 1);
}

unsafe fn drop_counting_waker(_: *const ()) {}

static COUNTING_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_counting_waker,
    wake_counting_waker,
    wake_counting_waker,
    drop_counting_waker,
);

fn counting_waker(count: &Cell<usize>) -> Waker {
    let data = core::ptr::from_ref(count).cast::<()>();
    unsafe {
        // SAFETY: the vtable preserves the same live `Cell` pointer without
        // owning it, and this test consumes the Waker before `count` is dropped.
        Waker::from_raw(RawWaker::new(data, &COUNTING_WAKER_VTABLE))
    }
}

struct DropEndpointState {
    target: Cell<*mut ()>,
    drop_target: Cell<Option<unsafe fn(*mut ())>>,
    fired: Cell<bool>,
}

impl DropEndpointState {
    fn empty() -> Self {
        Self {
            target: Cell::new(core::ptr::null_mut()),
            drop_target: Cell::new(None),
            fired: Cell::new(false),
        }
    }

    fn arm<T>(&self, target: &mut Option<T>) {
        if self.drop_target.get().is_some() || self.fired.get() {
            panic!("drop endpoint state armed twice");
        }
        self.target.set(core::ptr::from_mut(target).cast::<()>());
        self.drop_target.set(Some(drop_option::<T>));
    }

    fn fire(&self) {
        if self.fired.replace(true) {
            return;
        }
        let drop_target = self.drop_target.get().expect("armed drop callback");
        let target = self.target.get();
        if target.is_null() {
            panic!("armed drop target");
        }
        unsafe {
            // SAFETY: `arm` pairs the live target pointer with its
            // monomorphized callback and `fired` grants one invocation.
            drop_target(target);
        }
    }

    fn fire_if_armed(&self) {
        if self.drop_target.get().is_some() {
            self.fire();
        }
    }
}

unsafe fn drop_option<T>(target: *mut ()) {
    let target = unsafe {
        // SAFETY: `DropEndpointState::arm` stores the unique pointer to the
        // live test-owned `Option<T>` and `fire` invokes this at most once.
        &mut *target.cast::<Option<T>>()
    };
    drop(target.take());
}

unsafe fn clone_drop_on_clone_waker(data: *const ()) -> RawWaker {
    let state = unsafe {
        // SAFETY: `drop_on_clone_waker` stores the live test state pointer and
        // every derived Waker is removed before that state leaves scope.
        &*data.cast::<DropEndpointState>()
    };
    state.fire();
    RawWaker::new(data, &DROP_ON_CLONE_WAKER_VTABLE)
}

unsafe fn ignore_drop_on_clone_waker(_: *const ()) {}

static DROP_ON_CLONE_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_drop_on_clone_waker,
    ignore_drop_on_clone_waker,
    ignore_drop_on_clone_waker,
    ignore_drop_on_clone_waker,
);

fn drop_on_clone_waker(state: &DropEndpointState) -> Waker {
    let data = core::ptr::from_ref(state).cast::<()>();
    unsafe {
        // SAFETY: the state outlives the Waker and all clones installed into
        // endpoint storage are cleared before this test returns.
        Waker::from_raw(RawWaker::new(data, &DROP_ON_CLONE_WAKER_VTABLE))
    }
}

unsafe fn clone_drop_on_drop_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &DROP_ON_DROP_WAKER_VTABLE)
}

unsafe fn drop_drop_on_drop_waker(data: *const ()) {
    let state = unsafe {
        // SAFETY: `drop_on_drop_waker` stores the live state pointer and all
        // derived Wakers are removed before that state leaves scope.
        &*data.cast::<DropEndpointState>()
    };
    state.fire();
}

static DROP_ON_DROP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_drop_on_drop_waker,
    ignore_drop_on_clone_waker,
    ignore_drop_on_clone_waker,
    drop_drop_on_drop_waker,
);

fn drop_on_drop_waker(state: &DropEndpointState) -> Waker {
    let data = core::ptr::from_ref(state).cast::<()>();
    unsafe {
        // SAFETY: the state outlives the Waker and every clone installed into
        // the lease record is removed before this test returns.
        Waker::from_raw(RawWaker::new(data, &DROP_ON_DROP_WAKER_VTABLE))
    }
}

unsafe fn clone_drop_on_wake_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &DROP_ON_WAKE_WAKER_VTABLE)
}

unsafe fn wake_drop_on_wake_waker(data: *const ()) {
    let state = unsafe {
        // SAFETY: `drop_on_wake_waker` stores the live state pointer and every
        // derived Waker is consumed before that state leaves scope.
        &*data.cast::<DropEndpointState>()
    };
    state.fire();
}

static DROP_ON_WAKE_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_drop_on_wake_waker,
    wake_drop_on_wake_waker,
    wake_drop_on_wake_waker,
    ignore_drop_on_clone_waker,
);

fn drop_on_wake_waker(state: &DropEndpointState) -> Waker {
    let data = core::ptr::from_ref(state).cast::<()>();
    unsafe {
        // SAFETY: the state outlives this Waker and the lease-record clone
        // consumed by the wake path.
        Waker::from_raw(RawWaker::new(data, &DROP_ON_WAKE_WAKER_VTABLE))
    }
}

#[test]
fn reentrant_waker_clone_fault_is_observed_before_endpoint_borrow() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(19);
    let mut origin = Some(rv.enter(sid, &role0).expect("origin"));
    let mut target = rv.enter(sid, &role1).expect("target");
    let mut recv = pin!(target.recv::<Msg<1, u32>>());
    let state = DropEndpointState::empty();
    state.arm(&mut origin);
    let waker = drop_on_clone_waker(&state);
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(recv.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
    assert!(state.fired.get());
    assert!(origin.is_none());
}

#[test]
fn displaced_waker_drop_reenters_only_after_endpoint_borrow_ends() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(18);
    let mut origin = Some(rv.enter(sid, &role0).expect("origin"));
    let mut target = rv.enter(sid, &role1).expect("target");
    let mut recv = pin!(target.recv::<Msg<1, u32>>());
    let state = DropEndpointState::empty();
    state.arm(&mut origin);
    let first_waker = drop_on_drop_waker(&state);
    let mut first_cx = Context::from_waker(&first_waker);
    assert!(recv.as_mut().poll(&mut first_cx).is_pending());

    let wake_count = Cell::new(0);
    let replacement = counting_waker(&wake_count);
    let mut replacement_cx = Context::from_waker(&replacement);
    assert!(recv.as_mut().poll(&mut replacement_cx).is_pending());
    assert!(state.fired.get());
    assert!(origin.is_none());
    assert_eq!(wake_count.get(), 1);
    assert!(matches!(
        recv.as_mut().poll(&mut replacement_cx),
        Poll::Ready(Err(_))
    ));
}

struct ReentrantRecvTransport {
    state: Rc<DropEndpointState>,
}

impl Transport for ReentrantRecvTransport {
    type Tx<'a> = NoopTx;
    type Rx<'a> = NoopRx;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (NoopTx, NoopRx)
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.state.fire();
        Poll::Pending
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

#[test]
fn transport_callback_reentry_uses_lease_waiter_outside_endpoint_storage() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let mut storage = SessionKitStorage::<ReentrantRecvTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(
            &mut slab.0,
            ReentrantRecvTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(17);
    let mut origin = Some(rv.enter(sid, &role0).expect("origin"));
    let mut target = rv.enter(sid, &role1).expect("target");
    state.arm(&mut origin);
    let mut recv = pin!(target.recv::<Msg<1, u32>>());
    let wake_count = Cell::new(0);
    let waker = counting_waker(&wake_count);
    let mut cx = Context::from_waker(&waker);

    let error = match recv.as_mut().poll(&mut cx) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(payload)) => {
            core::hint::black_box(payload);
            panic!("reentrant transport recv committed after dropping its peer");
        }
        Poll::Pending => panic!("reentrant transport recv deferred an already published fault"),
    };
    assert!(state.fired.get());
    assert!(origin.is_none());
    assert_eq!(wake_count.get(), 1);
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn endpoint_waiter_survives_unrelated_growth_and_wakes_only_its_session() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(20);
    let origin = rv.enter(sid, &role0).expect("origin");
    let mut target = rv.enter(sid, &role1).expect("target");
    let mut recv = pin!(target.recv::<Msg<1, u32>>());
    let wake_count = Cell::new(0);
    let waker = counting_waker(&wake_count);
    let mut cx = Context::from_waker(&waker);
    assert!(recv.as_mut().poll(&mut cx).is_pending());

    let unrelated_sid = SessionId::new(21);
    let unrelated_origin = rv.enter(unrelated_sid, &role0).expect("unrelated origin");
    let unrelated_target = rv.enter(unrelated_sid, &role1).expect("unrelated target");
    drop(unrelated_origin);
    drop(unrelated_target);
    assert_eq!(wake_count.get(), 0);

    drop(origin);
    assert_eq!(wake_count.get(), 1);
    assert!(matches!(recv.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
}

#[test]
fn endpoint_drop_wakes_every_published_peer_waiter() {
    let role0 = fanout_program::<0>();
    let role1 = fanout_program::<1>();
    let role2 = fanout_program::<2>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(30);
    let origin = rv.enter(sid, &role0).expect("origin");
    let mut peer1 = rv.enter(sid, &role1).expect("peer 1");
    let mut peer2 = rv.enter(sid, &role2).expect("peer 2");
    let mut recv1 = pin!(peer1.recv::<Msg<2, u32>>());
    let mut recv2 = pin!(peer2.recv::<Msg<3, u32>>());
    let wake1 = Cell::new(0);
    let wake2 = Cell::new(0);
    let waker1 = counting_waker(&wake1);
    let waker2 = counting_waker(&wake2);
    let mut cx1 = Context::from_waker(&waker1);
    let mut cx2 = Context::from_waker(&waker2);

    assert!(recv1.as_mut().poll(&mut cx1).is_pending());
    assert!(recv2.as_mut().poll(&mut cx2).is_pending());
    drop(origin);

    assert_eq!(wake1.get(), 1);
    assert_eq!(wake2.get(), 1);
    assert!(matches!(recv1.as_mut().poll(&mut cx1), Poll::Ready(Err(_))));
    assert!(matches!(recv2.as_mut().poll(&mut cx2), Poll::Ready(Err(_))));
}

#[test]
fn reentrant_wake_reloads_lease_root_after_peer_drop_compaction() {
    let role0 = fanout_program::<0>();
    let role1 = fanout_program::<1>();
    let role2 = fanout_program::<2>();
    let mut slab = AlignedSlab([0; 65_536]);
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(31);
    let origin = rv.enter(sid, &role0).expect("origin");
    let mut peer1 = rv.enter(sid, &role1).expect("peer 1");
    let mut peer2 = Some(rv.enter(sid, &role2).expect("peer 2"));
    let state = DropEndpointState::empty();
    state.arm(&mut peer2);
    let waker = drop_on_wake_waker(&state);
    let mut cx = Context::from_waker(&waker);
    let mut recv = pin!(peer1.recv::<Msg<2, u32>>());
    assert!(recv.as_mut().poll(&mut cx).is_pending());

    drop(origin);
    assert!(state.fired.get());
    assert!(peer2.is_none());
    assert!(matches!(recv.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
}
