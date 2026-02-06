//! Custom Binding Example
//!
//! Demonstrates how to implement the `BindingSlot` trait to create a custom
//! protocol binder. This example simulates a packet-based transport where
//! each message is prefixed with a 1-byte label header.
//!
//! The binder automatically:
//! 1. Prepends the label byte on send (inferred from choreography metadata).
//! 2. Peeks the label byte on receive to classify the incoming message.
//! 3. Routes the message to the correct choreography branch based on the label.
//!
//! Run with:
//! ```bash
//! cargo run --example custom_binding --features std
//! ```

#![cfg(feature = "std")]

use hibana::{
    binding::{BindingSlot, Channel, IncomingClassification, SendDisposition, SendMetadata, TransportOpsError},
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    transport::{Transport, TransportError, wire::Payload},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, RING_EVENTS},
    },
    rendezvous::{Rendezvous, SessionId},
    observe::TapEvent,
    endpoint::ControlOutcome,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;

// 1. Define Roles
type Sender = Role<0>;
type Receiver = Role<1>;

// 2. Define Messages with explicit labels
type IntMsg = Msg<1, u32>;

// 3. Define Protocol steps
type ProtocolSteps = StepCons<SendStep<Sender, Receiver, IntMsg, 0>, StepNil>;

// 4. Define Global Protocol
const PROTOCOL: g::Program<ProtocolSteps> = g::send::<Sender, Receiver, IntMsg, 0>();

// 5. Project to Role Programs
type SenderLocal = <ProtocolSteps as ProjectRole<Sender>>::Output;
type ReceiverLocal = <ProtocolSteps as ProjectRole<Receiver>>::Output;

const SENDER_PROG: g::RoleProgram<'static, 0, SenderLocal> =
    g::project::<0, ProtocolSteps, _>(&PROTOCOL);
const RECEIVER_PROG: g::RoleProgram<'static, 1, ReceiverLocal> =
    g::project::<1, ProtocolSteps, _>(&PROTOCOL);

// 6. Mock Transport
#[derive(Clone, Default)]
struct MockTransport {
    queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl Transport for MockTransport {
    type Error = TransportError;
    type Tx<'a> = ();
    type Rx<'a> = ();
    type Send<'a> = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>>;
    type Recv<'a> = Pin<Box<dyn Future<Output = Result<Payload<'a>, Self::Error>> + Send + 'a>>;
    type Metrics = hibana::transport::NoopMetrics;

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        payload: Payload<'f>,
        _dest_role: u8,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        let queue = self.queue.clone();
        let data = payload.as_bytes().to_vec();
        Box::pin(async move {
            queue.lock().unwrap().push_back(data);
            Ok(())
        })
    }

    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        let queue = self.queue.clone();
        Box::pin(async move {
            loop {
                let item = queue.lock().unwrap().pop_front();
                if let Some(data) = item {
                    let leaked = Box::leak(data.into_boxed_slice());
                    return Ok(Payload::new(leaked));
                }
                std::thread::yield_now();
            }
        })
    }
}

// 7. Custom BindingSlot
struct LabelHeaderBinder {
    peeked: Option<Vec<u8>>,
    transport: MockTransport,
}

impl LabelHeaderBinder {
    fn new(transport: MockTransport) -> Self {
        Self {
            peeked: None,
            transport,
        }
    }
}

// SAFETY: LabelHeaderBinder uses a synchronous mutex-guarded queue for buffering.
// No network I/O is awaited; data is enqueued immediately.
unsafe impl BindingSlot for LabelHeaderBinder {
    fn on_send_with_meta(
        &mut self,
        _meta: SendMetadata,
        _payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError> {
        // Payload is sent via Transport; this binder only classifies/unwraps.
        Ok(SendDisposition::BypassTransport)
    }

    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        if self.peeked.is_none() {
            let mut queue = self.transport.queue.lock().unwrap();
            self.peeked = queue.pop_front();
        }

        if let Some(packet) = &self.peeked {
            if packet.len() < 2 {
                return None;
            }
            Some(IncomingClassification {
                label: packet[0],
                instance: 0,
                has_fin: false,
                channel: Channel::new(0),
            })
        } else {
            None
        }
    }

    fn on_recv(&mut self, _channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        if let Some(packet) = self.peeked.take() {
            if packet.len() < 2 {
                return Err(TransportOpsError::InvalidState);
            }
            let payload_len = packet.len() - 1;
            if buf.len() < payload_len {
                return Err(TransportOpsError::WriteFailed {
                    expected: payload_len,
                    actual: buf.len()
                });
            }
            buf[..payload_len].copy_from_slice(&packet[1..]);
            Ok(payload_len)
        } else {
            Err(TransportOpsError::InvalidState)
        }
    }
}

type Cluster = SessionCluster<'static, MockTransport, DefaultLabelUniverse, CounterClock, 4>;

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}

fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

fn run_demo() {
    let wire = MockTransport::default();

    // --- Receiver Side ---
    let receiver_wire = wire.clone();
    let receiver_thread = std::thread::Builder::new()
        .name("receiver".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            futures::executor::block_on(async {
                let cluster: &'static Cluster =
                    Box::leak(Box::new(SessionCluster::new(leak_clock())));
                let config = Config::new(leak_tap_storage(), leak_slab(1024));
                let rv = Rendezvous::from_config(config, receiver_wire.clone());
                let rv_id = cluster.add_rendezvous(rv).unwrap();

                let binder = LabelHeaderBinder::new(receiver_wire);

                let endpoint = cluster.attach_cursor::<1, _, _, _>(
                    rv_id,
                    SessionId::new(100),
                    &RECEIVER_PROG,
                    binder,
                ).unwrap();

                println!("[Receiver] Waiting for message...");

                let (_ep, val) = endpoint.recv::<IntMsg>().await.unwrap();
                println!("[Receiver] Received Int: {}", val);

                #[cfg(feature = "test-utils")]
                ep.phase_cursor().assert_terminal();
                println!("[Receiver] Protocol completed");
            });
        })
        .expect("spawn receiver thread");

    // --- Sender Side ---
    std::thread::sleep(std::time::Duration::from_millis(100));

    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let config = Config::new(leak_tap_storage(), leak_slab(1024));
    let rv = Rendezvous::from_config(config, wire.clone());
    let rv_id = cluster.add_rendezvous(rv).unwrap();

    let binder = LabelHeaderBinder::new(wire);

    let endpoint = cluster.attach_cursor::<0, _, _, _>(
        rv_id,
        SessionId::new(100),
        &SENDER_PROG,
        binder,
    ).unwrap();

    futures::executor::block_on(async {
        println!("[Sender] Sending IntMsg...");
        let (_endpoint, outcome) = endpoint.flow::<IntMsg>().unwrap().send(&42).await.unwrap();
        assert!(matches!(outcome, ControlOutcome::None));
        println!("[Sender] Sent IntMsg");

        #[cfg(feature = "test-utils")]
        endpoint.phase_cursor().assert_terminal();
        println!("[Sender] Protocol completed");
    });

    receiver_thread.join().unwrap();

    println!("\n=== Custom Binding Example Completed Successfully ===");
}

fn main() {
    std::thread::Builder::new()
        .name("hibana-custom-binding".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run_demo)
        .expect("spawn main thread")
        .join()
        .expect("main thread panicked");
}
