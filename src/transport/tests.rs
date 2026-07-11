use super::*;
use crate::transport::{ReceivedFrame, wire::Payload};
use core::{
    cell::{Cell, UnsafeCell},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::vec::Vec;

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
    ) -> Poll<Result<(), TransportError>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
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

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
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
fn frame_header_preserves_full_wire_domain_as_bytes() {
    let header = FrameHeader::from_bytes([u8::MAX; 8]);

    assert_eq!(header.bytes(), [u8::MAX; 8]);
}

#[test]
fn received_frame_framed_bytes_roundtrip() {
    let bytes = [0, 0, 0, 7, 2, 3, 4, 5];
    let payload = [8, 13];
    let frame = ReceivedFrame::framed(FrameHeader::from_bytes(bytes), Payload::new(&payload));
    let header = frame
        .evidence()
        .frame_header()
        .expect("framed receive carries header evidence");

    assert_eq!(header.bytes(), bytes);
    assert_eq!(frame.payload().as_bytes(), payload.as_slice());
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContractFrame {
    sequence: usize,
    value: u8,
}

struct ContractCarrier {
    frames: Vec<ContractFrame>,
    delivered: usize,
    closed: bool,
    waiter: Option<Waker>,
}

impl ContractCarrier {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            delivered: 0,
            closed: false,
            waiter: None,
        }
    }

    fn send(&mut self, value: u8) {
        assert!(!self.closed, "closed carrier rejects later sends");
        self.frames.push(ContractFrame {
            sequence: self.frames.len(),
            value,
        });
    }

    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<ContractFrame, TransportError>> {
        if let Some(frame) = self.frames.get(self.delivered).copied() {
            self.delivered += 1;
            Poll::Ready(Ok(frame))
        } else if self.closed {
            Poll::Ready(Err(TransportError::Offline))
        } else {
            self.waiter = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn close_peer(&mut self) {
        self.closed = true;
        if let Some(waiter) = self.waiter.take() {
            waiter.wake();
        }
    }
}

#[test]
fn transport_contract_fifo_exactly_once_and_no_replay() {
    let mut carrier = ContractCarrier::new();
    carrier.send(11);
    carrier.send(22);
    carrier.send(33);

    let wake_flag = Cell::new(false);
    let waker = unsafe { flag_waker(&wake_flag) };
    let mut cx = Context::from_waker(&waker);
    for (sequence, value) in [11, 22, 33].into_iter().enumerate() {
        let Poll::Ready(Ok(frame)) = carrier.poll_recv(&mut cx) else {
            panic!("accepted frames must remain FIFO-ready");
        };
        assert_eq!(frame, ContractFrame { sequence, value });
    }

    assert!(matches!(carrier.poll_recv(&mut cx), Poll::Pending));
    assert_eq!(carrier.delivered, carrier.frames.len());
}

#[test]
fn transport_contract_peer_close_wakes_and_is_observable_after_drain() {
    let mut carrier = ContractCarrier::new();
    let wake_flag = Cell::new(false);
    let waker = unsafe { flag_waker(&wake_flag) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(carrier.poll_recv(&mut cx), Poll::Pending));
    carrier.close_peer();
    assert!(wake_flag.get(), "peer close must wake the parked receiver");
    assert!(matches!(
        carrier.poll_recv(&mut cx),
        Poll::Ready(Err(TransportError::Offline))
    ));
}
