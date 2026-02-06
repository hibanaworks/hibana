//! Demonstrate cancellation and checkpoint/rollback orchestration with the cursor API.
//!
//! The example defines a pair of role-local programs derived from the global DSL,
//! attaches cursor endpoints, and drives the control-plane effects end to end.
//!
//! Key concept: CanonicalControl messages are self-send (Controller → Controller),
//! which means they use `flow().send(())` and the wire transmission is skipped.
#![allow(clippy::type_complexity)]

use hibana::{
    binding::NoBinding,
    control::cap::{
        GenericCapToken,
        resource_kinds::{CancelKind, CheckpointKind, RollbackKind},
    },
    endpoint::ControlOutcome,
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    observe::{self, CancelEventKind, PolicyEventKind, TapEvent},
    rendezvous::{Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{
            DefaultLabelUniverse, LABEL_CANCEL, LABEL_CHECKPOINT, LABEL_ROLLBACK, RING_EVENTS,
        },
    },
};

const SLAB_BYTES: usize = 4096;

type Cluster = SessionCluster<'static, InMemoryTransport, DefaultLabelUniverse, CounterClock, 4>;
type Controller = Role<0>;

type CancelMsg =
    Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, hibana::g::CanonicalControl<CancelKind>>;
type CheckpointMsg = Msg<
    { LABEL_CHECKPOINT },
    GenericCapToken<CheckpointKind>,
    hibana::g::CanonicalControl<CheckpointKind>,
>;
type RollbackMsg = Msg<
    { LABEL_ROLLBACK },
    GenericCapToken<RollbackKind>,
    hibana::g::CanonicalControl<RollbackKind>,
>;
// Self-send for CanonicalControl messages
type CancelSteps = StepCons<SendStep<Controller, Controller, CancelMsg>, StepNil>;
type CheckpointSteps = StepCons<
    SendStep<Controller, Controller, CheckpointMsg>,
    StepCons<SendStep<Controller, Controller, RollbackMsg>, StepNil>,
>;

const CANCEL_PROTOCOL: g::Program<CancelSteps> = g::send::<Controller, Controller, CancelMsg, 0>();
const CHECKPOINT_PROTOCOL: g::Program<CheckpointSteps> = g::seq(
    g::send::<Controller, Controller, CheckpointMsg, 0>(),
    g::send::<Controller, Controller, RollbackMsg, 0>(),
);

// Self-send projections: only Controller participates
type ControllerCancelLocal = <CancelSteps as ProjectRole<Controller>>::Output;
type ControllerCheckpointLocal = <CheckpointSteps as ProjectRole<Controller>>::Output;

static CONTROLLER_CANCEL_PROGRAM: g::RoleProgram<'static, 0, ControllerCancelLocal> =
    g::project::<0, CancelSteps, _>(&CANCEL_PROTOCOL);
static CONTROLLER_CHECKPOINT_PROGRAM: g::RoleProgram<'static, 0, ControllerCheckpointLocal> =
    g::project::<0, CheckpointSteps, _>(&CHECKPOINT_PROTOCOL);

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}

fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

async fn cancel_flow(
    cluster: &'static Cluster,
    rv_id: hibana::control::types::RendezvousId,
    sid: SessionId,
) {
    let rv = cluster
        .get_local(&rv_id)
        .expect("rendezvous reference for cancel");

    // Self-send: only Controller participates in CancelMsg (Canonical control)
    let controller = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_CANCEL_PROGRAM, NoBinding)
        .expect("controller attach");

    let tap_ring = rv.tap();
    let tap_cursor = tap_ring.head();

    // Self-send uses flow().send(()) - wire transmission is skipped for self-send
    let (controller, outcome) = controller
        .flow::<CancelMsg>()
        .expect("canonical cancel flow")
        .send(())
        .await
        .expect("cancel send");
    assert!(matches!(outcome, ControlOutcome::Canonical(_)));

    let summary = rv.association(sid);

    drop(controller);

    if let Some(snapshot) = summary {
        println!(
            "cancel counters: begin={} ack={}",
            snapshot.acks.cancel_begin,
            snapshot.acks.cancel_ack
        );
    } else {
        println!("cancel completed; association cleared");
    }

    let mut cancel_cursor = tap_cursor;
    let mut policy_cursor = tap_cursor;

    let mut begin = 0;
    let mut ack = 0;
    for event in tap_ring.cancel_events_since(&mut cancel_cursor) {
        match event.kind {
            CancelEventKind::Begin => begin += 1,
            CancelEventKind::Ack => ack += 1,
        }
    }

    let mut policy_abort = 0;
    let mut policy_trap = 0;
    for event in tap_ring.policy_events_since(&mut policy_cursor) {
        match event.kind {
            PolicyEventKind::Abort => policy_abort += 1,
            PolicyEventKind::Trap => policy_trap += 1,
            _ => {}
        }
    }

    println!(
        "tap events: cancel_begin={} cancel_ack={} (policy_abort={}, policy_trap={})",
        begin, ack, policy_abort, policy_trap
    );
}

async fn checkpoint_flow(
    cluster: &'static Cluster,
    rv_id: hibana::control::types::RendezvousId,
    sid: SessionId,
) {
    let rv = cluster
        .get_local(&rv_id)
        .expect("rendezvous reference for checkpoint");

    // Self-send: only Controller participates in CheckpointMsg/RollbackMsg (Canonical control)
    let controller = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_CHECKPOINT_PROGRAM, NoBinding)
        .expect("controller attach");

    let tap_ring = rv.tap();
    let tap_cursor = tap_ring.head();

    // Self-send uses flow().send(()) - wire transmission is skipped for self-send
    let (controller, checkpoint_outcome) = controller
        .flow::<CheckpointMsg>()
        .expect("canonical checkpoint flow")
        .send(())
        .await
        .expect("checkpoint send");
    assert!(matches!(checkpoint_outcome, ControlOutcome::Canonical(_)));

    let (controller, rollback_outcome) = controller
        .flow::<RollbackMsg>()
        .expect("canonical rollback flow")
        .send(())
        .await
        .expect("rollback send");
    assert!(matches!(rollback_outcome, ControlOutcome::Canonical(_)));

    let snapshot = rv.association(sid);

    drop(controller);
    if let Some(s) = snapshot {
        println!("checkpoint summary: last_checkpoint={:?}", s.last_checkpoint);
    } else {
        println!("checkpoint summary: association cleared");
    }

    let mut event_cursor = tap_cursor;
    let mut policy_cursor = tap_cursor;

    let mut checkpoint_events = 0;
    let mut rollback_events = 0;
    for event in tap_ring.events_since(&mut event_cursor, |e| Some(e)) {
        if event.id == observe::ids::CHECKPOINT_REQ {
            checkpoint_events += 1;
        } else if event.id == observe::ids::ROLLBACK_REQ {
            rollback_events += 1;
        }
    }

    let mut policy_effect = 0;
    let mut policy_effect_ok = 0;
    let mut policy_commit = 0;
    let mut policy_rollback = 0;
    for event in tap_ring.policy_events_since(&mut policy_cursor) {
        match event.kind {
            PolicyEventKind::Effect => policy_effect += 1,
            PolicyEventKind::EffectOk => policy_effect_ok += 1,
            PolicyEventKind::Commit => policy_commit += 1,
            PolicyEventKind::Rollback => policy_rollback += 1,
            _ => {}
        }
    }

    println!(
        "tap events: checkpoint={} rollback={} (policy_effect={}, policy_ok={}, policy_commit={}, policy_rollback={})",
        checkpoint_events,
        rollback_events,
        policy_effect,
        policy_effect_ok,
        policy_commit,
        policy_rollback
    );
}

async fn run_demo() {
    let tap = leak_tap_storage();
    let slab = leak_slab(SLAB_BYTES);
    let config = Config::new(tap, slab);
    let transport = InMemoryTransport::default();
    let rendezvous = Rendezvous::from_config(config, transport.clone());

    let cluster_handle: &'static mut Cluster =
        Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster_handle
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let cluster: &'static Cluster = cluster_handle;

    // Each protocol uses a separate session ID since they are independent protocols
    let cancel_sid = SessionId::new(1);
    let checkpoint_sid = SessionId::new(2);

    println!("=== Cancel Flow Demo ===");
    cancel_flow(cluster, rv_id, cancel_sid).await;

    println!("\n=== Checkpoint/Rollback Flow Demo ===");
    checkpoint_flow(cluster, rv_id, checkpoint_sid).await;
}

fn main() {
    std::thread::Builder::new()
        .name("hibana-transactional".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_demo());
        })
        .expect("spawn main thread")
        .join()
        .expect("main thread panicked");
}

// ---------------------------------------------------------------------------
// Minimal in-memory transport used by the example (copied from tests/common).
// ---------------------------------------------------------------------------

use hibana::transport::{Transport, TransportError, wire::Payload};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct InMemoryTransport {
    state: Arc<Mutex<VecDeque<(u8, Vec<u8>)>>>,
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

#[derive(Debug)]
enum InMemoryTransportError {
    Empty,
}

impl From<InMemoryTransportError> for TransportError {
    fn from(_: InMemoryTransportError) -> Self {
        TransportError::Failed
    }
}

impl Transport for InMemoryTransport {
    type Error = InMemoryTransportError;
    type Tx<'a>
        = InMemoryTx
    where
        Self: 'a;
    type Rx<'a>
        = InMemoryRx
    where
        Self: 'a;
    type Send<'a>
        = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>
    where
        Self: 'a;
    type Recv<'a>
        = Pin<Box<dyn Future<Output = Result<Payload<'a>, Self::Error>> + 'a>>
    where
        Self: 'a;
    type Metrics = hibana::transport::NoopMetrics;

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            InMemoryTx {
                state: self.state.clone(),
            },
            InMemoryRx {
                state: self.state.clone(),
                local_role,
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
        let bytes = payload.as_bytes().to_vec();
        let state = tx.state.clone();
        let dest = dest_role;
        Box::pin(async move {
            state
                .lock()
                .expect("transport state")
                .push_back((dest, bytes));
            Ok(())
        })
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        let state = rx.state.clone();
        let local = rx.local_role;
        Box::pin(async move {
            let mut guard = state.lock().expect("transport state");
            let pos = guard.iter().position(|(role, _)| *role == local);
            let (_, payload) = pos
                .and_then(|idx| guard.remove(idx))
                .ok_or(InMemoryTransportError::Empty)?;
            let bytes = Box::leak(payload.into_boxed_slice());
            Ok(Payload::new(bytes))
        })
    }
}

struct InMemoryTx {
    state: Arc<Mutex<VecDeque<(u8, Vec<u8>)>>>,
}

struct InMemoryRx {
    state: Arc<Mutex<VecDeque<(u8, Vec<u8>)>>>,
    local_role: u8,
}
