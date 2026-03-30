#![cfg(feature = "std")]
mod common;
#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::{RecvFuture, TestRx, TestTransport, TestTransportError, TestTransportMetrics, TestTx};
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use hibana::g::advanced::{CanonicalControl, MessageSpec, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    RendezvousId,
    cap::{GenericCapToken, advanced::RouteDecisionKind},
    policy::{DynamicResolution, PolicySignalsProvider, ResolverContext, ResolverError},
};
use hibana::substrate::{
    SessionId, SessionKit, Transport,
    binding::{BindingSlot, Channel, IncomingClassification, TransportOpsError},
    runtime::{Config, CounterClock, DefaultLabelUniverse},
    transport::Outgoing,
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const LABEL_ROUTE_DECISION: u8 = 57;
type RouteRightKind = route_control_kinds::RouteControl<70, 0>;

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

const ROUTE_POLICY_ID: u16 = 900;

const LEFT_ARM: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<71, u32>>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<71, u32>, 0>(),
);

const RIGHT_ARM: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<72, u32>>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<72, u32>, 0>(),
);

const ROUTE: g::Program<
    <SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<71, u32>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<72, u32>>, StepNil>,
        >,
    >>::Output,
> = g::route(LEFT_ARM, RIGHT_ARM);

const PROGRAM: g::Program<
    SeqSteps<
        <SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<71, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<72, u32>>, StepNil>,
            >,
        >>::Output,
        StepCons<SendStep<Role<0>, Role<1>, Msg<73, u32>>, StepNil>,
    >,
> = g::seq(ROUTE, g::send::<Role<0>, Role<1>, Msg<73, u32>, 0>());

static CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <SeqSteps<
        <SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<71, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<72, u32>>, StepNil>,
            >,
        >>::Output,
        StepCons<SendStep<Role<0>, Role<1>, Msg<73, u32>>, StepNil>,
    > as ProjectRole<Role<0>>>::Output,
> = project(&PROGRAM);

static WORKER_PROGRAM: RoleProgram<
    'static,
    1,
    <SeqSteps<
        <SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<71, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<70, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<72, u32>>, StepNil>,
            >,
        >>::Output,
        StepCons<SendStep<Role<0>, Role<1>, Msg<73, u32>>, StepNil>,
    > as ProjectRole<Role<1>>>::Output,
> = project(&PROGRAM);

#[derive(Clone, Copy)]
struct PendingInbound {
    lane: u8,
    classification: IncomingClassification,
}

#[derive(Default)]
struct FlowBindingShared {
    next_channel: u64,
    incoming: BTreeMap<u8, VecDeque<PendingInbound>>,
    payloads: BTreeMap<u64, Vec<u8>>,
}

#[derive(Clone)]
struct FlowBinding {
    role: u8,
    shared: Arc<Mutex<FlowBindingShared>>,
}

impl FlowBinding {
    fn new(role: u8, shared: Arc<Mutex<FlowBindingShared>>) -> Self {
        Self { role, shared }
    }
}

impl BindingSlot for FlowBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification> {
        let mut shared = self.shared.lock().expect("flow binding lock");
        let queue = shared.incoming.entry(self.role).or_default();
        let mut idx = 0usize;
        while idx < queue.len() {
            if let Some(entry) = queue.get(idx)
                && entry.lane == logical_lane
            {
                return queue.remove(idx).map(|pending| pending.classification);
            }
            idx += 1;
        }
        None
    }

    fn on_recv(&mut self, channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        let mut shared = self.shared.lock().expect("flow binding lock");
        let Some(payload) = shared.payloads.remove(&channel.raw()) else {
            return Err(TransportOpsError::ChannelNotFound);
        };
        if payload.len() > buf.len() {
            return Err(TransportOpsError::WriteFailed {
                expected: payload.len(),
                actual: buf.len(),
            });
        }
        buf[..payload.len()].copy_from_slice(payload.as_slice());
        Ok(payload.len())
    }

    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        None
    }
}

#[derive(Clone)]
struct FlowTransport {
    inner: TestTransport,
    shared: Arc<Mutex<FlowBindingShared>>,
}

impl FlowTransport {
    fn new(shared: Arc<Mutex<FlowBindingShared>>) -> Self {
        Self {
            inner: TestTransport::default(),
            shared,
        }
    }
}

impl Transport for FlowTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx
    where
        Self: 'a;
    type Send<'a>
        = Pin<Box<dyn std::future::Future<Output = Result<(), Self::Error>> + 'a>>
    where
        Self: 'a;
    type Recv<'a>
        = RecvFuture<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(local_role, session_id)
    }

    fn send<'a, 'f>(&'a self, tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>) -> Self::Send<'a>
    where
        'a: 'f,
    {
        if outgoing.meta.direction == hibana::substrate::transport::LocalDirection::Send
            && outgoing.meta.label == <Msg<71, u32> as MessageSpec>::LABEL
        {
            let mut shared = self.shared.lock().expect("flow binding lock");
            let channel = Channel::new(shared.next_channel);
            shared.next_channel += 1;
            shared
                .payloads
                .insert(channel.raw(), outgoing.payload.as_bytes().to_vec());
            let classification = IncomingClassification {
                label: outgoing.meta.label,
                instance: 0,
                has_fin: false,
                channel,
            };
            shared
                .incoming
                .entry(outgoing.meta.peer)
                .or_default()
                .push_back(PendingInbound {
                    lane: outgoing.meta.lane,
                    classification,
                });
            return Box::pin(std::future::ready(Ok(())));
        }
        self.inner.send(tx, outgoing)
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        self.inner.recv(rx)
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.inner.requeue(rx)
    }

    fn drain_events(&self, emit: &mut dyn FnMut(hibana::substrate::transport::TransportEvent)) {
        self.inner.drain_events(emit)
    }

    fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8> {
        self.inner.recv_label_hint(rx)
    }

    fn metrics(&self) -> Self::Metrics {
        self.inner.metrics()
    }

    fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16) {
        self.inner.apply_pacing_update(interval_us, burst_bytes)
    }
}

fn register_route_resolvers_for_program<const ROLE: u8, Steps, T>(
    cluster: &SessionKit<'static, T, DefaultLabelUniverse, CounterClock, 4>,
    rv_id: RendezvousId,
    program: &RoleProgram<'static, ROLE, Steps>,
) where
    T: Transport + 'static,
{
    cluster
        .set_resolver::<ROUTE_POLICY_ID, ROLE, _, _>(
            rv_id,
            program,
            hibana::substrate::policy::ResolverRef::from_fn(always_left_route_resolver),
        )
        .expect("register route resolver");
}

fn always_left_route_resolver(_ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

#[tokio::test]
async fn offer_decode_binding_consumes_classification_once() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let shared = Arc::new(Mutex::new(FlowBindingShared::default()));
    let transport = FlowTransport::new(Arc::clone(&shared));

    let cluster: &mut SessionKit<'static, FlowTransport, DefaultLabelUniverse, CounterClock, 4> =
        Box::leak(Box::new(SessionKit::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rv");

    register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
    register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

    let sid = SessionId::new(901);
    let controller_binding = FlowBinding::new(0, Arc::clone(&shared));
    let worker_binding = FlowBinding::new(1, Arc::clone(&shared));

    let mut controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, controller_binding)
        .expect("attach controller");
    let worker = cluster
        .enter::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, worker_binding)
        .expect("attach worker");

    let (controller_after_control, outcome) = controller
        .flow::<Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >>()
        .expect("control flow")
        .send(())
        .await
        .expect("send route control");
    assert!(outcome.is_canonical());
    controller = controller_after_control;

    let (controller_after_data, _outcome) = controller
        .flow::<Msg<71, u32>>()
        .expect("left data flow")
        .send(&4444)
        .await
        .expect("send left data");
    controller = controller_after_data;

    let worker_branch = worker.offer().await.expect("offer left arm");
    assert_eq!(worker_branch.label(), <Msg<71, u32> as MessageSpec>::LABEL);
    let (worker_after_data, data_value) = worker_branch
        .decode::<Msg<71, u32>>()
        .await
        .expect("decode left data");
    assert_eq!(data_value, 4444);

    let (_controller_after_tail, _outcome) = controller
        .flow::<Msg<73, u32>>()
        .expect("tail flow")
        .send(&55)
        .await
        .expect("send tail");

    let (_worker_after_tail, tail) = worker_after_data
        .recv::<Msg<73, u32>>()
        .await
        .expect("recv tail after offer/decode");
    assert_eq!(tail, 55);
}

#[tokio::test]
async fn dynamic_route_passive_ignores_non_authoritative_binding_classification() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let shared = Arc::new(Mutex::new(FlowBindingShared::default()));
    let transport = FlowTransport::new(Arc::clone(&shared));

    let cluster: &mut SessionKit<'static, FlowTransport, DefaultLabelUniverse, CounterClock, 4> =
        Box::leak(Box::new(SessionKit::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rv");

    register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
    register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

    let sid = SessionId::new(902);
    let controller_binding = FlowBinding::new(0, Arc::clone(&shared));
    let worker_binding = FlowBinding::new(1, Arc::clone(&shared));

    let mut controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, controller_binding)
        .expect("attach controller");
    let worker = cluster
        .enter::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, worker_binding)
        .expect("attach worker");

    {
        let mut guard = shared.lock().expect("flow binding lock");
        guard
            .incoming
            .entry(1)
            .or_default()
            .push_back(PendingInbound {
                lane: 0,
                classification: IncomingClassification {
                    label: <Msg<72, u32> as MessageSpec>::LABEL,
                    instance: 0,
                    has_fin: false,
                    channel: Channel::new(999),
                },
            });
    }

    let (controller_after_control, outcome) = controller
        .flow::<Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >>()
        .expect("control flow")
        .send(())
        .await
        .expect("send route control");
    assert!(outcome.is_canonical());
    controller = controller_after_control;

    let (controller_after_data, _outcome) = controller
        .flow::<Msg<71, u32>>()
        .expect("left data flow")
        .send(&7777)
        .await
        .expect("send left data");
    controller = controller_after_data;

    let worker_branch = worker.offer().await.expect("offer left arm");
    assert_eq!(worker_branch.label(), <Msg<71, u32> as MessageSpec>::LABEL);
    let (_worker_after_data, value) = worker_branch
        .decode::<Msg<71, u32>>()
        .await
        .expect("decode left data");
    assert_eq!(value, 7777);

    let (_controller_after_tail, _outcome) = controller
        .flow::<Msg<73, u32>>()
        .expect("tail flow")
        .send(&1)
        .await
        .expect("send tail");
}
