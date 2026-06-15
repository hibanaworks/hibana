use super::*;
use crate::transport::{ReceivedFrame, wire::Payload};
use core::{
    cell::{Cell, UnsafeCell},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

struct SharedState {
    waker: UnsafeCell<Option<Waker>>,
    ready: Cell<bool>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            waker: UnsafeCell::new(None),
            ready: Cell::new(false),
        }
    }

    fn store_waker(&self, waker: &Waker) {
        unsafe {
            *self.waker.get() = Some(waker.clone());
        }
    }

    fn take_waker(&self) -> Option<Waker> {
        unsafe { (*self.waker.get()).take() }
    }

    fn set_ready(&self) {
        self.ready.set(true);
    }

    fn take_ready(&self) -> bool {
        self.ready.replace(false)
    }
}

struct WakerAwareTransport {
    state: SharedState,
}

impl WakerAwareTransport {
    fn new() -> Self {
        Self {
            state: SharedState::new(),
        }
    }

    fn state(&self) -> &SharedState {
        &self.state
    }
}

impl Transport for WakerAwareTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        core::hint::black_box(port);
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        core::hint::black_box((tx, outgoing, cx.waker()));
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        static PAYLOAD: [u8; 0] = [];
        core::hint::black_box(rx);
        self.state.store_waker(cx.waker());
        if self.state.take_ready() {
            Poll::Ready(Ok(ReceivedFrame::deterministic(Payload::new(&PAYLOAD))))
        } else {
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        core::hint::black_box(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        core::hint::black_box(rx);
        self.state.set_ready();
        Ok(())
    }
}

unsafe fn flag_waker(flag: &Cell<bool>) -> Waker {
    unsafe fn clone(data: *const ()) -> RawWaker {
        RawWaker::new(data, &VTABLE)
    }

    unsafe fn wake(data: *const ()) {
        unsafe { (*(data as *const Cell<bool>)).set(true) };
    }

    unsafe fn wake_by_ref(data: *const ()) {
        unsafe { (*(data as *const Cell<bool>)).set(true) };
    }

    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    unsafe {
        Waker::from_raw(RawWaker::new(
            flag as *const Cell<bool> as *const (),
            &VTABLE,
        ))
    }
}

#[test]
fn frame_header_preserves_full_wire_domain() {
    let header = FrameHeader::from_raw(u64::MAX);

    assert_eq!(header.raw(), u64::MAX);
}

#[test]
fn recv_future_records_waker_and_wakes() {
    let transport = WakerAwareTransport::new();
    let shared = transport.state();
    transport.open(PortOpen::from_descriptor(
        0,
        crate::session::types::SessionId::new(0),
        crate::session::types::Lane::new(0),
    ));
    let mut rx = ();

    assert!(shared.take_waker().is_none(), "no waker before polling");

    let wake_flag = Cell::new(false);
    let waker = unsafe { flag_waker(&wake_flag) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(
        transport.poll_recv(&mut rx, &mut cx),
        Poll::Pending
    ));

    let stored = shared.take_waker().expect("future recorded waker");
    shared.set_ready();
    stored.wake();
    assert!(wake_flag.get(), "wake flag flipped");

    assert!(matches!(
        transport.poll_recv(&mut rx, &mut cx),
        Poll::Ready(Ok(_))
    ));
}
