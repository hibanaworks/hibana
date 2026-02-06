//! EPF Control Policy — Dynamic Policy Hot-Reload via Management Session
//!
//! This example demonstrates EPF's **control capability** with **hot-reload**:
//! blocking operations based on policy, then reverting to allow them.
//!
//! # EPF Use Cases (Two Examples)
//!
//! | Example | EPF Action | Use Case |
//! |---------|------------|----------|
//! | `mgmt_epf_observe.rs` | TAP_OUT | Observation — emit events to User Ring |
//! | `mgmt_epf_control.rs` | ACT_ABORT + Revert | Control + Hot-Reload |
//!
//! # Scenario
//!
//! 1. **Phase 1**: Install blocking policy via Management Session
//!    - Pong sends are blocked with `ACT_ABORT`
//!
//! 2. **Phase 2**: Run first 3 rounds of Ping-Pong
//!    - Ping succeeds, Pong is **BLOCKED**
//!
//! 3. **Phase 3**: Revert policy via Management Session (hot-reload)
//!    - Policy is rolled back to previous version (no policy)
//!
//! 4. **Phase 4**: Run remaining 2 rounds of Ping-Pong
//!    - Both Ping and Pong **SUCCEED**
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │ Timeline                                                                │
//! │                                                                        │
//! │  ──[Install Policy]──▶ Round 1-3 ──[Revert]──▶ Round 4-5              │
//! │                        (blocked)               (allowed)               │
//! │                                                                        │
//! │  EPF Slot:  v0 (empty) → v1 (block) → v0 (empty)                      │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Run with:
//! ```bash
//! cargo run --example mgmt_epf_control --features std
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
            Command, LOAD_CHUNK_MAX, LoadBegin, LoadChunk, Reply,
            session::{self, ControllerPlan},
        },
    },
    transport::{Transport, TransportError, TransportMetrics, TransportSnapshot, wire::Payload},
};

// =============================================================================
// Telemetry State
// =============================================================================

static TELEMETRY_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Abort reason code embedded in the EPF policy.
const ABORT_REASON_PONG_BLOCKED: u16 = 0x4321;

/// POLICY_ABORT event ID (from observe::ids) - goes to Infra Ring.
const POLICY_ABORT_ID: u16 = 0x0400;

/// POLICY_ROLLBACK event ID - emitted when Revert completes.
const POLICY_ROLLBACK_ID: u16 = 0x0406;

/// Format elapsed time as MM:SS.mmm
fn format_elapsed(start: Instant) -> String {
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = elapsed.subsec_millis();
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}

// =============================================================================
// Protocol Definition (Ping-Pong)
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
    collections::{HashMap, HashSet, VecDeque},
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

type QueueKey = (u32, u8);

#[derive(Default)]
struct IsolatedState {
    queues: HashMap<QueueKey, VecDeque<FrameOwned>>,
    waiters: HashMap<QueueKey, Vec<Waker>>,
    binding_owned: HashSet<QueueKey>,
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

    pub fn set_pending_label(&self, session_id: u32, dest_role: u8, label: u8) {
        let key = (session_id, dest_role);
        let mut state = self.state.lock().expect("state lock");
        state.pending_labels.insert(key, label);
    }

    fn take_pending_label(&self, session_id: u32, dest_role: u8) -> Option<u8> {
        let key = (session_id, dest_role);
        let mut state = self.state.lock().expect("state lock");
        state.pending_labels.remove(&key)
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

        let is_binding_owned = state.binding_owned.contains(&key);

        if let Some(queue) = state.queues.get_mut(&key) {
            if !queue.is_empty() {
                if is_binding_owned {
                    drop(state);
                    return Poll::Ready(Ok(Payload::new(&[])));
                } else {
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
        let pending_label = self.take_pending_label(tx.session_id, dest_role);
        let payload_vec = if let Some(label) = pending_label {
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
// EPF Control Policy — Abort on Pong sends
// =============================================================================

/// Labels for Ping and Pong messages
#[allow(dead_code)]
const LABEL_PING: u8 = 1;
const LABEL_PONG: u8 = 2;

/// EPF policy: Block Pong sends with ACT_ABORT
///
/// arg1 layout: | role:8 | lane:8 | label:8 | flags:8 |
/// label is at bits 8-15, extracted via: (arg1 >> 8) & 0xFF
#[rustfmt::skip]
const CONTROL_POLICY: &[u8] = &[
    // offset 0: GET_EVENT_ARG1 r0 (2 bytes) -> r0 = arg1
    ops::instr::GET_EVENT_ARG1, 0x00,

    // offset 2: SHR r1, r0, 8 (4 bytes) -> r1 = arg1 >> 8
    ops::instr::SHR, 0x01, 0x00, 8,

    // offset 6: AND_IMM r1, r1, 0xFF (4 bytes) -> r1 = label
    ops::instr::AND_IMM, 0x01, 0x01, 0xFF,

    // offset 10: JUMP_EQ_IMM r1, LABEL_PONG, 16 (5 bytes) -> if label == Pong, goto block
    ops::instr::JUMP_EQ_IMM, 0x01, LABEL_PONG, 16, 0x00,

    // offset 15: HALT (1 byte) -> allow (not Pong)
    ops::instr::HALT,

    // block (offset 16):
    ops::instr::ACT_ABORT,
    (ABORT_REASON_PONG_BLOCKED & 0xFF) as u8,
    ((ABORT_REASON_PONG_BLOCKED >> 8) & 0xFF) as u8,
];

/// Empty policy (HALT immediately = Proceed)
#[rustfmt::skip]
const EMPTY_POLICY: &[u8] = &[
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
        fuel_max: 10000,
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

fn build_controller_plan(sid: SessionId, lane: Lane, slot: Slot, code: &'static [u8], command: Command) -> ControllerPlan<'static> {
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
        command,
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
// Run a single Ping-Pong round
// =============================================================================

async fn run_ping_pong_round(
    cluster: &'static Cluster,
    rv_id: RendezvousId,
    round: u32,
    pong_blocked_count: &Arc<AtomicU32>,
    pong_success_count: &Arc<AtomicU32>,
) {
    let ping_pong_sid = SessionId::new(0x2000 + round);

    let client_endpoint = cluster
        .attach_cursor::<0, _, _, _>(
            rv_id,
            ping_pong_sid,
            &CLIENT_PROGRAM,
            NoBinding,
        )
        .expect("attach ping-pong client");

    let server_endpoint = cluster
        .attach_cursor::<1, _, _, _>(
            rv_id,
            ping_pong_sid,
            &SERVER_PROGRAM,
            NoBinding,
        )
        .expect("attach ping-pong server");

    let ping_value = round * 100;
    let pong_blocked = pong_blocked_count.clone();
    let pong_success = pong_success_count.clone();

    let start = TELEMETRY_START.get_or_init(Instant::now);

    futures::join!(
        async {
            let send_result = client_endpoint
                .flow::<Ping>()
                .expect("ping flow")
                .send(&ping_value)
                .await;

            match send_result {
                Ok((ep, _)) => {
                    let elapsed = format_elapsed(*start);
                    println!("  [{elapsed}] [CLIENT] Round {}: Ping({}) sent ✓", round, ping_value);
                    let recv_future = ep.recv::<Pong>();
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        recv_future,
                    ).await {
                        Ok(Ok((_ep, pong_value))) => {
                            let elapsed = format_elapsed(*start);
                            println!("  [{elapsed}] [CLIENT] Round {}: Pong({}) received ✓", round, pong_value);
                        }
                        Ok(Err(e)) => {
                            let elapsed = format_elapsed(*start);
                            println!("  [{elapsed}] [CLIENT] Round {}: recv error: {:?}", round, e);
                        }
                        Err(_) => {
                            let elapsed = format_elapsed(*start);
                            println!("  [{elapsed}] [CLIENT] Round {}: Pong timeout (blocked)", round);
                        }
                    }
                }
                Err(e) => {
                    let elapsed = format_elapsed(*start);
                    println!("  [{elapsed}] [CLIENT] Round {}: Ping send error: {:?}", round, e);
                }
            }
        },
        async {
            let recv_result = server_endpoint.recv::<Ping>().await;
            match recv_result {
                Ok((ep, ping_value)) => {
                    let elapsed = format_elapsed(*start);
                    println!("  [{elapsed}] [SERVER] Round {}: Ping({}) received", round, ping_value);
                    let pong_value = ping_value + 1;

                    let send_result = ep
                        .flow::<Pong>()
                        .expect("pong flow")
                        .send(&pong_value)
                        .await;

                    match send_result {
                        Ok(_) => {
                            pong_success.fetch_add(1, Ordering::Relaxed);
                            let elapsed = format_elapsed(*start);
                            println!("  [{elapsed}] [SERVER] Round {}: Pong({}) sent ✓", round, pong_value);
                        }
                        Err(e) => {
                            pong_blocked.fetch_add(1, Ordering::Relaxed);
                            let elapsed = format_elapsed(*start);
                            println!("  [{elapsed}] [SERVER] Round {}: Pong BLOCKED ✗ ({:?})", round, e);
                        }
                    }
                }
                Err(e) => {
                    let elapsed = format_elapsed(*start);
                    println!("  [{elapsed}] [SERVER] Round {}: Ping recv error: {:?}", round, e);
                }
            }
        }
    );
}

// =============================================================================
// Execute a Management Session command
// =============================================================================

async fn execute_mgmt_command(
    cluster: &'static Cluster,
    rv_id: RendezvousId,
    mgmt_sid: SessionId,
    mgmt_lane: Lane,
    slot: Slot,
    code: &'static [u8],
    command: Command,
) -> Reply {
    let controller_plan = build_controller_plan(mgmt_sid, mgmt_lane, slot, code, command);
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

    cluster.drive_mgmt(rv_id, mgmt_sid, seed).expect("drive mgmt")
}

// =============================================================================
// Main Demo
// =============================================================================

async fn run_demo() {
    println!();
    println!("==================================================================");
    println!("     EPF Control Policy Demo — Hot-Reload                        ");
    println!("     Block Pong → Revert → Allow Pong                            ");
    println!("==================================================================");
    println!();

    TELEMETRY_START.get_or_init(Instant::now);

    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

    let transport = IsolatedTransport::new();
    let config = Config::new(leak_tap_storage(), leak_slab(4096));
    let rendezvous: Rendezvous<'_, '_, IsolatedTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport.clone());
    let rv_id = cluster.add_rendezvous(rendezvous).expect("add rendezvous");

    let tap = cluster.get_local(&rv_id).expect("get rendezvous").tap();
    let tap_static = unsafe { tap.assume_static() };
    let _previous_ring = observe::install_ring(tap_static);

    register_mgmt_loop_resolvers(cluster, rv_id);

    let pong_blocked_count = Arc::new(AtomicU32::new(0));
    let pong_success_count = Arc::new(AtomicU32::new(0));

    // =========================================================================
    // Phase 1a: Install empty policy (v1) — baseline for Revert
    // =========================================================================
    println!("Phase 1a: Installing empty EPF policy (baseline)");
    println!("------------------------------------------------------------");
    {
        let reply = execute_mgmt_command(
            cluster,
            rv_id,
            SessionId::new(0x1000),
            Lane::new(0),
            Slot::EndpointTx,
            EMPTY_POLICY,
            Command::Activate { slot: Slot::EndpointTx },
        ).await;

        match reply {
            Reply::Activated(report) => {
                println!("  ✓ Empty policy activated: version={}", report.version);
            }
            other => panic!("expected Activated, got {:?}", other),
        }
    }
    println!();

    // =========================================================================
    // Phase 1b: Install blocking policy (v2)
    // =========================================================================
    println!("Phase 1b: Installing blocking EPF policy");
    println!("------------------------------------------------------------");
    {
        let reply = execute_mgmt_command(
            cluster,
            rv_id,
            SessionId::new(0x1001),
            Lane::new(0),
            Slot::EndpointTx,
            CONTROL_POLICY,
            Command::Activate { slot: Slot::EndpointTx },
        ).await;

        match reply {
            Reply::Activated(report) => {
                println!("  ✓ Blocking policy activated: version={}", report.version);
            }
            other => panic!("expected Activated, got {:?}", other),
        }
    }
    println!();

    // =========================================================================
    // Phase 2: Run first 3 rounds (Pong blocked)
    // =========================================================================
    println!("Phase 2: Running rounds 1-3 (Pong BLOCKED)");
    println!("------------------------------------------------------------");
    for round in 1..=3 {
        run_ping_pong_round(cluster, rv_id, round, &pong_blocked_count, &pong_success_count).await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        println!();
    }

    // =========================================================================
    // Phase 3: Revert policy (hot-reload back to v1)
    // =========================================================================
    println!("Phase 3: Reverting EPF policy (hot-reload back to v1)");
    println!("------------------------------------------------------------");
    {
        // Revert to previous version (v1 = empty policy)
        let reply = execute_mgmt_command(
            cluster,
            rv_id,
            SessionId::new(0x1002),
            Lane::new(0),
            Slot::EndpointTx,
            EMPTY_POLICY, // Not actually used by Revert, but required by helper
            Command::Revert { slot: Slot::EndpointTx },
        ).await;

        match reply {
            Reply::Reverted(report) => {
                println!("  ✓ Policy reverted: version={}", report.version);
            }
            other => println!("  Revert result: {:?}", other),
        }
    }
    println!();

    // =========================================================================
    // Phase 4: Run remaining 2 rounds (Pong allowed)
    // =========================================================================
    println!("Phase 4: Running rounds 4-5 (Pong ALLOWED)");
    println!("------------------------------------------------------------");
    for round in 4..=5 {
        run_ping_pong_round(cluster, rv_id, round, &pong_blocked_count, &pong_success_count).await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        println!();
    }

    // =========================================================================
    // Phase 5: Poll Infra Ring for events
    // =========================================================================
    println!("Phase 5: Polling Infra Ring");
    println!("------------------------------------------------------------");

    let mut policy_abort_count = 0u32;
    let mut policy_rollback_count = 0u32;
    let mut cursor = 0usize;
    observe::for_each_since(&mut cursor, |event| {
        if event.id == POLICY_ABORT_ID {
            policy_abort_count += 1;
            println!("  [INFRA] POLICY_ABORT  reason=0x{:04X} sid=0x{:04X}",
                event.arg0 as u16, event.arg1);
        } else if event.id == POLICY_ROLLBACK_ID {
            policy_rollback_count += 1;
            println!("  [INFRA] POLICY_ROLLBACK  slot={} version={}",
                event.arg0, event.arg1);
        }
    });
    println!();

    // =========================================================================
    // Final Summary
    // =========================================================================
    println!("================================================================");
    println!("                  FINAL SUMMARY");
    println!("================================================================");

    let blocked = pong_blocked_count.load(Ordering::Relaxed);
    let success = pong_success_count.load(Ordering::Relaxed);

    println!();
    println!("  Rounds 1-3 (blocking policy active):");
    println!("    Pong blocked:  {} (expected: 3)", blocked);
    println!();
    println!("  Rounds 4-5 (after Revert):");
    println!("    Pong success:  {} (expected: 2)", success);
    println!();
    println!("  Infra Ring events:");
    println!("    POLICY_ABORT:    {}", policy_abort_count);
    println!("    POLICY_ROLLBACK: {}", policy_rollback_count);
    println!();

    if blocked == 3 && success == 2 {
        println!("  ✓ EPF hot-reload working correctly!");
        println!("    - Phase 1a: Empty policy (v1) installed as baseline");
        println!("    - Phase 1b: Blocking policy (v2) activated");
        println!("    - Phase 2: Blocking policy blocked 3 Pong sends");
        println!("    - Phase 3: Revert rolled back to v1 (empty policy)");
        println!("    - Phase 4: Remaining 2 Pong sends succeeded");
    } else {
        println!("  ⚠ Results differ from expected:");
        println!("    blocked={} (expected 3), success={} (expected 2)", blocked, success);
    }
    println!();
    println!("================================================================");
}

fn main() {
    let handle = std::thread::Builder::new()
        .name("epf-control".into())
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
