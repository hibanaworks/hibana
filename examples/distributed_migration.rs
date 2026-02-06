//! Distributed session migration (splice) example.
//!
//! Demonstrates migrating a session from one rendezvous to another using
//! hibana's splice protocol. This is a key feature for load balancing and
//! session mobility in distributed systems.
//!
//! The protocol:
//! 1. Handshake: Controller → Worker (establish session)
//! 2. SpliceIntent: Worker → Controller (request migration)
//! 3. SpliceAck: Controller → Worker (confirm migration)
//! 4. Session moves to destination rendezvous
//! 5. Resume: Controller → Worker (continue on new rendezvous)
//!
//! Run with:
//! ```bash
//! cargo run --example distributed_migration --features std
//! ```

#![cfg(feature = "std")]

use hibana::{
    binding::NoBinding,
    control::{
        cap::{
            EpochInit, GenericCapToken, ResourceKind,
            resource_kinds::{SpliceAckKind, SpliceIntentKind},
        },
        cluster::{AttachError, DynamicResolution, ResolverContext},
        types::{LaneId, RendezvousId},
    },
    endpoint::{ControlOutcome, CursorEndpoint, RecvError, SendError},
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    global::const_dsl::{DynamicMeta, HandlePlan},
    observe::{AssociationSnapshot, PolicyEventKind},
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{
        SessionCluster,
        config::Config,
        consts::{DefaultLabelUniverse, LABEL_SPLICE_ACK, LABEL_SPLICE_INTENT},
    },
};
use std::{
    error::Error,
    fmt,
    sync::{Mutex, OnceLock},
};
use tokio::runtime;
use tokio::task::yield_now;

use transport::{TestTransport, TestTransportError, leak_clock};

// Role aliases
type Controller = Role<0>;
type Worker = Role<1>;

// Message aliases
type Handshake = Msg<10, u32>;
type DelegateMsg = Msg<
    { LABEL_SPLICE_INTENT },
    GenericCapToken<SpliceIntentKind>,
    hibana::g::ExternalControl<SpliceIntentKind>,
>;
type AcceptMsg = Msg<
    { LABEL_SPLICE_ACK },
    GenericCapToken<SpliceAckKind>,
    hibana::g::ExternalControl<SpliceAckKind>,
>;
type ResumeMsg = Msg<42, u64>;

const SPLICE_POLICY_ID: u16 = 13;
const SPLICE_META: DynamicMeta = DynamicMeta::new();
const QUEUE_ALERT_DEPTH: u32 = 64;
const LATENCY_ALERT_US: u64 = 250_000;

static SPLICE_TARGET: OnceLock<Mutex<Option<(RendezvousId, LaneId)>>> = OnceLock::new();

fn transport_congested(snapshot: &hibana::transport::TransportSnapshot) -> bool {
    snapshot
        .queue_depth
        .map(|depth| depth >= QUEUE_ALERT_DEPTH)
        .unwrap_or(false)
        || snapshot
            .latency_us
            .map(|lat| lat >= LATENCY_ALERT_US)
            .unwrap_or(false)
}

fn example_splice_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    if ctx.tag != SpliceIntentKind::TAG && ctx.tag != SpliceAckKind::TAG {
        return Err(());
    }
    if ctx.tag == SpliceIntentKind::TAG && transport_congested(&ctx.metrics) {
        return Err(());
    }
    let guard = SPLICE_TARGET
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("splice target mutex poisoned");
    let (dst_rv, dst_lane) = guard.as_ref().ok_or(())?;
    Ok(DynamicResolution::Splice {
        dst_rv: *dst_rv,
        dst_lane: *dst_lane,
        fences: None,
    })
}

// Protocol definition
// Handshake → SpliceIntent → SpliceAck
type HandshakeSteps = StepCons<SendStep<Controller, Worker, Handshake>, StepNil>;
type DelegateSteps = StepCons<SendStep<Worker, Controller, DelegateMsg>, StepNil>;
type AcceptSteps = StepCons<SendStep<Controller, Worker, AcceptMsg>, StepNil>;
type ProtocolSteps = StepCons<
    SendStep<Controller, Worker, Handshake>,
    StepCons<
        SendStep<Worker, Controller, DelegateMsg>,
        StepCons<SendStep<Controller, Worker, AcceptMsg>, StepNil>,
    >,
>;

// Post-splice protocol
type ResumeSteps = StepCons<SendStep<Controller, Worker, ResumeMsg>, StepNil>;

const HANDSHAKE: g::Program<HandshakeSteps> = g::send::<Controller, Worker, Handshake, 0>();
const DELEGATE: g::Program<DelegateSteps> = g::with_control_plan(
    g::send::<Worker, Controller, DelegateMsg, 0>(),
    HandlePlan::dynamic(SPLICE_POLICY_ID, SPLICE_META),
);
const ACCEPT: g::Program<AcceptSteps> = g::with_control_plan(
    g::send::<Controller, Worker, AcceptMsg, 0>(),
    HandlePlan::dynamic(SPLICE_POLICY_ID, SPLICE_META),
);

const WORKFLOW: g::Program<ProtocolSteps> = g::seq(HANDSHAKE, g::seq(DELEGATE, ACCEPT));

const RESUME_PROTOCOL: g::Program<ResumeSteps> = g::send::<Controller, Worker, ResumeMsg, 0>();

type ControllerLocal = <ProtocolSteps as ProjectRole<Controller>>::Output;
type WorkerLocal = <ProtocolSteps as ProjectRole<Worker>>::Output;
type ControllerResumeLocal = <ResumeSteps as ProjectRole<Controller>>::Output;
type WorkerResumeLocal = <ResumeSteps as ProjectRole<Worker>>::Output;

static CONTROLLER_PROGRAM: g::RoleProgram<'static, 0, ControllerLocal> =
    g::project::<0, ProtocolSteps, _>(&WORKFLOW);
static WORKER_PROGRAM: g::RoleProgram<'static, 1, WorkerLocal> =
    g::project::<1, ProtocolSteps, _>(&WORKFLOW);
static CONTROLLER_RESUME_PROGRAM: g::RoleProgram<'static, 0, ControllerResumeLocal> =
    g::project::<0, ResumeSteps, _>(&RESUME_PROTOCOL);
static WORKER_RESUME_PROGRAM: g::RoleProgram<'static, 1, WorkerResumeLocal> =
    g::project::<1, ResumeSteps, _>(&RESUME_PROTOCOL);

// Endpoint aliases
type ControllerEndpoint =
    CursorEndpoint<'static, 0, TestTransport, DefaultLabelUniverse, transport::Clock, EpochInit, 4>;
type WorkerEndpoint =
    CursorEndpoint<'static, 1, TestTransport, DefaultLabelUniverse, transport::Clock, EpochInit, 4>;

type ExampleResult<T> = Result<T, DemoError>;

fn main() -> ExampleResult<()> {
    run_with_large_stack(|| {
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| DemoError::Protocol(format!("rt build failed: {err}")))?;
        rt.block_on(run_demo())
    })
}

fn run_with_large_stack<F>(f: F) -> ExampleResult<()>
where
    F: FnOnce() -> ExampleResult<()> + Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("hibana-distributed-demo".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || f())?;
    match handle.join() {
        Ok(res) => res,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn run_demo() -> ExampleResult<()> {
    let mut world = DistributedWorld::new()?;

    println!(
        "initial primary assoc:  {}",
        describe(world.primary_assoc())
    );
    println!(
        "initial secondary assoc: {}",
        describe(world.secondary_assoc())
    );

    let endpoints = run_pre_splice(&world).await?;
    println!(
        "after control tokens primary assoc:  {}",
        describe(world.primary_assoc())
    );

    drop(endpoints);

    yield_now().await;
    println!(
        "after distributed splice primary assoc:  {}",
        describe(world.primary_assoc())
    );
    println!(
        "after distributed splice secondary assoc: {}",
        describe(world.secondary_assoc())
    );

    run_post_splice(&world).await?;

    world.assert_quiescent();
    world.policy_summary();
    Ok(())
}

fn describe(snapshot: Option<AssociationSnapshot>) -> String {
    match snapshot {
        Some(assoc) => format!(
            "sid={} lane={} gen={}",
            assoc.sid.raw(),
            assoc.lane.raw(),
            assoc
                .last_generation
                .map(|g| g.raw().to_string())
                .unwrap_or_else(|| "-".into())
        ),
        None => "-".to_string(),
    }
}

// World setup & helpers
struct DistributedWorld {
    cluster: &'static Cluster,
    src_transport: TestTransport,
    dst_transport: TestTransport,
    src_rv: RendezvousId,
    dst_rv: RendezvousId,
    sid_rv: RendezvousSessionId,
    policy_cursor_src: usize,
    policy_cursor_dst: usize,
}

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, transport::Clock, 4>;

impl DistributedWorld {
    fn new() -> ExampleResult<Self> {
        let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let src_transport = TestTransport::default();
        let dst_transport = TestTransport::default();

        let rendezvous_primary: Rendezvous<
            '_,
            '_,
            TestTransport,
            DefaultLabelUniverse,
            transport::Clock,
        > = Rendezvous::from_config(
            Config::new(transport::leak_tap_storage(), transport::leak_slab(4096)),
            src_transport.clone(),
        );
        let rendezvous_secondary: Rendezvous<
            '_,
            '_,
            TestTransport,
            DefaultLabelUniverse,
            transport::Clock,
        > = Rendezvous::from_config(
            Config::new(transport::leak_tap_storage(), transport::leak_slab(4096)),
            dst_transport.clone(),
        );

        let src_rv = cluster.add_rendezvous(rendezvous_primary)?;
        let dst_rv = cluster.add_rendezvous(rendezvous_secondary)?;

        let sid_rv = RendezvousSessionId::new(7);
        let dst_lane_rv = Lane::new(1);

        // Register splice resolver for intent
        let intent_plan = WORKER_PROGRAM
            .control_plans()
            .find(|info| info.label == LABEL_SPLICE_INTENT)
            .expect("splice intent control plan");
        SPLICE_TARGET
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("splice target mutex poisoned")
            .replace((dst_rv, LaneId::new(dst_lane_rv.raw())));
        if intent_plan.plan.is_dynamic() {
            cluster.register_control_plan_resolver(
                src_rv,
                &intent_plan,
                example_splice_resolver,
            )?;
        }

        // Register splice resolver for ack
        let accept_plan = CONTROLLER_PROGRAM
            .control_plans()
            .find(|info| info.label == LABEL_SPLICE_ACK)
            .expect("splice ack control plan");
        if accept_plan.plan.is_dynamic() {
            cluster.register_control_plan_resolver(
                src_rv,
                &accept_plan,
                example_splice_resolver,
            )?;
        }

        Ok(Self {
            cluster,
            src_transport,
            dst_transport,
            src_rv,
            dst_rv,
            sid_rv,
            policy_cursor_src: 0,
            policy_cursor_dst: 0,
        })
    }

    fn attach_pre_splice(&self) -> ExampleResult<DemoEndpoints> {
        let controller = self.cluster.attach_cursor::<0, _, _, _>(
            self.src_rv,
            self.sid_rv,
            &CONTROLLER_PROGRAM,
            NoBinding,
        )?;
        let worker = self.cluster.attach_cursor::<1, _, _, _>(
            self.src_rv,
            self.sid_rv,
            &WORKER_PROGRAM,
            NoBinding,
        )?;
        Ok(DemoEndpoints { controller, worker })
    }

    fn attach_post_splice(&self) -> ExampleResult<DemoEndpoints> {
        let controller = self.cluster.attach_cursor::<0, _, _, _>(
            self.dst_rv,
            self.sid_rv,
            &CONTROLLER_RESUME_PROGRAM,
            NoBinding,
        )?;
        let worker = self.cluster.attach_cursor::<1, _, _, _>(
            self.dst_rv,
            self.sid_rv,
            &WORKER_RESUME_PROGRAM,
            NoBinding,
        )?;
        Ok(DemoEndpoints { controller, worker })
    }

    fn primary_assoc(&self) -> Option<AssociationSnapshot> {
        self.cluster
            .get_local(&self.src_rv)
            .and_then(|rv| rv.association(self.sid_rv))
    }

    fn secondary_assoc(&self) -> Option<AssociationSnapshot> {
        self.cluster
            .get_local(&self.dst_rv)
            .and_then(|rv| rv.association(self.sid_rv))
    }

    fn assert_quiescent(&self) {
        assert!(
            self.src_transport.is_empty(),
            "source transport still has in-flight frames"
        );
        assert!(
            self.dst_transport.is_empty(),
            "destination transport still has in-flight frames"
        );
    }

    fn policy_summary(&mut self) {
        let mut abort = 0;
        let mut trap = 0;
        let mut effect = 0;
        let mut effect_ok = 0;
        let mut commit = 0;
        let mut rollback = 0;
        let mut last_commit = None;
        let mut last_rollback = None;

        if let Some(rv) = self.cluster.get_local(&self.src_rv) {
            for event in rv.tap().policy_events_since(&mut self.policy_cursor_src) {
                match event.kind {
                    PolicyEventKind::Abort => abort += 1,
                    PolicyEventKind::Trap => trap += 1,
                    PolicyEventKind::Effect => effect += 1,
                    PolicyEventKind::EffectOk => effect_ok += 1,
                    PolicyEventKind::Commit => {
                        commit += 1;
                        last_commit = Some(event.arg1);
                    }
                    PolicyEventKind::Rollback => {
                        rollback += 1;
                        last_rollback = Some(event.arg1);
                    }
                    PolicyEventKind::Annotate => {}
                }
            }
        }

        if let Some(rv) = self.cluster.get_local(&self.dst_rv) {
            for event in rv.tap().policy_events_since(&mut self.policy_cursor_dst) {
                match event.kind {
                    PolicyEventKind::Abort => abort += 1,
                    PolicyEventKind::Trap => trap += 1,
                    PolicyEventKind::Effect => effect += 1,
                    PolicyEventKind::EffectOk => effect_ok += 1,
                    PolicyEventKind::Commit => {
                        commit += 1;
                        last_commit = Some(event.arg1);
                    }
                    PolicyEventKind::Rollback => {
                        rollback += 1;
                        last_rollback = Some(event.arg1);
                    }
                    PolicyEventKind::Annotate => {}
                }
            }
        }

        println!(
            "policy summary: abort={abort} trap={trap} effect={effect} ok={effect_ok} commit={commit} last_commit={last_commit:?} rollback={rollback} last_rollback={last_rollback:?}",
        );
    }
}

struct DemoEndpoints {
    controller: ControllerEndpoint,
    worker: WorkerEndpoint,
}

// Demo execution
async fn run_pre_splice(world: &DistributedWorld) -> ExampleResult<DemoEndpoints> {
    let DemoEndpoints {
        mut controller,
        mut worker,
    } = world.attach_pre_splice()?;

    // Step 1: Handshake
    let (controller_next, handshake_outcome) =
        controller.flow::<Handshake>()?.send(&0xCAFE_BABE).await?;
    assert!(matches!(handshake_outcome, ControlOutcome::None));
    controller = controller_next;
    let (next_worker, payload) = worker.recv::<Handshake>().await?;
    worker = next_worker;
    println!("handshake complete: payload={payload:#X}");

    // Step 2: Worker sends SpliceIntent to Controller (External with AUTO_MINT)
    let (worker_next, delegate_outcome) = worker.flow::<DelegateMsg>()?.send(()).await?;
    let delegate_token_local = match delegate_outcome {
        ControlOutcome::External(token) => token,
        _other => return Err(DemoError::Send(SendError::PhaseInvariant)),
    };
    worker = worker_next;
    let (next_controller, delegate_token_rx) = controller.recv::<DelegateMsg>().await?;
    controller = next_controller;
    if let Ok(handle) = delegate_token_local.decode_handle() {
        println!(
            "splice intent: src_rv={} src_lane={} -> dst_rv={} dst_lane={}",
            handle.src_rv, handle.src_lane, handle.dst_rv, handle.dst_lane
        );
    }
    assert_eq!(delegate_token_local.bytes, delegate_token_rx.bytes);

    // Step 3: Controller sends SpliceAck to Worker (External with AUTO_MINT)
    let (controller_next, accept_outcome) = controller.flow::<AcceptMsg>()?.send(()).await?;
    let ack_token_local = match accept_outcome {
        ControlOutcome::External(token) => token,
        _other => return Err(DemoError::Send(SendError::PhaseInvariant)),
    };
    controller = controller_next;
    let (next_worker, ack_token) = worker.recv::<AcceptMsg>().await?;
    worker = next_worker;
    println!("splice ack: caps_mask={}", ack_token.caps_mask().bits());
    assert_eq!(ack_token_local.bytes, ack_token.bytes);

    #[cfg(feature = "test-utils")]
    controller.phase_cursor().assert_terminal();
    #[cfg(feature = "test-utils")]
    worker.phase_cursor().assert_terminal();
    println!("pre-splice protocol complete");

    Ok(DemoEndpoints { controller, worker })
}

async fn run_post_splice(world: &DistributedWorld) -> ExampleResult<()> {
    let DemoEndpoints {
        controller,
        worker,
    } = world.attach_post_splice()?;

    // Resume on destination rendezvous
    let (controller_next, resume_outcome) = controller
        .flow::<ResumeMsg>()?
        .send(&0x1234_5678_9ABC_DEF0)
        .await?;
    assert!(matches!(resume_outcome, ControlOutcome::None));
    let _controller = controller_next;
    let (next_worker, resume) = worker.recv::<ResumeMsg>().await?;
    let _worker = next_worker;
    println!("resume on destination: payload={resume:#X}");

    #[cfg(feature = "test-utils")]
    controller.phase_cursor().assert_terminal();
    #[cfg(feature = "test-utils")]
    worker.phase_cursor().assert_terminal();
    println!("post-splice protocol complete");
    Ok(())
}

// Errors
#[derive(Debug)]
enum DemoError {
    Attach(AttachError),
    Send(SendError),
    Recv(RecvError),
    Transport(TestTransportError),
    Protocol(String),
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemoError::Attach(err) => write!(f, "attach error: {err:?}"),
            DemoError::Send(err) => write!(f, "send error: {err:?}"),
            DemoError::Recv(err) => write!(f, "recv error: {err:?}"),
            DemoError::Transport(err) => write!(f, "transport error: {err:?}"),
            DemoError::Protocol(msg) => f.write_str(msg),
        }
    }
}

impl Error for DemoError {}

impl From<AttachError> for DemoError {
    fn from(err: AttachError) -> Self {
        DemoError::Attach(err)
    }
}

impl From<SendError> for DemoError {
    fn from(err: SendError) -> Self {
        DemoError::Send(err)
    }
}

impl From<RecvError> for DemoError {
    fn from(err: RecvError) -> Self {
        DemoError::Recv(err)
    }
}

impl From<TestTransportError> for DemoError {
    fn from(err: TestTransportError) -> Self {
        DemoError::Transport(err)
    }
}

impl From<&'static str> for DemoError {
    fn from(msg: &'static str) -> Self {
        DemoError::Protocol(msg.to_string())
    }
}

impl From<hibana::control::CpError> for DemoError {
    fn from(err: hibana::control::CpError) -> Self {
        DemoError::Protocol(format!("control-plane error: {err:?}"))
    }
}

impl From<std::io::Error> for DemoError {
    fn from(err: std::io::Error) -> Self {
        DemoError::Protocol(format!("io error: {err}"))
    }
}

// Development transport (Tokio, VecDeque-backed)
mod transport {
    use hibana::{
        observe::TapEvent,
        runtime::{config::CounterClock, consts::RING_EVENTS},
        transport::{Transport, TransportError, wire::Payload},
    };
    use std::{
        collections::VecDeque,
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::task::yield_now;

    pub type Clock = CounterClock;

    #[derive(Clone, Default)]
    pub struct TestTransport {
        queue: Arc<Mutex<VecDeque<FrameOwned>>>,
    }

    struct FrameOwned {
        payload: Vec<u8>,
    }

    impl TestTransport {
        pub fn is_empty(&self) -> bool {
            self.queue.lock().expect("queue mutex poisoned").is_empty()
        }
    }

    #[derive(Debug)]
    pub enum TestTransportError {
        Empty,
    }

    impl From<TestTransportError> for TransportError {
        fn from(err: TestTransportError) -> Self {
            match err {
                TestTransportError::Empty => TransportError::Failed,
            }
        }
    }

    impl Transport for TestTransport {
        type Error = TestTransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Send<'a>
            = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>>
        where
            Self: 'a;
        type Recv<'a>
            = Pin<Box<dyn Future<Output = Result<Payload<'a>, Self::Error>> + Send + 'a>>
        where
            Self: 'a;
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
            let payload = payload.as_bytes().to_vec();
            let queue = Arc::clone(&self.queue);
            Box::pin(async move {
                let frame = FrameOwned { payload };
                queue.lock().expect("queue mutex poisoned").push_back(frame);
                Ok(())
            })
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            let queue = Arc::clone(&self.queue);
            Box::pin(async move {
                loop {
                    if let Some(frame) = queue.lock().expect("queue mutex poisoned").pop_front() {
                        let FrameOwned { payload, .. } = frame;
                        let leaked = Box::leak(payload.into_boxed_slice());
                        return Ok(Payload::new(leaked));
                    }
                    yield_now().await;
                }
            })
        }
    }

    pub fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
        Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
    }

    pub fn leak_slab(size: usize) -> &'static mut [u8] {
        Box::leak(vec![0u8; size].into_boxed_slice())
    }

    pub fn leak_clock() -> &'static Clock {
        Box::leak(Box::new(CounterClock::new()))
    }
}
