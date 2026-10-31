//! Host-only carrier for the runnable example.
//!
//! Production deployments replace this module with their own `Transport`.

use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    task::{Context, Poll, Waker},
    vec::Vec,
};

use hibana::runtime::{
    ids::SessionId,
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};

#[derive(Clone)]
pub(crate) struct InMemoryTransport {
    network: Rc<Network>,
}

struct Network {
    state: RefCell<State>,
    initial_bindings: Vec<((SessionId, u8), EndpointBinding)>,
}

impl InMemoryTransport {
    #[cfg(all(test, hibana_repo_tests, hibana_profile))]
    pub(crate) fn queue_is_empty(&self) -> bool {
        self.network.state.borrow().frames.is_empty()
    }

    pub(crate) fn new(
        bindings: impl IntoIterator<Item = ((SessionId, u8), EndpointBinding)>,
    ) -> Self {
        Self {
            network: Rc::new(Network {
                state: RefCell::new(State::default()),
                initial_bindings: bindings.into_iter().collect(),
            }),
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
    label: u16,
    payload: Vec<u8>,
}

impl Frame {
    fn received(&self) -> ReceivedFrame<'_> {
        let session = self.session.raw().to_be_bytes();
        let label = self.label.to_be_bytes();
        let header = FrameHeader::from_bytes([
            session[0],
            session[1],
            session[2],
            session[3],
            self.lane,
            self.source,
            self.target,
            label[0],
            label[1],
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

    fn initial_endpoint_binding(&self, session: SessionId, role: u8) -> Option<&EndpointBinding> {
        self.network
            .initial_bindings
            .iter()
            .find_map(|(key, binding)| (*key == (session, role)).then_some(binding))
    }

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

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
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
            let mut state = self.network.state.borrow_mut();
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

    fn cancel_send(&self, _tx: &mut Self::Tx<'_>) {
        // Sends complete atomically and never retain pending payload state.
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        rx.current = None;
        let frame = {
            let mut state = self.network.state.borrow_mut();
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
        let wake = {
            let mut state = self.network.state.borrow_mut();
            let wake = state
                .waiters
                .iter()
                .position(|waiter| waiter.is_for_frame(&frame))
                .map(|index| state.waiters.swap_remove(index).waker);
            state.frames.push_front(frame);
            wake
        };
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn requeued_frame_wakes_the_waiting_replacement_receiver() {
        use super::*;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::task::Wake;

        struct CountWake(AtomicUsize);

        impl Wake for CountWake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let network = InMemoryTransport::new([]);
        let session = SessionId::new(1);
        let mut previous = Rx {
            session,
            lane: 0,
            local_role: 2,
            current: Some(Frame {
                session,
                lane: 0,
                source: 3,
                target: 2,
                label: 0,
                payload: std::vec![7],
            }),
        };
        let mut replacement = Rx {
            session,
            lane: 0,
            local_role: 2,
            current: None,
        };
        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());
        let mut context = Context::from_waker(&waker);
        assert!(
            network
                .poll_recv(&mut replacement, &mut context)
                .is_pending()
        );
        network.requeue(&mut previous).unwrap();
        assert!(previous.current.is_none());
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
        assert!(matches!(
            network.poll_recv(&mut replacement, &mut context),
            Poll::Ready(Ok(_))
        ));
        assert_eq!(replacement.current.unwrap().payload, [7]);
    }
}
