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
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    },
};

struct NoopTransport;
struct NoopTx;
struct NoopRx;

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
