#![cfg(feature = "std")]
mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use hibana::g::advanced::{CanonicalControl, MessageSpec, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    RendezvousId,
    cap::{GenericCapToken, advanced::RouteDecisionKind},
    policy::{DynamicResolution, PolicySignalsProvider, ResolverContext, ResolverError},
};
use hibana::substrate::{
    SessionCluster, SessionId,
    binding::{
        BindingSlot, Channel, IncomingClassification, SendDisposition, SendMetadata,
        TransportOpsError,
    },
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const LABEL_ROUTE_DECISION: u8 = 57;
hibana::impl_control_resource!(
    RouteRightKind,
    handle: RouteDecision,
    name: "RouteRightDecision",
    label: 70,
);

use std::collections::{BTreeMap, VecDeque};
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

// SAFETY: Test binding performs no network I/O and all methods return immediately.
unsafe impl BindingSlot for FlowBinding {
    fn on_send_with_meta(
        &mut self,
        meta: SendMetadata,
        payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError> {
        if meta.is_send() && meta.label == <Msg<71, u32> as MessageSpec>::LABEL {
            let mut shared = self.shared.lock().expect("flow binding lock");
            let channel = Channel::new(shared.next_channel);
            shared.next_channel += 1;
            shared.payloads.insert(channel.raw(), payload.to_vec());
            let classification = IncomingClassification {
                label: meta.label,
                instance: 0,
                has_fin: false,
                channel,
            };
            shared
                .incoming
                .entry(meta.peer)
                .or_default()
                .push_back(PendingInbound {
                    lane: meta.lane,
                    classification,
                });
            return Ok(SendDisposition::Handled);
        }
        Ok(SendDisposition::BypassTransport)
    }

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

fn register_route_resolvers_for_program<const ROLE: u8, Steps>(
    cluster: &SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    rv_id: RendezvousId,
    program: &RoleProgram<'static, ROLE, Steps>,
) {
    cluster
        .set_resolver(
            rv_id,
            program,
            hibana::substrate::policy::PolicyId::new(ROUTE_POLICY_ID),
            always_left_route_resolver,
        )
        .expect("register route resolver");
}

fn always_left_route_resolver(
    _cluster: &SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ResolverError> {
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

#[tokio::test]
async fn offer_decode_binding_consumes_classification_once() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();

    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rv");

    register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
    register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

    let sid = SessionId::new(901);
    let shared = Arc::new(Mutex::new(FlowBindingShared::default()));
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
    let transport = TestTransport::default();

    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rv");

    register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
    register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

    let sid = SessionId::new(902);
    let shared = Arc::new(Mutex::new(FlowBindingShared::default()));
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
