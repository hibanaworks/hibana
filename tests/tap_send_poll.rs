use core::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        tap::{self, TapEvent},
        transport::{
            EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport,
            TransportError,
        },
        wire::Payload,
    },
};

#[derive(Clone, Copy)]
struct Frame {
    header: FrameHeader,
    bytes: [u8; 3],
    len: u8,
}

// Physical storage only: a poll exposes one copied frame before the sender
// completes. Each opened port owns its own pending header and receive buffer.
struct DelayedPublication {
    inbox: Cell<Result<Option<Frame>, TransportError>>,
    reader: Cell<Option<Waker>>,
    resource: Option<EndpointBinding>,
}

impl Transport for DelayedPublication {
    type Tx<'a> = (PortOpen, Option<FrameHeader>);
    type Rx<'a> = (PortOpen, Option<Frame>);

    fn initial_endpoint_binding(&self, _: SessionId, role: u8) -> Option<&EndpointBinding> {
        if role == 3 {
            self.resource.as_ref()
        } else {
            None
        }
    }

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        ((port, None), (port, None))
    }

    fn poll_send(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: Outgoing<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        let session = tx.0.session_id().raw().to_be_bytes();
        let label = outgoing.frame_label().raw().to_be_bytes();
        let header = FrameHeader::from_bytes([
            session[0],
            session[1],
            session[2],
            session[3],
            tx.0.lane(),
            tx.0.local_role(),
            outgoing.target_role(),
            label[0],
            label[1],
        ]);
        assert_eq!(outgoing.lane(), tx.0.lane());
        if let Some(accepted) = tx.1.take() {
            assert_eq!(accepted, header);
            return Poll::Ready(Ok(()));
        }
        match self.inbox.get() {
            Ok(None) => {
                let bytes = outgoing.payload().as_bytes();
                let mut copied = [0; 3];
                copied[..bytes.len()].copy_from_slice(bytes);
                self.inbox.set(Ok(Some(Frame {
                    header,
                    bytes: copied,
                    len: bytes.len() as u8,
                })));
                tx.1 = Some(header);
                if let Some(reader) = self.reader.take() {
                    reader.wake();
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Ok(Some(_)) => Poll::Ready(Err(TransportError::Capacity)),
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        if tx.1.take().is_some() {
            self.inbox.set(Err(TransportError::Offline));
            if let Some(reader) = self.reader.take() {
                reader.wake();
            }
        }
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        match self.inbox.get() {
            Ok(Some(frame)) => {
                assert_eq!(frame.header.bytes()[6], rx.0.local_role());
                self.inbox.set(Ok(None));
                rx.1 = Some(frame);
                let stored = rx.1.as_ref().unwrap();
                Poll::Ready(Ok(ReceivedFrame::framed(
                    stored.header,
                    Payload::new(&stored.bytes[..usize::from(stored.len)]),
                )))
            }
            Ok(None) => {
                self.reader.set(Some(cx.waker().clone()));
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    fn requeue(&self, rx: &mut Self::Rx<'_>) -> Result<(), TransportError> {
        let frame = rx.1.take().unwrap();
        match self.inbox.get() {
            Ok(None) => {
                self.inbox.set(Ok(Some(frame)));
                if let Some(reader) = self.reader.take() {
                    reader.wake();
                }
                Ok(())
            }
            Ok(Some(_)) => Err(TransportError::Capacity),
            Err(error) => Err(error),
        }
    }
}

#[test]
fn each_checked_wire_poll_precedes_frame_delivery_and_keeps_its_prepared_scope() {
    type Message = g::Msg<7, ()>;
    let global = g::send::<0, 1, Message>().roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let roll = tap::projected(&p0)
        .find(|row| row.id() == tap::PROJECTED_SCOPE && row.causal_key() >> 13 == 1)
        .unwrap()
        .causal_key();
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(
            &mut slab,
            DelayedPublication {
                inbox: Cell::new(Ok(None)),
                reader: Cell::new(None),
                resource: None,
            },
        )
        .unwrap();
    let mut sender = kit.enter(SessionId::new(101), &p0).unwrap();
    let mut receiver = kit.enter(SessionId::new(101), &p1).unwrap();
    let mut tap = kit.tap();
    let mut captured: Vec<TapEvent> = tap.by_ref().collect();
    let mut cx = Context::from_waker(Waker::noop());
    for visit in 0..2 {
        let begin = captured.len();
        let mut sending = pin!(sender.send::<Message>(&()));
        assert!(sending.as_mut().poll(&mut cx).is_pending());
        captured.extend(tap.by_ref());
        let first = captured[begin..]
            .iter()
            .find(|row| row.id() == tap::SEND_POLL)
            .unwrap();
        assert_eq!(first.causal_key(), 0);
        assert_eq!(first.arg0(), 101);
        assert_eq!(
            first.arg1(),
            u32::from(if visit == 0 { u16::MAX } else { roll }) << 16
        );
        assert!(
            !captured[begin..]
                .iter()
                .any(|row| row.id() == tap::ENDPOINT_SEND)
        );
        {
            let receiving = pin!(receiver.recv::<Message>());
            assert!(matches!(receiving.poll(&mut cx), Poll::Ready(Ok(()))));
        }
        assert!(matches!(
            sending.as_mut().poll(&mut cx),
            Poll::Ready(Ok(()))
        ));
        captured.extend(tap.by_ref());
        let batch = &captured[begin..];
        assert_eq!(
            batch
                .iter()
                .filter(|row| row.id() == tap::SEND_POLL)
                .count(),
            2
        );
        let admitted = batch
            .iter()
            .position(|row| row.id() == tap::SEND_POLL)
            .unwrap();
        let received = batch
            .iter()
            .position(|row| row.id() == tap::ENDPOINT_RECV)
            .unwrap();
        let published = batch
            .iter()
            .position(|row| row.id() == tap::ENDPOINT_SEND)
            .unwrap();
        assert!(admitted < received && received < published);
        if visit != 0 {
            assert!(batch.iter().any(|row| row.id() == tap::SCOPE_REENTRY
                && row.causal_key() == 0
                && row.arg1() == u32::from(roll) << 16));
        }
    }
    assert_eq!(tap.checkpoint(), Some(captured.len() as u32));
}

#[test]
fn an_out_of_order_send_never_reaches_a_transport_poll_observation() {
    type Gate = g::Msg<6, ()>;
    type Message = g::Msg<7, ()>;
    let global = g::seq(g::send::<0, 0, Gate>(), g::send::<0, 1, Message>());
    let p0 = project::<0>(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(
            &mut slab,
            DelayedPublication {
                inbox: Cell::new(Ok(None)),
                reader: Cell::new(None),
                resource: None,
            },
        )
        .unwrap();
    let mut sender = kit.enter(SessionId::new(102), &p0).unwrap();
    let mut tap = kit.tap();
    let mut captured: Vec<TapEvent> = tap.by_ref().collect();
    let mut cx = Context::from_waker(Waker::noop());
    let sending = pin!(sender.send::<Message>(&()));
    let Poll::Ready(Err(error)) = sending.poll(&mut cx) else {
        panic!("a source-order violation must be rejected before polling the transport")
    };
    assert!(format!("{error:?}").contains("OutOfOrder"));
    captured.extend(tap.by_ref());
    assert!(!captured.iter().any(|row| matches!(
        row.id(),
        tap::SEND_POLL | tap::ENDPOINT_SEND | tap::TRANSPORT_FRAME
    )));
}

#[test]
fn local_unit_operations_do_not_invent_transport_polls() {
    type Tick = g::Msg<7, ()>;
    let global = g::send::<0, 0, Tick>().roll();
    let p0 = project::<0>(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(
            &mut slab,
            DelayedPublication {
                inbox: Cell::new(Ok(None)),
                reader: Cell::new(None),
                resource: None,
            },
        )
        .unwrap();
    let mut endpoint = kit.enter(SessionId::new(104), &p0).unwrap();
    let mut tap = kit.tap();
    let mut captured: Vec<TapEvent> = tap.by_ref().collect();
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..2 {
        let operation = pin!(endpoint.send::<Tick>(&()));
        assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
    }
    captured.extend(tap.by_ref());
    assert_eq!(
        captured
            .iter()
            .filter(|row| row.id() == tap::ENDPOINT_SEND)
            .count(),
        2
    );
    assert!(!captured.iter().any(|row| row.id() == tap::SEND_POLL));
}

#[test]
fn a_received_resource_can_be_used_before_the_loan_sender_publishes() {
    type Loan = g::Msg<1, Endpoint<'static, 3>>;
    type Use = g::Msg<2, ()>;
    let global = g::seq(g::send::<0, 1, Loan>(), g::send::<3, 2, Use>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(
            &mut slab,
            DelayedPublication {
                inbox: Cell::new(Ok(None)),
                reader: Cell::new(None),
                // SAFETY: this transport owns the sole physical resource fixture;
                // every port and actor of this run reaches this same binding.
                resource: Some(unsafe { EndpointBinding::new() }),
            },
        )
        .unwrap();
    let sid = SessionId::new(103);
    let mut owner = kit.enter(sid, &p0).unwrap();
    let mut worker = kit.enter(sid, &p1).unwrap();
    let mut service = kit.enter(sid, &p2).unwrap();
    let resource = kit.enter(sid, &p3).unwrap();
    let mut tap = kit.tap();
    let mut captured: Vec<TapEvent> = tap.by_ref().collect();
    let mut cx = Context::from_waker(Waker::noop());
    let mut lending = pin!(owner.send::<Loan>(resource));
    assert!(lending.as_mut().poll(&mut cx).is_pending());
    captured.extend(tap.by_ref());
    assert!(captured.iter().any(|row| row.id() == tap::SEND_POLL
        && row.causal_key() == 0
        && row.arg1() & 0xffff == 0));
    let mut resource = {
        let receive = pin!(worker.recv::<Loan>());
        let Poll::Ready(Ok(resource)) = receive.poll(&mut cx) else {
            panic!("the delivered loan must be available before sender publication")
        };
        resource
    };
    {
        let mut using = pin!(resource.send::<Use>(&()));
        assert!(using.as_mut().poll(&mut cx).is_pending());
        captured.extend(tap.by_ref());
        assert!(captured.iter().any(|row| row.id() == tap::SEND_POLL
            && row.causal_key() == ((1 << 8) | 3)
            && row.arg1() & 0xffff == 1));
        let receive = pin!(service.recv::<Use>());
        assert!(matches!(receive.poll(&mut cx), Poll::Ready(Ok(()))));
        assert!(matches!(using.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    captured.extend(tap.by_ref());
    assert!(captured.iter().any(|row| row.id() == tap::SEND_POLL
        && row.causal_key() == ((1 << 8) | 3)
        && row.arg1() & 0xffff == 1));
    assert!(
        !captured
            .iter()
            .any(|row| row.id() == tap::ENDPOINT_SEND && row.arg1() & 0xffff == 0)
    );
    assert!(matches!(
        lending.as_mut().poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    captured.extend(tap.by_ref());
    assert_eq!(
        captured
            .iter()
            .filter(|row| row.id() == tap::ENDPOINT_SEND)
            .count(),
        2
    );
    assert_eq!(tap.checkpoint(), Some(captured.len() as u32));
}
