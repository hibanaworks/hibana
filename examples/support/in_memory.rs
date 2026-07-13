//! Host-only carrier for the runnable example.
//!
//! Production deployments replace this module with their own `Transport`.

use std::{
    cell::RefCell,
    collections::VecDeque,
    task::{Context, Poll, Waker},
};

use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};

pub(crate) struct InMemoryTransport {
    state: RefCell<State>,
}

impl InMemoryTransport {
    pub(crate) fn new() -> Self {
        Self {
            state: RefCell::new(State::default()),
        }
    }
}

#[derive(Default)]
struct State {
    frames: VecDeque<Frame>,
    waiters: Vec<Waiter>,
}

struct Frame {
    session: SessionId,
    lane: u8,
    source: u8,
    target: u8,
    label: u8,
    payload: Vec<u8>,
}

impl Frame {
    fn received(&self) -> ReceivedFrame<'_> {
        let session = self.session.raw().to_be_bytes();
        let header = FrameHeader::from_bytes([
            session[0],
            session[1],
            session[2],
            session[3],
            self.lane,
            self.source,
            self.target,
            self.label,
        ]);
        ReceivedFrame::framed(header, Payload::new(&self.payload))
    }

    fn is_for(&self, rx: &Rx) -> bool {
        self.session == rx.session && self.lane == rx.lane && self.target == rx.local_role
    }
}

struct Waiter {
    session: SessionId,
    lane: u8,
    local_role: u8,
    waker: Waker,
}

impl Waiter {
    fn is_for_frame(&self, frame: &Frame) -> bool {
        self.session == frame.session && self.lane == frame.lane && self.local_role == frame.target
    }

    fn is_for_rx(&self, rx: &Rx) -> bool {
        self.session == rx.session && self.lane == rx.lane && self.local_role == rx.local_role
    }
}

pub(crate) struct Tx {
    session: SessionId,
    lane: u8,
    local_role: u8,
}

pub(crate) struct Rx {
    session: SessionId,
    lane: u8,
    local_role: u8,
    current: Option<Frame>,
}

impl Transport for InMemoryTransport {
    type Tx<'a> = Tx;
    type Rx<'a> = Rx;

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        let session = port.session_id();
        let lane = port.lane();
        let local_role = port.local_role();
        (
            Tx {
                session,
                lane,
                local_role,
            },
            Rx {
                session,
                lane,
                local_role,
                current: None,
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        if outgoing.lane() != tx.lane {
            return Poll::Ready(Err(TransportError::Failed));
        }
        let frame = Frame {
            session: tx.session,
            lane: tx.lane,
            source: tx.local_role,
            target: outgoing.target_role(),
            label: outgoing.frame_label().raw(),
            payload: outgoing.payload().as_bytes().to_vec(),
        };
        let wake = {
            let mut state = self.state.borrow_mut();
            let wake = state
                .waiters
                .iter()
                .position(|waiter| waiter.is_for_frame(&frame))
                .map(|index| state.waiters.swap_remove(index).waker);
            state.frames.push_back(frame);
            wake
        };
        if let Some(waker) = wake {
            waker.wake();
        }
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {
        // Sends complete atomically and never retain pending payload state.
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        rx.current = None;
        let frame = {
            let mut state = self.state.borrow_mut();
            let frame = state
                .frames
                .iter()
                .position(|frame| frame.is_for(rx))
                .and_then(|index| state.frames.remove(index));
            if frame.is_none() {
                if let Some(waiter) = state.waiters.iter_mut().find(|waiter| waiter.is_for_rx(rx)) {
                    waiter.waker = context.waker().clone();
                } else {
                    state.waiters.push(Waiter {
                        session: rx.session,
                        lane: rx.lane,
                        local_role: rx.local_role,
                        waker: context.waker().clone(),
                    });
                }
            }
            frame
        };
        match frame {
            Some(frame) => {
                rx.current = Some(frame);
                Poll::Ready(Ok(rx.current.as_ref().expect("stored frame").received()))
            }
            None => Poll::Pending,
        }
    }

    fn requeue(&self, rx: &mut Self::Rx<'_>) -> Result<(), TransportError> {
        let frame = rx.current.take().ok_or(TransportError::Failed)?;
        self.state.borrow_mut().frames.push_front(frame);
        Ok(())
    }
}
