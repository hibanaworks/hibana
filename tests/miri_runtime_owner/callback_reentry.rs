use super::{
    AlignedSlab, DropEndpointState, NoopRx, NoopTransport, NoopTx, ReceiveState, ReceiveTransport,
    fanout_program, program,
};
use core::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll},
};
use std::rc::Rc;

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};

const CALLBACK_RESOLVER: u16 = 703;
const CALLBACK_VALIDATION_LABEL: u8 = 46;
const CALLBACK_ENCODING_LABEL: u8 = 47;

fn callback_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::send::<0, 1, Msg<44, u32>>(),
            g::send::<0, 1, Msg<45, u32>>(),
        )
        .resolve::<CALLBACK_RESOLVER>(),
    )
}

fn callback_validation_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::send::<
        0,
        1,
        Msg<CALLBACK_VALIDATION_LABEL, CallbackValidatedU32>,
    >())
}

fn callback_encoding_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::send::<
        0,
        1,
        Msg<CALLBACK_ENCODING_LABEL, CallbackEncodedU32>,
    >())
}

struct CallbackValidatedU32;
struct CallbackEncodedU32;

thread_local! {
    static VALIDATOR_DROP_STATE: Cell<*const DropEndpointState> = const {
        Cell::new(core::ptr::null())
    };
}

thread_local! {
    static ENCODER_DROP_STATE: Cell<*const DropEndpointState> = const {
        Cell::new(core::ptr::null())
    };
}

impl WireEncode for CallbackEncodedU32 {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        ENCODER_DROP_STATE.with(|slot| {
            let state = slot.get();
            if state.is_null() {
                panic!("callback encoder state is not armed");
            }
            /* SAFETY: the test installs the live stack state immediately
            before send and clears this thread-local pointer before that state
            leaves scope. */
            unsafe { (&*state).fire() };
        });
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&47u32.to_be_bytes());
        Ok(4)
    }
}

impl WireEncode for CallbackValidatedU32 {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&46u32.to_be_bytes());
        Ok(4)
    }
}

impl WirePayload for CallbackValidatedU32 {
    type Decoded<'a> = u32;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        VALIDATOR_DROP_STATE.with(|slot| {
            let state = slot.get();
            if state.is_null() {
                panic!("callback validator state is not armed");
            }
            /* SAFETY: the test installs the live stack state immediately
            before recv and clears this thread-local pointer before that state
            leaves scope. */
            unsafe { (&*state).fire() };
        });
        match input.as_bytes().len() {
            4 => Ok(()),
            0..=3 => Err(CodecError::Truncated),
            _ => Err(CodecError::Malformed),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

fn drop_peer_and_choose_left(state: &DropEndpointState) -> Result<DecisionArm, ResolverError> {
    state.fire();
    Ok(DecisionArm::Left)
}

struct ReentrantOpenTransport {
    state: Rc<DropEndpointState>,
}

impl Transport for ReentrantOpenTransport {
    type Tx<'a> = NoopTx;
    type Rx<'a> = NoopRx;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.state.fire_if_armed();
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

struct ReentrantReadyRecvTransport {
    state: Rc<DropEndpointState>,
    bytes: [u8; 4],
}

impl Transport for ReentrantReadyRecvTransport {
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
        Poll::Ready(Ok(ReceivedFrame::deterministic(Payload::new(&self.bytes))))
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

struct ReentrantSendTransport {
    state: Rc<DropEndpointState>,
}

struct ReentrantCancelTransport {
    state: Rc<DropEndpointState>,
    cancel_count: Rc<Cell<usize>>,
}

impl Transport for ReentrantCancelTransport {
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
        Poll::Pending
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {
        self.cancel_count.set(self.cancel_count.get() + 1);
        self.state.fire();
    }

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

impl Transport for ReentrantSendTransport {
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
        self.state.fire();
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
fn transport_open_reentry_cannot_publish_endpoint_into_poisoned_session() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let mut storage = SessionKitStorage::<ReentrantOpenTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantOpenTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(16);
    let mut origin = Some(rendezvous.enter(sid, &role0).expect("origin"));
    state.arm(&mut origin);

    let target = rendezvous.enter(sid, &role1);

    assert!(state.fired.get());
    assert!(origin.is_none());
    let error = match target {
        Ok(_) => panic!("attach published an endpoint after Transport::open poisoned its session"),
        Err(error) => error,
    };
    assert!(format!("{error:?}").contains("SessionPoisoned"));

    let fresh = rendezvous
        .enter(SessionId::new(160), &role1)
        .expect("aborted reentrant attach must release its reservation and registry lease");
    drop(fresh);
}

#[test]
fn resolver_callback_reentry_cannot_materialize_branch_after_peer_drop() {
    let role0 = callback_resolver_program::<0>();
    let role1 = callback_resolver_program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let resolver_state = DropEndpointState::empty();
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    rendezvous
        .set_resolver(
            &role0,
            ResolverRef::<CALLBACK_RESOLVER>::decision_state(
                &resolver_state,
                drop_peer_and_choose_left,
            ),
        )
        .expect("install callback resolver");
    let sid = SessionId::new(15);
    let mut controller = rendezvous.enter(sid, &role0).expect("controller");
    let mut peer = Some(rendezvous.enter(sid, &role1).expect("peer"));
    resolver_state.arm(&mut peer);

    let result = futures::executor::block_on(controller.offer());

    assert!(resolver_state.fired.get());
    assert!(peer.is_none());
    match result {
        Ok(branch) => {
            drop(branch);
            panic!("resolver callback published a branch after dropping its peer");
        }
        Err(error) => assert!(format!("{error:?}").contains("EndpointDropped")),
    }
}

#[test]
fn resolver_callback_reentry_cannot_prepare_send_after_peer_drop() {
    let role0 = callback_resolver_program::<0>();
    let role1 = callback_resolver_program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let resolver_state = DropEndpointState::empty();
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    rendezvous
        .set_resolver(
            &role0,
            ResolverRef::<CALLBACK_RESOLVER>::decision_state(
                &resolver_state,
                drop_peer_and_choose_left,
            ),
        )
        .expect("install callback resolver");
    let sid = SessionId::new(14);
    let mut controller = rendezvous.enter(sid, &role0).expect("controller");
    let mut peer = Some(rendezvous.enter(sid, &role1).expect("peer"));
    resolver_state.arm(&mut peer);

    let result = futures::executor::block_on(controller.send::<Msg<44, u32>>(&44));

    assert!(resolver_state.fired.get());
    assert!(peer.is_none());
    let error =
        result.expect_err("resolver callback must not prepare send after dropping its peer");
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn transport_recv_callback_reentry_cannot_commit_frame_after_peer_drop() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let mut storage = SessionKitStorage::<ReentrantReadyRecvTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantReadyRecvTransport {
                state: Rc::clone(&state),
                bytes: 12u32.to_be_bytes(),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(12);
    let mut peer = Some(rendezvous.enter(sid, &role0).expect("peer"));
    let mut receiver = rendezvous.enter(sid, &role1).expect("receiver");
    state.arm(&mut peer);

    let result = futures::executor::block_on(receiver.recv::<Msg<1, u32>>());

    assert!(state.fired.get());
    assert!(peer.is_none());
    let error =
        result.expect_err("transport recv callback must not commit after dropping its peer");
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn payload_validation_callback_reentry_cannot_commit_after_peer_drop() {
    let role0 = callback_validation_program::<0>();
    let role1 = callback_validation_program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let transport_state = Rc::new(ReceiveState {
        bytes: 46u32.to_be_bytes(),
        requeues: Cell::new(0),
    });
    let drop_state = DropEndpointState::empty();
    let mut storage = SessionKitStorage::<ReceiveTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReceiveTransport {
                state: Rc::clone(&transport_state),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(11);
    let mut peer = Some(rendezvous.enter(sid, &role0).expect("peer"));
    let mut receiver = rendezvous.enter(sid, &role1).expect("receiver");
    drop_state.arm(&mut peer);
    VALIDATOR_DROP_STATE.with(|slot| slot.set(core::ptr::from_ref(&drop_state)));

    let result = futures::executor::block_on(
        receiver.recv::<Msg<CALLBACK_VALIDATION_LABEL, CallbackValidatedU32>>(),
    );
    VALIDATOR_DROP_STATE.with(|slot| slot.set(core::ptr::null()));

    assert!(drop_state.fired.get());
    assert!(peer.is_none());
    let error = result.expect_err("payload validation must not commit after dropping its peer");
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn payload_encoding_callback_reentry_cannot_commit_after_peer_drop() {
    let role0 = callback_encoding_program::<0>();
    let role1 = callback_encoding_program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let drop_state = DropEndpointState::empty();
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab.0, NoopTransport)
        .expect("register rendezvous");
    let sid = SessionId::new(9);
    let mut sender = rendezvous.enter(sid, &role0).expect("sender");
    let mut peer = Some(rendezvous.enter(sid, &role1).expect("peer"));
    drop_state.arm(&mut peer);
    ENCODER_DROP_STATE.with(|slot| slot.set(core::ptr::from_ref(&drop_state)));

    let result = futures::executor::block_on(
        sender.send::<Msg<CALLBACK_ENCODING_LABEL, CallbackEncodedU32>>(&CallbackEncodedU32),
    );
    ENCODER_DROP_STATE.with(|slot| slot.set(core::ptr::null()));

    assert!(drop_state.fired.get());
    assert!(peer.is_none());
    let error = result.expect_err("payload encoding must not commit after dropping its peer");
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn transport_send_callback_reentry_cannot_commit_after_peer_drop() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let mut storage = SessionKitStorage::<ReentrantSendTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantSendTransport {
                state: Rc::clone(&state),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(13);
    let mut sender = rendezvous.enter(sid, &role0).expect("sender");
    let mut peer = Some(rendezvous.enter(sid, &role1).expect("peer"));
    state.arm(&mut peer);

    let result = futures::executor::block_on(sender.send::<Msg<1, u32>>(&13));

    assert!(state.fired.get());
    assert!(peer.is_none());
    let error =
        result.expect_err("transport send callback must not commit after dropping its peer");
    assert!(format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn transport_cancel_callback_reentry_consumes_cleanup_authority_once() {
    let role0 = fanout_program::<0>();
    let role1 = fanout_program::<1>();
    let role2 = fanout_program::<2>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let cancel_count = Rc::new(Cell::new(0));
    let mut storage = SessionKitStorage::<ReentrantCancelTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantCancelTransport {
                state: Rc::clone(&state),
                cancel_count: Rc::clone(&cancel_count),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(17);
    let mut sender = rendezvous.enter(sid, &role0).expect("sender");
    let mut peer1 = Some(rendezvous.enter(sid, &role1).expect("peer 1"));
    let mut peer2 = Some(rendezvous.enter(sid, &role2).expect("peer 2"));
    state.arm(&mut peer2);
    let payload = 17u32;
    {
        let mut send = pin!(sender.send::<Msg<2, u32>>(&payload));
        let waker = futures::task::noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
        drop(peer1.take());
        let result = match send.as_mut().poll(&mut cx) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("poisoned pending send must cancel and terminate"),
        };

        assert!(state.fired.get());
        assert!(peer2.is_none());
        assert_eq!(cancel_count.get(), 1);
        let error = result.expect_err("cancel callback reentry must preserve the session fault");
        assert!(format!("{error:?}").contains("EndpointDropped"));
    }
    assert_eq!(cancel_count.get(), 1);
}
