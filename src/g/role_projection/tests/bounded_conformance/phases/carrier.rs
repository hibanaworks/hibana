//! Test I/O controls, never choreography authority. The first loan, tick,
//! return and next loan each split acceptance from sender completion.
use super::*;
use crate::{
    test_support::transport::{FrameEvidence, TestRx, TestTx},
    transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
};
use core::task::{Context, Poll, Waker};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Receipt {
    Queued,
    Accept,
    Accepted,
    Complete,
    Finished,
    Cancelled,
}

struct Gate {
    receipt: Receipt,
    waker: Waker,
}

#[derive(Clone)]
pub(super) struct Network {
    inner: TestTransport,
    actor: u8,
    gates: Rc<RefCell<[Option<Gate>; 4]>>,
    pub(super) events: Rc<RefCell<Vec<Boundary>>>,
}

pub(super) struct Tx {
    raw: TestTx,
    packet: Option<(usize, usize)>,
    gate: Option<usize>,
    encoded: Vec<u8>,
    retired: bool,
}

pub(super) struct Rx<'a> {
    raw: TestRx<'a>,
    port: PortOpen,
}

impl Network {
    pub(super) fn new() -> Self {
        Self {
            inner: TestTransport::new(),
            actor: 0,
            gates: Rc::new(RefCell::new([None, None, None, None])),
            events: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub(super) fn for_actor(&self, actor: u8) -> Self {
        Self {
            actor,
            ..self.clone()
        }
    }

    pub(super) fn inner_queue_is_empty(&self) -> bool {
        self.inner.queue_is_empty()
    }

    pub(super) fn call(&self, kind: u8, operation: usize, visit: usize) {
        self.events.borrow_mut().push(Boundary {
            kind,
            operation,
            visit,
            actor: self.actor,
            wire: Vec::new(),
            witness: 0,
        });
    }

    pub(super) fn close_loan(&self, abort: bool) {
        let lane = crate::runtime::program::project::<1>(&g::Program::<OwnedParallel>::new())
            .role_image_ref()
            .local_step_lane(0)
            .expect("loan receive lane");
        // The checker independently requires a single incoming logical source
        // on this port. TestTransport's injector closes an entire receive port.
        self.events.borrow_mut().push(Boundary {
            kind: if abort { 5 } else { 4 },
            operation: 0,
            visit: 0,
            actor: 0,
            wire: vec![0, 1, lane],
            witness: 0,
        });
        if abort {
            self.inner.abort_port(SID, 1, lane);
        } else {
            self.inner.close_port(SID, 1, lane);
        }
    }

    pub(super) fn release(&self, command: u8) -> bool {
        let wake = {
            let mut gates = self.gates.borrow_mut();
            let Some(gate) = &mut gates[usize::from(command / 2)] else {
                return false;
            };
            let (expected, next) = if command.is_multiple_of(2) {
                (Receipt::Queued, Receipt::Accept)
            } else {
                (Receipt::Accepted, Receipt::Complete)
            };
            if gate.receipt != expected {
                return false;
            }
            gate.receipt = next;
            gate.waker.clone()
        };
        wake.wake();
        true
    }

    fn accepted(&self, tx: &Tx, receiver: u8) {
        let (origin, visit) = tx.packet.expect("staged send");
        let actual = self.inner.newest_accepted_frame(receiver);
        self.packet(1, origin, visit, &actual);
    }

    fn packet(&self, kind: u8, origin: usize, visit: usize, packet: &FrameEvidence) {
        let mut wire = packet.header.bytes().to_vec();
        wire.extend_from_slice(packet.payload());
        self.events.borrow_mut().push(Boundary {
            kind,
            operation: 2 * origin + usize::from(kind == 2),
            visit,
            actor: self.actor,
            wire,
            witness: packet.witness,
        });
    }
}

impl Transport for Network {
    type Tx<'a> = Tx;
    type Rx<'a> = Rx<'a>;

    fn initial_endpoint_binding(&self, role: u8) -> Option<&crate::transport::EndpointBinding> {
        if self.actor == 0 && role == 10 {
            self.inner.initial_endpoint_binding(role)
        } else {
            None
        }
    }

    fn open(&self, port: PortOpen) -> (Tx, Rx<'_>) {
        let (raw, rx) = self.inner.open(port);
        (
            Tx {
                raw,
                packet: None,
                gate: None,
                encoded: Vec::new(),
                retired: false,
            },
            Rx { raw: rx, port },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Tx,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        if tx.retired {
            return Poll::Ready(Err(TransportError::Offline));
        }
        let receiver = outgoing.target_role();
        if tx.packet.is_none() {
            let origin = outgoing.meta.eff_index.dense_ordinal();
            let visit = self
                .events
                .borrow()
                .iter()
                .filter(|event| event.kind == 1 && event.operation == 2 * origin)
                .count();
            tx.packet = Some((origin, visit));
            tx.encoded = outgoing.payload().as_bytes().to_vec();
            // Source event/visit names select only physical test I/O delays.
            // Endpoints still decide whether the actual operation is permitted.
            let gate = match (origin, visit) {
                (0, 0) => Some(0), // First loan.
                (3, 0) => Some(1), // Independent tick from the same owner actor.
                (2, 0) => Some(2), // Return can arrive before its sender completes.
                (0, 1) => Some(3), // Next loan can be accepted during that return.
                _ => None,
            };
            if let Some(index) = gate {
                let mut gates = self.gates.borrow_mut();
                if gates[index].is_none() {
                    gates[index] = Some(Gate {
                        receipt: Receipt::Queued,
                        waker: cx.waker().clone(),
                    });
                    tx.gate = Some(index);
                }
            }
            self.inner.stage_send(
                &mut tx.raw,
                receiver,
                outgoing.lane(),
                outgoing.frame_label().raw(),
                outgoing.payload().as_bytes(),
            );
        }
        assert_eq!(
            tx.encoded,
            outgoing.payload().as_bytes(),
            "repoll changed encoded bytes"
        );
        if let Some(index) = tx.gate {
            let receipt = {
                let mut gates = self.gates.borrow_mut();
                let gate = gates[index].as_mut().unwrap();
                gate.waker = cx.waker().clone();
                gate.receipt
            };
            match receipt {
                Receipt::Queued | Receipt::Accepted => return Poll::Pending,
                Receipt::Accept => {
                    match self.inner.poll_send_staged(&mut tx.raw) {
                        Poll::Ready(Ok(())) => {}
                        Poll::Ready(Err(error)) => {
                            let (origin, visit) = tx.packet.unwrap();
                            self.call(6, 2 * origin, visit);
                            self.gates.borrow_mut()[index].as_mut().unwrap().receipt =
                                Receipt::Cancelled;
                            return Poll::Ready(Err(error));
                        }
                        Poll::Pending => panic!("synchronous test queue returned Pending"),
                    }
                    self.accepted(tx, receiver);
                    self.gates.borrow_mut()[index].as_mut().unwrap().receipt = Receipt::Accepted;
                    return Poll::Pending;
                }
                Receipt::Complete => {
                    self.gates.borrow_mut()[index].as_mut().unwrap().receipt = Receipt::Finished;
                }
                Receipt::Finished | Receipt::Cancelled => panic!("retired I/O call repolled"),
            }
        } else {
            assert_eq!(
                self.inner.poll_send_staged(&mut tx.raw),
                Poll::Ready(Ok(()))
            );
            self.accepted(tx, receiver);
        }
        tx.packet = None;
        tx.gate = None;
        tx.encoded.clear();
        Poll::Ready(Ok(()))
    }

    fn cancel_send(&self, tx: &mut Tx) {
        if let Some(index) = tx.gate {
            self.gates.borrow_mut()[index].as_mut().unwrap().receipt = Receipt::Cancelled;
        }
        self.inner.cancel_send_staged(&mut tx.raw);
        tx.retired = true;
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        let result = self.inner.poll_recv_current(&mut rx.raw, cx);
        if matches!(result, Poll::Ready(Ok(_))) {
            let packet = self.inner.received_frame(
                rx.port.local_role(),
                rx.port.session_id(),
                rx.port.lane(),
            );
            let (origin, visit) = {
                let events = self.events.borrow();
                let sent = events
                    .iter()
                    .find(|event| event.kind == 1 && event.witness == packet.witness)
                    .expect("actual acceptance witness");
                (sent.operation / 2, sent.visit)
            };
            self.packet(2, origin, visit, &packet);
        } else if matches!(result, Poll::Ready(Err(_))) {
            assert_eq!(rx.port.local_role(), 1, "only the loan port was closed");
            let visit = self
                .events
                .borrow()
                .iter()
                .rfind(|event| event.kind == 0 && event.actor == self.actor && event.operation == 1)
                .expect("started loan receive")
                .visit;
            self.call(6, 1, visit);
        }
        result
    }

    fn requeue(&self, rx: &mut Rx<'_>) -> Result<(), TransportError> {
        self.inner.requeue(&mut rx.raw)
    }
}
