use std::{
    cell::Cell,
    collections::VecDeque,
    task::{Context, Poll, Waker},
};

use hibana::runtime::{
    ids::SessionId,
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};

#[derive(Default)]
struct Queues {
    frames: VecDeque<Frame>,
    waiters: Vec<(SessionId, u8, u8, Waker)>,
}

pub(super) struct Network {
    queues: Cell<Queues>,
    binding: EndpointBinding,
    pub sent: Cell<usize>,
    pub received: Cell<usize>,
    pub parallel_receivers: Cell<usize>,
}

impl Network {
    pub fn new() -> Self {
        Self {
            queues: Cell::new(Queues::default()),
            // SAFETY: this fixture owns one DB resource; every runtime borrows
            // this network and the same binding, with no other issuer.
            binding: unsafe { EndpointBinding::new() },
            sent: Cell::new(0),
            received: Cell::new(0),
            parallel_receivers: Cell::new(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        let queues = self.queues.take();
        let empty = queues.frames.is_empty();
        self.queues.set(queues);
        empty
    }
}

struct Frame {
    header: FrameHeader,
    payload: Vec<u8>,
}

pub(super) struct Tx {
    session: SessionId,
    lane: u8,
    role: u8,
}

pub(super) struct Rx {
    port: Tx,
    current: Option<Frame>,
}

impl Transport for &Network {
    type Tx<'a>
        = Tx
    where
        Self: 'a;
    type Rx<'a>
        = Rx
    where
        Self: 'a;

    fn initial_endpoint_binding(&self, _session: SessionId, role: u8) -> Option<&EndpointBinding> {
        (role == 2).then_some(&self.binding)
    }

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        let tx = || Tx {
            session: port.session_id(),
            lane: port.lane(),
            role: port.local_role(),
        };
        (
            tx(),
            Rx {
                port: tx(),
                current: None,
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Tx,
        outgoing: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        assert_eq!(tx.lane, outgoing.lane());
        let session = tx.session.raw().to_be_bytes();
        let label = outgoing.frame_label().raw().to_be_bytes();
        let frame = Frame {
            header: FrameHeader::from_bytes([
                session[0],
                session[1],
                session[2],
                session[3],
                tx.lane,
                tx.role,
                outgoing.target_role(),
                label[0],
                label[1],
            ]),
            payload: outgoing.payload().as_bytes().to_vec(),
        };
        let mut queues = self.queues.take();
        let wake = queues
            .waiters
            .iter()
            .position(|(session, lane, role, _)| {
                (*session, *lane, *role) == (tx.session, tx.lane, outgoing.target_role())
            })
            .map(|i| queues.waiters.swap_remove(i).3);
        queues.frames.push_back(frame);
        self.queues.set(queues);
        self.sent.set(self.sent.get() + 1);
        if let Some(wake) = wake {
            wake.wake();
        }
        Poll::Ready(Ok(()))
    }

    fn cancel_send(&self, _: &mut Tx) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Rx,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        let next_waker = cx.waker().clone();
        rx.current = None;
        let mut queues = self.queues.take();
        let frame = queues
            .frames
            .iter()
            .position(|frame| {
                let bytes = frame.header.bytes();
                bytes[..4] == rx.port.session.raw().to_be_bytes()
                    && bytes[4] == rx.port.lane
                    && bytes[6] == rx.port.role
            })
            .and_then(|i| queues.frames.remove(i));
        let mut replaced = None;
        if frame.is_none() {
            if let Some(waiter) = queues.waiters.iter_mut().find(|(session, lane, role, _)| {
                (*session, *lane, *role) == (rx.port.session, rx.port.lane, rx.port.role)
            }) {
                replaced = Some(std::mem::replace(&mut waiter.3, next_waker));
            } else {
                queues
                    .waiters
                    .push((rx.port.session, rx.port.lane, rx.port.role, next_waker));
            }
            let count = queues
                .waiters
                .iter()
                .filter(|(_, _, role, _)| *role == 2)
                .count();
            self.parallel_receivers
                .set(self.parallel_receivers.get().max(count));
        }
        self.queues.set(queues);
        drop(replaced);
        if let Some(frame) = frame {
            rx.current = Some(frame);
            self.received.set(self.received.get() + 1);
            let frame = rx.current.as_ref().unwrap();
            Poll::Ready(Ok(ReceivedFrame::framed(
                frame.header,
                Payload::new(&frame.payload),
            )))
        } else {
            Poll::Pending
        }
    }

    fn requeue(&self, rx: &mut Rx) -> Result<(), TransportError> {
        let frame = rx.current.take().ok_or(TransportError::Failed)?;
        let mut queues = self.queues.take();
        let wake = queues
            .waiters
            .iter()
            .position(|(session, lane, role, _)| {
                (*session, *lane, *role) == (rx.port.session, rx.port.lane, rx.port.role)
            })
            .map(|i| queues.waiters.swap_remove(i).3);
        queues.frames.push_front(frame);
        self.queues.set(queues);
        self.received.set(self.received.get() - 1);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
}
