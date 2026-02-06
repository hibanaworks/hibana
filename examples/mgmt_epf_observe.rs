//! EPF Choreography Observation via Management Session
//!
//! Demonstrates the relationship between hibana choreography execution and EPF
//! observation through the **Dual-Ring TapRing Architecture**.
//!
//! # Key Concepts
//!
//! 1. **Choreography → Infra Ring**: `flow().send()` emits `ENDPOINT_SEND` (id >= 0x0100)
//! 2. **EPF TAP_OUT → User Ring**: Policy emits `TAP_OUT` (id < 0x0100) for Ping/Pong
//! 3. **Observer Effect Prevention**: Streaming observes **only User Ring**, so it
//!    doesn't see the `ENDPOINT_SEND` events that streaming itself generates
//!
//! # Dual-Ring Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ Choreography: Ping-Pong Protocol                                    │
//! │                                                                     │
//! │   Client ──── flow::<Ping>().send() ────▶ Server                   │
//! │          ◀─── flow::<Pong>().send() ────                           │
//! │                      │                                              │
//! │                      ▼                                              │
//! │            ENDPOINT_SEND (0x0202) → Infra Ring                     │
//! └─────────────────────────────────────────────────────────────────────┘
//!                          │
//!                          ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ EPF Policy (installed via Management Session)                       │
//! │                                                                     │
//! │   On ENDPOINT_SEND where label == Ping/Pong:                        │
//! │     → TAP_OUT 0x0001 → User Ring                                   │
//! └─────────────────────────────────────────────────────────────────────┘
//!                          │
//!                          ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ Dual-Ring TapRing                                                   │
//! │                                                                     │
//! │   User Ring (0x0000-0x00FF)   │   Infra Ring (0x0100-0xFFFF)       │
//! │   ─────────────────────────   │   ──────────────────────────        │
//! │   TAP_OUT 0x0001 (EPF)        │   ENDPOINT_SEND 0x0202              │
//! │                               │   ENDPOINT_RECV 0x0203              │
//! │                               │   LANE_ACQUIRE 0x0210               │
//! │                               │                                     │
//! │   ↓ Streaming observes this   │   ✗ Not visible to streaming       │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Management Session Flow
//!
//! ```text
//! Controller (Remote)                    Cluster (Local)
//!      │                                      │
//!      │── LoadBegin + LoadChunk + Commit ──▶│  1. Install EPF policy
//!      │                                      │
//!      │── Subscribe ───────────────────────▶│  2. Start streaming
//!      │                                      │     (observes User Ring only)
//!      │                                      │
//!      │     ┌────────────────────────────────│  3. Workload runs
//!      │     │ Ping-Pong choreography         │     flow().send() fires
//!      │     │ → EPF evaluates ENDPOINT_SEND  │     → TAP_OUT to User Ring
//!      │     └────────────────────────────────│
//!      │                                      │
//!      │◀── TapBatch (TAP_OUT events) ───────│  4. Stream TAP_OUT in real-time
//!      │                                      │
//! ```
//!
//! # Observer Effect Prevention
//!
//! Without Dual-Ring separation, streaming would see its own infrastructure events:
//! - Stream sends TapBatch → generates ENDPOINT_SEND
//! - ENDPOINT_SEND appears in ring → triggers another stream send
//! - Infinite feedback loop!
//!
//! With Dual-Ring: streaming observes User Ring (TAP_OUT only), Infra Ring events
//! (ENDPOINT_SEND from streaming itself) are invisible to the stream.
//!
//! Run with:
//! ```bash
//! cargo run --example mgmt_epf_observe --features std
//! ```

#![cfg(feature = "std")]

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use hibana::{
    NoBinding,
    control::cap::{
        CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN,
        CapShot, GenericCapToken, ResourceKind,
        resource_kinds::{LoadBeginKind, LoadCommitKind},
    },
    epf::{Slot, ops, verifier::compute_hash},
    g::{self, Msg, Role, steps::{ProjectRole, SendStep, StepCons, StepNil}},
    observe::{self, TapEvent},
    rendezvous::{Lane, Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, RING_EVENTS},
        mgmt::{
            Command, LOAD_CHUNK_MAX, LoadBegin, LoadChunk, Reply, SubscribeReq,
            session::{self, ControllerPlan, StreamControl},
        },
    },
    transport::{Transport, TransportError, TransportMetrics, TransportSnapshot, wire::Payload},
};

// =============================================================================
// Live Telemetry State (Collected via Management Session Streaming)
// =============================================================================

static TELEMETRY_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static TAP_OUT_COUNT: AtomicU32 = AtomicU32::new(0);
static TOTAL_EVENTS_RECEIVED: AtomicU32 = AtomicU32::new(0);

const CHANNEL_RETRY_WARNING: u16 = 0x0001;

/// Format elapsed time as MM:SS.mmm
fn format_elapsed(start: Instant) -> String {
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = elapsed.subsec_millis();
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}

/// Process a TapEvent received via Management Session streaming.
/// Only counts and displays EPF TAP_OUT events (EPF filters Ping/Pong for us).
fn process_streamed_event(event: TapEvent) {
    // Count all events received
    TOTAL_EVENTS_RECEIVED.fetch_add(1, Ordering::Relaxed);

    // Only process EPF TAP_OUT events — EPF already filters to Ping/Pong
    if event.id != CHANNEL_RETRY_WARNING {
        return;
    }

    let start = TELEMETRY_START.get_or_init(Instant::now);
    let count = EVENT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    TAP_OUT_COUNT.fetch_add(1, Ordering::Relaxed);

    // TAP_OUT from EPF policy: arg0 = session_id, arg1 = label (1=Ping, 2=Pong)
    let sid = event.arg0;
    let label = event.arg1;
    let msg_name = match label {
        1 => "Ping",
        2 => "Pong",
        _ => "?",
    };

    let elapsed = format_elapsed(*start);
    println!("  [{elapsed}] EPF  sid=0x{:04X} {msg_name}", sid);

    // Periodic stats (every 5 events)
    if count % 5 == 0 {
        println!("  ─────────────────────────────────────────────────");
        println!("  EPF TAP_OUT: {} events captured", count);
        println!("  ─────────────────────────────────────────────────");
    }
}

// =============================================================================
// Protocol Definition (Ping-Pong for workload generation)
// =============================================================================

type Client = Role<0>;
type Server = Role<1>;

type Ping = Msg<1, u32>;
type Pong = Msg<2, u32>;

type ProtocolSteps = StepCons<
    SendStep<Client, Server, Ping>,
    StepCons<SendStep<Server, Client, Pong>, StepNil>,
>;

const PROTOCOL: g::Program<ProtocolSteps> = g::seq(
    g::send::<Client, Server, Ping, 0>(),
    g::send::<Server, Client, Pong, 0>(),
);

type ClientLocal = <ProtocolSteps as ProjectRole<Client>>::Output;
type ServerLocal = <ProtocolSteps as ProjectRole<Server>>::Output;

const CLIENT_PROGRAM: g::RoleProgram<'static, 0, ClientLocal> =
    g::project::<0, ProtocolSteps, _>(&PROTOCOL);
const SERVER_PROGRAM: g::RoleProgram<'static, 1, ServerLocal> =
    g::project::<1, ProtocolSteps, _>(&PROTOCOL);

// =============================================================================
// IsolatedTransport - Session-aware message routing
// =============================================================================

use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

#[derive(Clone)]
struct FrameOwned {
    payload: Vec<u8>,
}

/// Key for message routing: (session_id, role)
type QueueKey = (u32, u8);

use std::collections::HashSet;

#[derive(Default)]
struct IsolatedState {
    /// Message queues keyed by (session_id, dest_role)
    queues: HashMap<QueueKey, VecDeque<FrameOwned>>,
    /// Waiters keyed by (session_id, role)
    waiters: HashMap<QueueKey, Vec<Waker>>,
    /// Roles that are using BindingSlot (frames should be consumed via poll_incoming/on_recv)
    binding_owned: HashSet<QueueKey>,
    /// Pending labels keyed by (session_id, dest_role) - scoped to avoid races
    pending_labels: HashMap<QueueKey, u8>,
}

impl IsolatedState {
    fn ensure_session_role(&mut self, session_id: u32, role: u8) {
        self.queues.entry((session_id, role)).or_default();
    }

    fn enqueue(&mut self, key: QueueKey, frame: FrameOwned) -> Vec<Waker> {
        let queue = self.queues.entry(key).or_default();
        queue.push_back(frame);
        self.waiters.remove(&key).unwrap_or_default()
    }

    fn dequeue(&mut self, key: QueueKey) -> Option<FrameOwned> {
        self.queues.get_mut(&key).and_then(|q| q.pop_front())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IsolatedTransportError;

impl From<IsolatedTransportError> for TransportError {
    fn from(_: IsolatedTransportError) -> Self {
        TransportError::Failed
    }
}

/// Session-aware transport that routes messages by (session_id, role).
/// Supports label prefixing for BindingSlot integration.
#[derive(Clone)]
pub struct IsolatedTransport {
    state: Arc<Mutex<IsolatedState>>,
    send_count: Arc<AtomicU64>,
}

impl IsolatedTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(IsolatedState::default())),
            send_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Set a pending label for the next send to (session_id, dest_role).
    /// Scoped per destination to avoid races between concurrent sessions.
    pub fn set_pending_label(&self, session_id: u32, dest_role: u8, label: u8) {
        let key = (session_id, dest_role);
        let mut state = self.state.lock().expect("state lock");
        state.pending_labels.insert(key, label);
    }

    /// Take (consume) the pending label for (session_id, dest_role) if set.
    fn take_pending_label(&self, session_id: u32, dest_role: u8) -> Option<u8> {
        let key = (session_id, dest_role);
        let mut state = self.state.lock().expect("state lock");
        state.pending_labels.remove(&key)
    }

    #[allow(dead_code)]
    pub fn send_count(&self) -> u64 {
        self.send_count.load(Ordering::Relaxed)
    }

    pub fn try_recv(&self, session_id: u32, role: u8) -> Option<Vec<u8>> {
        let key = (session_id, role);
        let mut state = self.state.lock().expect("state lock");
        state.dequeue(key).map(|f| f.payload)
    }

    pub fn peek(&self, session_id: u32, role: u8) -> Option<Vec<u8>> {
        let key = (session_id, role);
        let state = self.state.lock().expect("state lock");
        state.queues.get(&key).and_then(|q| q.front()).map(|f| f.payload.clone())
    }

    /// Mark a (session_id, role) as binding-owned.
    /// When binding-owned, IsolatedRecvFuture returns empty payload to let binding handle frames.
    pub fn mark_binding_owned(&self, session_id: u32, role: u8) {
        let key = (session_id, role);
        let mut state = self.state.lock().expect("state lock");
        state.binding_owned.insert(key);
    }

}

pub struct IsolatedTx {
    session_id: u32,
}

pub struct IsolatedRx {
    state: Arc<Mutex<IsolatedState>>,
    session_id: u32,
    role: u8,
}

pub struct IsolatedRecvFuture<'a> {
    state: Arc<Mutex<IsolatedState>>,
    session_id: u32,
    role: u8,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Future for IsolatedRecvFuture<'a> {
    type Output = Result<Payload<'a>, IsolatedTransportError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let key = (self.session_id, self.role);
        let mut state = self.state.lock().expect("state lock");

        // Check if this session/role is binding-owned
        let is_binding_owned = state.binding_owned.contains(&key);

        if let Some(queue) = state.queues.get_mut(&key) {
            if !queue.is_empty() {
                if is_binding_owned {
                    // Binding-owned: DON'T dequeue, just signal data is available.
                    drop(state);
                    return Poll::Ready(Ok(Payload::new(&[])));
                } else {
                    // Not binding-owned: dequeue and return the full payload.
                    let frame = queue.pop_front().expect("checked non-empty");
                    drop(state);
                    let payload = frame.payload.into_boxed_slice();
                    let leaked = Box::leak(payload);
                    return Poll::Ready(Ok(Payload::new(leaked)));
                }
            }
        }
        state.waiters.entry(key).or_default().push(cx.waker().clone());
        Poll::Pending
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IsolatedTransportMetrics;

impl TransportMetrics for IsolatedTransportMetrics {
    fn latency_us(&self) -> Option<u64> {
        None
    }

    fn queue_depth(&self) -> Option<u32> {
        None
    }

    fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot::default()
    }
}

impl Transport for IsolatedTransport {
    type Error = IsolatedTransportError;
    type Tx<'a> = IsolatedTx where Self: 'a;
    type Rx<'a> = IsolatedRx where Self: 'a;
    type Send<'a> = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> where Self: 'a;
    type Recv<'a> = IsolatedRecvFuture<'a> where Self: 'a;
    type Metrics = IsolatedTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let mut state = self.state.lock().expect("state lock");
        state.ensure_session_role(session_id, local_role);
        drop(state);
        (
            IsolatedTx { session_id },
            IsolatedRx {
                state: self.state.clone(),
                session_id,
                role: local_role,
            },
        )
    }

    fn send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        payload: Payload<'f>,
        dest_role: u8,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        self.send_count.fetch_add(1, Ordering::Relaxed);
        // Check if binding set a pending label to prefix for this (session_id, dest_role)
        let pending_label = self.take_pending_label(tx.session_id, dest_role);
        let payload_vec = if let Some(label) = pending_label {
            // Prefix the payload with the label byte
            let mut v = Vec::with_capacity(1 + payload.as_bytes().len());
            v.push(label);
            v.extend_from_slice(payload.as_bytes());
            v
        } else {
            payload.as_bytes().to_vec()
        };
        let state = self.state.clone();
        let key = (tx.session_id, dest_role);

        Box::pin(async move {
            let waiters = {
                let mut guard = state.lock().expect("state lock");
                guard.enqueue(key, FrameOwned { payload: payload_vec })
            };
            for waker in waiters {
                waker.wake();
            }
            // Yield to allow other tasks to run
            tokio::task::yield_now().await;
            Ok(())
        })
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        IsolatedRecvFuture {
            state: rx.state.clone(),
            session_id: rx.session_id,
            role: rx.role,
            _marker: PhantomData,
        }
    }

    fn metrics(&self) -> Self::Metrics {
        IsolatedTransportMetrics
    }
}

// =============================================================================
// MgmtBinding - Adds label header for stream control
// =============================================================================

use hibana::binding::{BindingSlot, Channel, IncomingClassification, LocalDirection, SendDisposition, SendMetadata, TransportOpsError};

struct MgmtBinding {
    transport: IsolatedTransport,
    session_id: u32,
    local_role: u8,
}

#[derive(Clone, Copy)]
struct LaneBinding {
    lane: Lane,
}

impl LaneBinding {
    fn new(lane: Lane) -> Self {
        Self { lane }
    }
}

// SAFETY: LaneBinding performs no I/O; only remaps lanes.
unsafe impl BindingSlot for LaneBinding {
    fn on_send_with_meta(
        &mut self,
        _meta: SendMetadata,
        _payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError> {
        Ok(SendDisposition::BypassTransport)
    }

    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        None
    }

    fn on_recv(&mut self, _channel: Channel, _buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        Ok(0)
    }

    fn map_lane(&self, _logical_lane: u8) -> Lane {
        self.lane
    }
}

impl MgmtBinding {
    fn new(transport: IsolatedTransport, session_id: u32, local_role: u8) -> Self {
        // Mark this session/role as binding-owned so IsolatedRecvFuture won't consume frames
        transport.mark_binding_owned(session_id, local_role);
        Self {
            transport,
            session_id,
            local_role,
        }
    }
}

// SAFETY: MgmtBinding only sets a pending label in a concurrent HashMap.
// No network I/O is awaited; Transport::send() handles actual transmission.
unsafe impl BindingSlot for MgmtBinding {
    fn on_send_with_meta(
        &mut self,
        meta: SendMetadata,
        _payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError> {
        // Only set pending label for cross-role sends (not self-send)
        if meta.direction == LocalDirection::Send {
            // Scoped by (session_id, dest_role) to avoid races between concurrent sessions
            self.transport.set_pending_label(self.session_id, meta.peer, meta.label);
        }
        // BypassTransport: let hibana core call Transport::send() to handle wire I/O
        Ok(SendDisposition::BypassTransport)
    }

    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        // Peek the next frame — first byte is the label
        if let Some(frame) = self.transport.peek(self.session_id, self.local_role) {
            if !frame.is_empty() {
                let label = frame[0];
                return Some(IncomingClassification {
                    label,
                    instance: 0,
                    has_fin: false,
                    channel: Channel::new(0),
                });
            }
        }
        None
    }

    fn on_recv(&mut self, _channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        // Read the frame and skip the first byte (label)
        if let Some(frame) = self.transport.try_recv(self.session_id, self.local_role) {
            if frame.is_empty() {
                return Ok(0);
            }
            // Skip label byte, return payload
            let payload = &frame[1..];
            if payload.len() > buf.len() {
                return Err(TransportOpsError::WriteFailed {
                    expected: payload.len(),
                    actual: buf.len(),
                });
            }
            buf[..payload.len()].copy_from_slice(payload);
            Ok(payload.len())
        } else {
            Err(TransportOpsError::OpenFailed)
        }
    }
}

// =============================================================================
// EPF Observation Policy
// =============================================================================

/// Labels for Ping and Pong messages (used by EPF policy bytecode below)
#[allow(dead_code)]
const LABEL_PING: u8 = 1;
#[allow(dead_code)]
const LABEL_PONG: u8 = 2;

/// EPF policy: emit TAP_OUT only for Ping/Pong messages (labels 1 and 2)
///
/// arg1 layout: | role:8 | lane:8 | label:8 | flags:8 |
/// label is at bits 8-15, extracted via: (arg1 >> 8) & 0xFF
///
/// Optimized bytecode using JUMP_EQ_IMM and AND_IMM (25 bytes vs 47 bytes)
#[rustfmt::skip]
const OBSERVE_POLICY: &[u8] = &[
    // offset 0: GET_EVENT_ARG1 r0 (2 bytes) -> r0 = arg1
    ops::instr::GET_EVENT_ARG1, 0x00,

    // offset 2: SHR r1, r0, 8 (4 bytes) -> r1 = arg1 >> 8
    ops::instr::SHR, 0x01, 0x00, 8,

    // offset 6: AND_IMM r1, r1, 0xFF (4 bytes) -> r1 = label
    ops::instr::AND_IMM, 0x01, 0x01, 0xFF,

    // offset 10: JUMP_EQ_IMM r1, 1, 21 (5 bytes) -> if label == Ping, goto emit
    ops::instr::JUMP_EQ_IMM, 0x01, 0x01, 21, 0x00,

    // offset 15: JUMP_EQ_IMM r1, 2, 21 (5 bytes) -> if label == Pong, goto emit
    ops::instr::JUMP_EQ_IMM, 0x01, 0x02, 21, 0x00,

    // offset 20: HALT (1 byte) -> skip (not Ping/Pong)
    ops::instr::HALT,

    // emit:
    // offset 21: GET_EVENT_ARG0 r2 (2 bytes) -> r2 = session_id
    ops::instr::GET_EVENT_ARG0, 0x02,

    // offset 23: TAP_OUT id=1, r2, r1 (5 bytes) -> emit (sid, label)
    ops::instr::TAP_OUT, 0x01, 0x00, 0x02, 0x01,

    // offset 28: HALT (1 byte)
    ops::instr::HALT,
];

// =============================================================================
// Helper Functions
// =============================================================================

type Cluster = SessionCluster<'static, IsolatedTransport, DefaultLabelUniverse, CounterClock, 4>;

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}

fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

fn slot_to_u8(slot: Slot) -> u8 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_nonce() -> [u8; CAP_NONCE_LEN] {
    let value = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut nonce = [0u8; CAP_NONCE_LEN];
    nonce[..8].copy_from_slice(&value.to_be_bytes());
    nonce[8..16].copy_from_slice(&(!value).to_be_bytes());
    nonce
}

const CAP_FIXED_HEADER_LEN: usize = 10;

fn base_header(sid: SessionId, lane: Lane, role: u8, tag: u8) -> [u8; CAP_HEADER_LEN] {
    let mut header = [0u8; CAP_HEADER_LEN];
    header[..4].copy_from_slice(&sid.raw().to_be_bytes());
    let lane_raw = lane.raw();
    header[4] = lane_raw as u8;
    header[5] = role;
    header[6] = tag;
    header[7] = CapShot::One.as_u8();
    header
}

fn make_load_begin_token(
    slot: Slot,
    hash: u32,
    sid: SessionId,
    lane: Lane,
) -> GenericCapToken<LoadBeginKind> {
    let nonce = next_nonce();
    let mut header = base_header(sid, lane, session::ROLE_CONTROLLER, LoadBeginKind::TAG);
    let handle = (slot_to_u8(slot), u64::from(hash));
    let mask_bits = LoadBeginKind::caps_mask(&handle).bits();
    header[8..10].copy_from_slice(&mask_bits.to_be_bytes());
    let handle_bytes = LoadBeginKind::encode_handle(&handle);
    header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; CAP_TAG_LEN])
}

fn make_load_commit_token(slot: Slot, sid: SessionId, lane: Lane) -> GenericCapToken<LoadCommitKind> {
    let nonce = next_nonce();
    let mut header = base_header(sid, lane, session::ROLE_CONTROLLER, LoadCommitKind::TAG);
    let handle = slot_to_u8(slot);
    let mask_bits = LoadCommitKind::caps_mask(&handle).bits();
    header[8..10].copy_from_slice(&mask_bits.to_be_bytes());
    let handle_bytes = LoadCommitKind::encode_handle(&handle);
    header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; CAP_TAG_LEN])
}

fn make_load_begin(slot: Slot, code: &[u8]) -> LoadBegin {
    LoadBegin {
        slot: slot_to_u8(slot),
        code_len: code.len() as u32,
        fuel_max: 10000, // Sufficient for many policy evaluations before reset
        mem_len: 256,
        hash: compute_hash(code),
    }
}

fn make_chunk(offset: u32, code: &[u8], is_last: bool) -> LoadChunk {
    assert!(code.len() <= LOAD_CHUNK_MAX);
    let mut bytes = [0u8; LOAD_CHUNK_MAX];
    if !code.is_empty() {
        bytes[..code.len()].copy_from_slice(code);
    }
    LoadChunk {
        offset,
        len: code.len() as u16,
        is_last,
        bytes,
    }
}

fn build_controller_plan(sid: SessionId, lane: Lane, slot: Slot) -> ControllerPlan<'static> {
    let code = OBSERVE_POLICY;
    let chunk = make_chunk(0, code, true);
    let chunks: &'static [LoadChunk] = Box::leak(Box::new([chunk]));
    let load_begin = make_load_begin(slot, code);
    let load_token = make_load_begin_token(slot, load_begin.hash, sid, lane);
    let commit_token = make_load_commit_token(slot, sid, lane);

    ControllerPlan {
        load_token,
        load_begin,
        chunks,
        commit_token,
        command: Command::Activate { slot },
    }
}

// =============================================================================
// Management Loop Resolver
// =============================================================================

use std::sync::LazyLock;
use hibana::control::types::RendezvousId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LoopKey {
    rv: RendezvousId,
    session: hibana::control::types::SessionId,
    lane: hibana::control::types::LaneId,
}

#[derive(Clone, Copy, Debug)]
struct LoopSchedule {
    total: usize,
    sent: usize,
}

static MGMT_LOOP_SCHEDULE: LazyLock<Mutex<HashMap<LoopKey, LoopSchedule>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn register_mgmt_loop_resolvers(cluster: &Cluster, rv_id: RendezvousId) {
    use hibana::{
        control::cluster::{DynamicResolution, ResolverContext},
        global::const_dsl::DynamicMeta,
    };

    fn mgmt_loop_resolver(
        _cluster: &Cluster,
        _meta: &DynamicMeta,
        ctx: ResolverContext,
    ) -> Result<DynamicResolution, ()> {
        let session = ctx.session.ok_or(())?;
        let key = LoopKey {
            rv: ctx.rv_id,
            session,
            lane: ctx.lane,
        };
        let mut schedules = MGMT_LOOP_SCHEDULE.lock().expect("lock");
        let schedule = schedules.get_mut(&key).ok_or(())?;
        if schedule.sent >= schedule.total {
            return Err(());
        }
        let decision = schedule.sent + 1 < schedule.total;
        schedule.sent += 1;
        if schedule.sent == schedule.total {
            schedules.remove(&key);
        }
        Ok(DynamicResolution::Loop { decision })
    }

    for info in session::CONTROLLER_PROGRAM.control_plans() {
        if info.plan.is_dynamic() {
            cluster
                .register_control_plan_resolver(rv_id, &info, mgmt_loop_resolver)
                .expect("register resolver");
        }
    }
}

fn reset_mgmt_loop_resolver(rv_id: RendezvousId, sid: SessionId, lane: Lane, total_chunks: usize) {
    use hibana::control::types::{LaneId, SessionId as CpSessionId};
    let key = LoopKey {
        rv: rv_id,
        session: CpSessionId::new(sid.raw()),
        lane: LaneId::new(lane.raw()),
    };
    let mut schedules = MGMT_LOOP_SCHEDULE.lock().expect("lock");
    schedules.insert(key, LoopSchedule { total: total_chunks, sent: 0 });
}

// =============================================================================
// Main Demo
// =============================================================================

async fn run_demo() {
    println!();
    println!("==================================================================");
    println!("     Hibana Live Telemetry Dashboard                             ");
    println!("     Management Session Streaming (Wire Protocol)                ");
    println!("==================================================================");
    println!();

    // Initialize timing
    TELEMETRY_START.get_or_init(Instant::now);

    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

    // =========================================================================
    // Single Rendezvous with IsolatedTransport that routes by (session, lane, role)
    // This ensures messages don't mix between different sessions
    // =========================================================================

    let transport = IsolatedTransport::new();
    let config = Config::new(leak_tap_storage(), leak_slab(4096)).with_lane_range(0..2);
    let rendezvous: Rendezvous<'_, '_, IsolatedTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport.clone());
    let rv_id = cluster.add_rendezvous(rendezvous).expect("add rendezvous");

    // Install tap ring globally
    let tap = cluster.get_local(&rv_id).expect("get rendezvous").tap();
    let tap_static = unsafe { tap.assume_static() };
    let _previous_ring = observe::install_ring(tap_static);

    // =========================================================================
    // Phase 1: Install EPF policy via management session (EndpointTx only)
    // =========================================================================
    println!("Phase 1: Installing EPF policy on EndpointTx");
    println!("------------------------------------------------------------");

    register_mgmt_loop_resolvers(cluster, rv_id);

    // Install EPF policy on EndpointTx slot
    {
        let mgmt_sid = SessionId::new(0x1000);
        let mgmt_lane = Lane::new(0);
        let controller_plan = build_controller_plan(mgmt_sid, mgmt_lane, Slot::EndpointTx);
        reset_mgmt_loop_resolver(rv_id, mgmt_sid, mgmt_lane, controller_plan.chunks.len());

        let controller_endpoint = cluster
            .attach_cursor::<{ session::ROLE_CONTROLLER }, _, _, _>(
                rv_id, mgmt_sid, &session::CONTROLLER_PROGRAM, NoBinding,
            )
            .expect("attach controller");

        let cluster_endpoint = cluster
            .attach_cursor::<{ session::ROLE_CLUSTER }, _, _, _>(
                rv_id, mgmt_sid, &session::CLUSTER_PROGRAM, NoBinding,
            )
            .expect("attach cluster");

        let (controller_result, cluster_result) = tokio::join!(
            async { session::drive_controller(controller_endpoint, controller_plan).await },
            async { cluster.init_mgmt(rv_id, mgmt_sid, mgmt_lane, cluster_endpoint).await }
        );

        let _controller_cursor = controller_result.expect("controller succeeded");
        let (cluster_cursor, seed) = cluster_result.expect("cluster init succeeded");
        drop(cluster_cursor);

        let reply = cluster.drive_mgmt(rv_id, mgmt_sid, seed).expect("drive mgmt");
        match reply {
            Reply::Activated(report) => {
                println!("  EPF policy activated on EndpointTx: version={}", report.version);
            }
            other => panic!("expected Activated, got {:?}", other),
        }
    }

    println!();

    // =========================================================================
    // Phase 2: Start streaming AND workload CONCURRENTLY
    // =========================================================================
    println!();
    println!("Phase 2: Real-time streaming + concurrent workload");
    println!("------------------------------------------------------------");
    println!("  Streaming and Ping-Pong run concurrently.");
    println!();

    let stream_sid = SessionId::new(0x3000);

    // Attach streaming session endpoints
    let stream_controller = cluster
        .attach_cursor::<{ session::ROLE_CONTROLLER }, _, _, _>(
            rv_id,
            stream_sid,
            &session::STREAM_CONTROLLER_PROGRAM,
            MgmtBinding::new(transport.clone(), stream_sid.raw(), session::ROLE_CONTROLLER),
        )
        .expect("attach stream controller");

    let stream_cluster = cluster
        .attach_cursor::<{ session::ROLE_CLUSTER }, _, _, _>(
            rv_id,
            stream_sid,
            &session::STREAM_CLUSTER_PROGRAM,
            MgmtBinding::new(transport.clone(), stream_sid.raw(), session::ROLE_CLUSTER),
        )
        .expect("attach stream cluster");

    // =========================================================================
    // Run streaming AND Ping-Pong workload CONCURRENTLY
    // =========================================================================

    // Workload parameters
    let total_rounds = 10u32;
    let workload_complete = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let workload_complete_clone = workload_complete.clone();
    // Synchronization: workload waits for streaming to be ready
    let streaming_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let streaming_ready_signal = streaming_ready.clone();

    let local = tokio::task::LocalSet::new();
    let stream_result = local.run_until(async {
        // Spawn stream controller task
        let controller_handle = tokio::task::spawn_local({
            async move {
                session::drive_stream_controller(
                    stream_controller,
                    SubscribeReq { flags: 0 },
                    |event| {
                        process_streamed_event(event);
                        true
                    },
                )
                .await
            }
        });

        // Spawn stream cluster task (will run until max_events or workload completes)
        let cluster_handle = tokio::task::spawn_local({
            let workload_done = workload_complete_clone;
            let streaming_ready = streaming_ready_signal;
            async move {
                struct ConcurrentControl {
                    max_events: u32,
                    seen: u32,
                    seen_after_done: u32,
                    workload_done: Arc<std::sync::atomic::AtomicBool>,
                }
                impl StreamControl for ConcurrentControl {
                    fn should_continue(&mut self) -> bool {
                        self.seen += 1;
                        // After workload completes, capture more events then stop
                        let done = self.workload_done.load(Ordering::Relaxed);
                        if done {
                            self.seen_after_done += 1;
                            // Wait for 50 more events after workload completes
                            if self.seen_after_done >= 50 {
                                return false;
                            }
                        }
                        self.seen < self.max_events
                    }
                }
                // Signal streaming is ready BEFORE entering the event loop
                // (WaitForNewUserEvents blocks until User ring has events)
                streaming_ready.store(true, Ordering::Release);
                session::drive_stream_cluster(
                    stream_cluster,
                    ConcurrentControl {
                        max_events: 2000,
                        seen: 0,
                        seen_after_done: 0,
                        workload_done: workload_done,
                    },
                )
                .await
            }
        });

        // Spawn workload task - runs Ping-Pong concurrently with streaming
        let workload_handle = tokio::task::spawn_local({
            let workload_done = workload_complete;
            let streaming_ready = streaming_ready;
            async move {
                // Wait for streaming to be ready before starting workload
                while !streaming_ready.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                println!("  [WORKLOAD] Streaming ready, starting Ping-Pong rounds...");

                for round in 1..=total_rounds {
                    let ping_pong_sid = SessionId::new(0x2000 + round);
                    let ping_pong_lane = Lane::new(1);

                    let client_endpoint = cluster
                        .attach_cursor::<0, _, _, _>(
                            rv_id,
                            ping_pong_sid,
                            &CLIENT_PROGRAM,
                            LaneBinding::new(ping_pong_lane),
                        )
                        .expect("attach ping-pong client");

                    let server_endpoint = cluster
                        .attach_cursor::<1, _, _, _>(
                            rv_id,
                            ping_pong_sid,
                            &SERVER_PROGRAM,
                            LaneBinding::new(ping_pong_lane),
                        )
                        .expect("attach ping-pong server");

                    let ping_value = round * 100;
                    let (client_res, server_res) = futures::join!(
                        async {
                            let (ep, _) = client_endpoint
                                .flow::<Ping>()
                                .expect("ping flow")
                                .send(&ping_value)
                                .await
                                .expect("send ping");
                            // Allow streaming to process Ping TAP_OUT
                            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                            let (ep, pong_value) = ep
                                .recv::<Pong>()
                                .await
                                .expect("recv pong");
                            (ep, pong_value)
                        },
                        async {
                            let (ep, ping_value) = server_endpoint
                                .recv::<Ping>()
                                .await
                                .expect("recv ping");
                            let pong_value = ping_value + 1;
                            let (ep, _) = ep
                                .flow::<Pong>()
                                .expect("pong flow")
                                .send(&pong_value)
                                .await
                                .expect("send pong");
                            // Allow streaming to process Pong TAP_OUT
                            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                            (ep, pong_value)
                        }
                    );

                    let (_, pong_received) = client_res;
                    let (_, pong_sent) = server_res;
                    println!("  [WORKLOAD] Round {}: Ping({}) → Pong({})", round, ping_value, pong_received);
                    assert_eq!(pong_received, pong_sent);

                    // Extra delay between rounds
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }

                println!("  [WORKLOAD] All rounds complete.");
                workload_done.store(true, Ordering::Relaxed);
                Ok::<_, ()>(())
            }
        });

        // Wait for workload to complete first
        let workload_res = workload_handle.await.expect("workload task");

        // Give streaming more time to drain remaining events from TapRing
        // The streaming loop processes one event per iteration with wire
        // round-trip overhead, so we need enough time for it to catch up
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Abort streaming tasks (they may be waiting for more events)
        controller_handle.abort();
        cluster_handle.abort();

        // Streaming tasks are aborted; we don't need their results
        let _ = controller_handle.await;
        let _ = cluster_handle.await;

        workload_res
    }).await;

    let _workload_res = stream_result;

    println!();
    println!("  Real-time streaming completed (aborted after workload)");

    // =========================================================================
    // Final Summary (EPF TAP_OUT events only)
    // =========================================================================
    println!();
    println!("================================================================");
    println!("                  FINAL SUMMARY (EPF TAP_OUT)");
    println!("================================================================");

    let tap_outs = TAP_OUT_COUNT.load(Ordering::Relaxed);
    let total_received = TOTAL_EVENTS_RECEIVED.load(Ordering::Relaxed);

    println!();
    println!("  Total events received: {}", total_received);
    println!("  EPF TAP_OUT:           {} (Ping/Pong filtered by EPF policy)", tap_outs);
    println!();
    println!("  Maximum: 10 rounds × 2 TAP_OUT = 20 events");
    println!("    (EndpointTx fires on each SEND: Client→Ping, Server→Pong)");
    println!();
    println!("  Note: Live streaming is best-effort. Wire protocol overhead");
    println!("  limits real-time throughput. Use ring polling for 100% capture.");
    println!();
    println!("================================================================");
}

fn main() {
    let handle = std::thread::Builder::new()
        .name("live-telemetry".into())
        .stack_size(128 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run_demo());
        })
        .expect("spawn thread");
    handle.join().expect("join thread");
}
