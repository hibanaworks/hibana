use super::*;
use crate::transport::wire::Payload;
use core::{
    cell::{Cell, UnsafeCell},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

#[derive(Default)]
struct SharedState {
    waker: UnsafeCell<Option<Waker>>,
    ready: Cell<bool>,
}

impl SharedState {
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

#[test]
fn transport_event_fixed_decoders_reject_trailing_bytes() {
    assert_eq!(
        TransportEventKind::decode_payload(Payload::new(&[0])),
        Ok(TransportEventKind::Ack)
    );
    assert_eq!(
        TransportEventKind::decode_payload(Payload::new(&[0, 0])),
        Err(CodecError::Invalid("payload length"))
    );

    let event = TransportEvent::from_meta(
        TransportEventMeta::new(TransportEventKind::Loss)
            .packet_number(0x0102_0304_0506_0708)
            .payload_len(0x1122_3344)
            .retransmissions(0x5566_7788)
            .packet_number_space(3)
            .connection_id_tag(4),
    );
    let mut encoded = [0u8; 20];
    assert_eq!(event.encode_into(&mut encoded[..19]), Ok(19));
    assert_eq!(
        TransportEvent::decode_payload(Payload::new(&encoded[..19])),
        Ok(event)
    );
    assert_eq!(
        TransportEvent::decode_payload(Payload::new(&encoded)),
        Err(CodecError::Invalid("payload length"))
    );
}

#[test]
fn transport_event_tap_packet_number_space_saturates_without_modulo_aliasing() {
    let event = TransportEvent::from_meta(
        TransportEventMeta::new(TransportEventKind::Ack).packet_number_space(8),
    );
    assert_eq!(event.pn_space(), 7);
    let (_, arg1) = event.encode_tap_args();
    assert_eq!((arg1 >> 26) & 0x7, 7);

    let mut encoded = [0u8; 19];
    encoded[0] = 0;
    encoded[1] = 9;
    let decoded = TransportEvent::decode_payload(Payload::new(&encoded))
        .expect("transport event with extended wire pn-space byte must decode");
    assert_eq!(decoded.pn_space(), 9);
    let (_, decoded_arg1) = decoded.encode_tap_args();
    assert_eq!((decoded_arg1 >> 26) & 0x7, 7);
}

impl WakerAwareTransport {
    fn new() -> Self {
        Self {
            state: SharedState::default(),
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
    type Metrics = ();

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        static PAYLOAD: [u8; 0] = [];
        self.state.store_waker(cx.waker());
        if self.state.take_ready() {
            Poll::Ready(Ok(Payload::new(&PAYLOAD)))
        } else {
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: WakerAwareTransport only exercises direct poll_recv
    // waker storage.
    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        unreachable!("WakerAwareTransport does not exercise endpoint rollback")
    }

    fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent)) {
        let _ = emit;
    }

    fn recv_frame_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
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
fn recv_future_records_waker_and_wakes() {
    let transport = WakerAwareTransport::new();
    let shared = transport.state();
    let mut rx = transport
        .open(PortOpen::from_descriptor(
            0,
            crate::control::types::SessionId::new(0),
            crate::control::types::Lane::new(0),
        ))
        .1;

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
