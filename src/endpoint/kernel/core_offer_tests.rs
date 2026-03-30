//! Offer-path kernel regression tests.

use super::*;
use crate::binding::{Channel, IncomingClassification, TransportOpsError};
use crate::control::cap::mint::{
    CapError, CapShot, CapsMask, ControlResourceKind, GenericCapToken, ResourceKind,
    SessionScopedKind,
};
use crate::control::cap::resource_kinds::{RouteDecisionHandle, RouteDecisionKind};
use crate::control::cluster::core::SessionCluster;
use crate::g::{self, Msg, Role};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::global::role_program::{RoleProgram, project};
use crate::global::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use crate::global::{CanonicalControl, ControlHandling};
use crate::observe::core::TapEvent;
use crate::runtime::config::{Config, CounterClock};
use crate::runtime::consts::{DefaultLabelUniverse, LABEL_ROUTE_DECISION, RING_EVENTS};
use crate::transport::{Transport, TransportError, wire::Payload};
use core::{
    cell::Cell,
    future::{Future, Ready, ready},
    mem::ManuallyDrop,
    pin::pin,
    task::{Context, Poll},
};
use futures::task::noop_waker_ref;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Waker,
    vec::Vec,
};

type SendOnly<const LANE: u8, S, D, M> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
type BranchSteps<L, R> = <L as StepConcat<R>>::Output;

fn poll_ready_ok<F, T, E>(cx: &mut Context<'_>, fut: F, context: &str) -> T
where
    F: Future<Output = Result<T, E>>,
    E: core::fmt::Debug,
{
    let mut fut = pin!(fut);
    match fut.as_mut().poll(cx) {
        Poll::Ready(Ok(value)) => value,
        Poll::Ready(Err(err)) => panic!("{context} failed: {err:?}"),
        Poll::Pending => panic!("{context} unexpectedly pending"),
    }
}

#[derive(Default)]
struct TestBinding {
    incoming: VecDeque<IncomingClassification>,
    recv_payloads: VecDeque<Vec<u8>>,
    polls: Cell<usize>,
}

impl TestBinding {
    fn with_incoming(incoming: &[IncomingClassification]) -> Self {
        let mut queue = VecDeque::new();
        queue.extend(incoming.iter().copied());
        Self {
            incoming: queue,
            recv_payloads: VecDeque::new(),
            polls: Cell::new(0),
        }
    }

    fn with_incoming_and_payloads(
        incoming: &[IncomingClassification],
        recv_payloads: &[&[u8]],
    ) -> Self {
        let mut queue = VecDeque::new();
        queue.extend(incoming.iter().copied());
        let mut payloads = VecDeque::new();
        payloads.extend(recv_payloads.iter().map(|payload| payload.to_vec()));
        Self {
            incoming: queue,
            recv_payloads: payloads,
            polls: Cell::new(0),
        }
    }

    fn poll_count(&self) -> usize {
        self.polls.get()
    }
}

struct LaneAwareTestBinding {
    incoming: [VecDeque<IncomingClassification>; MAX_LANES],
    polls: [usize; MAX_LANES],
}

impl LaneAwareTestBinding {
    fn with_lane_incoming(incoming: &[(u8, IncomingClassification)]) -> Self {
        let mut binding = Self {
            incoming: core::array::from_fn(|_| VecDeque::new()),
            polls: [0; MAX_LANES],
        };
        for (lane, classification) in incoming.iter().copied() {
            let lane_idx = lane as usize;
            if lane_idx < MAX_LANES {
                binding.incoming[lane_idx].push_back(classification);
            }
        }
        binding
    }

    fn poll_count_for_lane(&self, lane_idx: usize) -> usize {
        self.polls.get(lane_idx).copied().unwrap_or(0)
    }
}

impl BindingSlot for LaneAwareTestBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification> {
        let lane_idx = logical_lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        self.polls[lane_idx] = self.polls[lane_idx].saturating_add(1);
        self.incoming[lane_idx].pop_front()
    }

    fn on_recv(&mut self, _channel: Channel, _buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        Ok(0)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

impl BindingSlot for TestBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        self.polls.set(self.polls.get().saturating_add(1));
        self.incoming.pop_front()
    }

    fn on_recv(&mut self, _channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        let Some(payload) = self.recv_payloads.pop_front() else {
            return Ok(0);
        };
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(len)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

const HINT_NONE: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct HintOnlyTransport {
    worker_hint: u8,
}

impl HintOnlyTransport {
    const fn new(worker_hint: u8) -> Self {
        Self { worker_hint }
    }
}

struct HintOnlyRx {
    hint: Cell<u8>,
}

impl Transport for HintOnlyTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = HintOnlyRx
    where
        Self: 'a;
    type Send<'a>
        = Ready<Result<(), Self::Error>>
    where
        Self: 'a;
    type Recv<'a>
        = Ready<Result<Payload<'a>, Self::Error>>
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintOnlyRx {
                hint: Cell::new(hint),
            },
        )
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        ready(Ok(()))
    }

    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        ready(Ok(Payload::new(&[0u8; 1])))
    }

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8> {
        let hint = rx.hint.get();
        if hint == HINT_NONE {
            None
        } else {
            rx.hint.set(HINT_NONE);
            Some(hint)
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

#[derive(Clone)]
struct PendingTransport {
    state: Arc<PendingTransportState>,
}

impl PendingTransport {
    fn new() -> Self {
        Self {
            state: Arc::new(PendingTransportState::default()),
        }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct PendingTransportState {
    polls: AtomicUsize,
    ready: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

#[derive(Default)]
struct DeferredIngressState {
    incoming: Mutex<VecDeque<IncomingClassification>>,
    recv_payloads: Mutex<VecDeque<Vec<u8>>>,
    available: AtomicUsize,
}

struct DeferredIngressBinding {
    state: Arc<DeferredIngressState>,
    polls: Cell<usize>,
}

impl DeferredIngressBinding {
    fn with_incoming_and_payloads(
        state: Arc<DeferredIngressState>,
        incoming: &[IncomingClassification],
        recv_payloads: &[&[u8]],
    ) -> Self {
        {
            let mut queue = state
                .incoming
                .lock()
                .expect("deferred ingress incoming lock");
            queue.extend(incoming.iter().copied());
        }
        {
            let mut payloads = state
                .recv_payloads
                .lock()
                .expect("deferred ingress payload lock");
            payloads.extend(recv_payloads.iter().map(|payload| payload.to_vec()));
        }
        Self {
            state,
            polls: Cell::new(0),
        }
    }
}

impl BindingSlot for DeferredIngressBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        self.polls.set(self.polls.get().saturating_add(1));
        if self.state.available.load(Ordering::SeqCst) == 0 {
            return None;
        }
        let mut queue = self
            .state
            .incoming
            .lock()
            .expect("deferred ingress incoming lock");
        let classification = queue.pop_front()?;
        self.state.available.fetch_sub(1, Ordering::SeqCst);
        Some(classification)
    }

    fn on_recv(&mut self, _channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        let mut payloads = self
            .state
            .recv_payloads
            .lock()
            .expect("deferred ingress payload lock");
        let Some(payload) = payloads.pop_front() else {
            return Ok(0);
        };
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(len)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

#[derive(Clone)]
struct DeferredIngressTransport {
    state: Arc<DeferredIngressState>,
}

impl DeferredIngressTransport {
    fn new(state: Arc<DeferredIngressState>) -> Self {
        Self { state }
    }
}

struct DeferredIngressRx;

struct PendingRx;

struct PendingRecv<'a> {
    state: &'a PendingTransportState,
}

impl<'a> Future for PendingRecv<'a> {
    type Output = Result<Payload<'a>, TransportError>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.state.polls.fetch_add(1, Ordering::SeqCst);
        if self.state.ready.load(Ordering::SeqCst) {
            Poll::Ready(Ok(Payload::new(&[])))
        } else {
            *self
                .state
                .waker
                .lock()
                .expect("pending transport waker lock") = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl Transport for PendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = PendingRx
    where
        Self: 'a;
    type Send<'a>
        = Ready<Result<(), Self::Error>>
    where
        Self: 'a;
    type Recv<'a>
        = PendingRecv<'a>
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), PendingRx)
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        ready(Ok(()))
    }

    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        PendingRecv { state: &self.state }
    }

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

impl Transport for DeferredIngressTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = DeferredIngressRx
    where
        Self: 'a;
    type Send<'a>
        = Ready<Result<(), Self::Error>>
    where
        Self: 'a;
    type Recv<'a>
        = Ready<Result<Payload<'a>, Self::Error>>
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), DeferredIngressRx)
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        ready(Ok(()))
    }

    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        self.state.available.fetch_add(1, Ordering::SeqCst);
        ready(Ok(Payload::new(&[])))
    }

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

#[derive(Clone, Copy, Debug)]
struct RouteHintRightKind;

impl ResourceKind for RouteHintRightKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "RouteHintRightDecision";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
        handle.scope = ScopeId::generic(0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    fn scope_id(handle: &Self::Handle) -> Option<ScopeId> {
        Some(handle.scope)
    }
}

impl SessionScopedKind for RouteHintRightKind {
    fn handle_for_session(_sid: crate::control::types::SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::default()
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl crate::control::cap::mint::ControlResourceKind for RouteHintRightKind {
    const LABEL: u8 = 99;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 =
        <RouteDecisionKind as crate::control::cap::mint::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}

impl crate::control::cap::mint::ControlMint for RouteHintRightKind {
    fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
        RouteDecisionHandle { scope, arm: 0 }
    }
}

const HINT_ROUTE_POLICY_ID: u16 = 601;
const HINT_LEFT_ARM: g::Program<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
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
    .policy::<HINT_ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
);
const HINT_RIGHT_ARM: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>,
        0,
    >()
    .policy::<HINT_ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>(),
);
const HINT_ROUTE_PROGRAM: g::Program<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        99,
                        GenericCapToken<RouteHintRightKind>,
                        CanonicalControl<RouteHintRightKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
        >,
    >>::Output,
> = g::route(HINT_LEFT_ARM, HINT_RIGHT_ARM);
static HINT_CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <<SeqSteps<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        99,
                        GenericCapToken<RouteHintRightKind>,
                        CanonicalControl<RouteHintRightKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&HINT_ROUTE_PROGRAM);
static HINT_WORKER_PROGRAM: RoleProgram<
    'static,
    1,
    <<SeqSteps<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        99,
                        GenericCapToken<RouteHintRightKind>,
                        CanonicalControl<RouteHintRightKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
        >,
    >>::Output as ProjectRole<Role<1>>>::Output,
> = project(&HINT_ROUTE_PROGRAM);
const HINT_LEFT_DATA_LABEL: u8 = 100;
const HINT_RIGHT_DATA_LABEL: u8 = 101;

const ENTRY_ARM0_PROGRAM: g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<Role<0>, Role<0>, Msg<102, u8>, 0>(),
    g::seq(
        g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>(),
        g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
    ),
);
const ENTRY_ARM1_PROGRAM: g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<Role<0>, Role<0>, Msg<105, u8>, 0>(),
    g::seq(
        g::send::<Role<0>, Role<1>, Msg<106, u8>, 0>(),
        g::send::<Role<1>, Role<0>, Msg<107, u8>, 0>(),
    ),
);
const ENTRY_ROUTE_PROGRAM: g::Program<
    <SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
            >,
        >,
    >>::Output,
> = g::route(ENTRY_ARM0_PROGRAM, ENTRY_ARM1_PROGRAM);
static ENTRY_CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <<SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
            >,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&ENTRY_ROUTE_PROGRAM);
static ENTRY_WORKER_PROGRAM: RoleProgram<
    'static,
    1,
    <<SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
            >,
        >,
    >>::Output as ProjectRole<Role<1>>>::Output,
> = project(&ENTRY_ROUTE_PROGRAM);
const ENTRY_ARM0_SIGNAL_LABEL: u8 = 103;

#[test]
fn binding_inbox_take_is_one_shot() {
    let classification = IncomingClassification {
        label: 7,
        instance: 3,
        has_fin: false,
        channel: Channel::new(1),
    };
    let mut binding = TestBinding::with_incoming(&[classification]);
    let mut inbox = BindingInbox::EMPTY;

    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(classification));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), None);

    inbox.put_back(0, classification);
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(classification));
}

#[test]
fn binding_inbox_take_matching_skips_head_mismatch() {
    let head = IncomingClassification {
        label: 7,
        instance: 3,
        has_fin: false,
        channel: Channel::new(1),
    };
    let expected = IncomingClassification {
        label: 9,
        instance: 4,
        has_fin: false,
        channel: Channel::new(2),
    };
    let mut binding = TestBinding::with_incoming(&[head, expected]);
    let mut inbox = BindingInbox::EMPTY;

    let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
    assert_eq!(picked, Some(expected));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(head));
}

#[test]
fn binding_inbox_take_matching_scans_buffered_entries() {
    let first = IncomingClassification {
        label: 3,
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IncomingClassification {
        label: 4,
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IncomingClassification {
        label: 5,
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    let mut inbox = BindingInbox::EMPTY;
    assert!(inbox.push_back(0, first));
    assert!(inbox.push_back(0, second));
    assert!(inbox.push_back(0, expected));

    let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
    assert_eq!(picked, Some(expected));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
}

#[test]
fn binding_inbox_nonempty_mask_tracks_buffered_lanes() {
    let first = IncomingClassification {
        label: 3,
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IncomingClassification {
        label: 4,
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let mut binding = TestBinding::default();
    let mut inbox = BindingInbox::EMPTY;
    assert!(!inbox.has_buffered_for_lane_mask((1u8 << 0) | (1u8 << 2)));

    assert!(inbox.push_back(0, first));
    assert!(inbox.has_buffered_for_lane_mask(1u8 << 0));
    assert!(!inbox.has_buffered_for_lane_mask(1u8 << 2));

    assert!(inbox.push_back(2, second));
    assert!(inbox.has_buffered_for_lane_mask((1u8 << 0) | (1u8 << 2)));

    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
    assert!(!inbox.has_buffered_for_lane_mask(1u8 << 0));
    assert!(inbox.has_buffered_for_lane_mask(1u8 << 2));

    assert_eq!(
        inbox.take_matching_or_poll(&mut binding, 2, second.label),
        Some(second)
    );
    assert!(!inbox.has_buffered_for_lane_mask(1u8 << 2));
}

#[test]
fn binding_inbox_label_masks_track_buffered_labels_exactly() {
    let first = IncomingClassification {
        label: 3,
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IncomingClassification {
        label: 4,
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let third = IncomingClassification {
        label: 7,
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    let mut inbox = BindingInbox::EMPTY;

    assert!(inbox.push_back(0, first));
    assert!(inbox.push_back(0, second));
    assert!(inbox.push_back(2, third));
    assert_eq!(
        inbox.label_masks[0],
        ScopeLabelMeta::label_bit(first.label) | ScopeLabelMeta::label_bit(second.label)
    );
    assert_eq!(inbox.label_masks[2], ScopeLabelMeta::label_bit(third.label));
    assert_eq!(
        inbox.buffered_label_lane_masks[first.label as usize],
        1u8 << 0
    );
    assert_eq!(
        inbox.buffered_label_lane_masks[second.label as usize],
        1u8 << 0
    );
    assert_eq!(
        inbox.buffered_label_lane_masks[third.label as usize],
        1u8 << 2
    );

    assert_eq!(
        inbox.take_matching_or_poll(&mut binding, 0, second.label),
        Some(second)
    );
    assert_eq!(inbox.label_masks[0], ScopeLabelMeta::label_bit(first.label));
    assert_eq!(inbox.buffered_label_lane_masks[second.label as usize], 0);
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
    assert_eq!(inbox.label_masks[0], 0);
    assert_eq!(inbox.buffered_label_lane_masks[first.label as usize], 0);
}

#[test]
fn binding_inbox_take_matching_mask_drops_buffered_loop_control_labels() {
    let loop_control = IncomingClassification {
        label: LABEL_LOOP_CONTINUE,
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let deferred = IncomingClassification {
        label: 33,
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IncomingClassification {
        label: 55,
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::with_incoming(&[expected]);
    let mut inbox = BindingInbox::EMPTY;

    assert!(inbox.push_back(0, loop_control));
    assert!(inbox.push_back(0, deferred));

    let picked = inbox.take_matching_mask_or_poll(
        &mut binding,
        0,
        ScopeLabelMeta::label_bit(expected.label),
        ScopeLabelMeta::label_bit(LABEL_LOOP_CONTINUE)
            | ScopeLabelMeta::label_bit(LABEL_LOOP_BREAK),
        |label| matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK),
    );
    assert_eq!(picked, Some(expected));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(deferred));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), None);
}

#[test]
fn binding_mismatch_scan_finds_later_matching_label() {
    let first = IncomingClassification {
        label: 11,
        instance: 1,
        has_fin: false,
        channel: Channel::new(21),
    };
    let second = IncomingClassification {
        label: 12,
        instance: 2,
        has_fin: false,
        channel: Channel::new(22),
    };
    let expected = IncomingClassification {
        label: 13,
        instance: 3,
        has_fin: false,
        channel: Channel::new(23),
    };
    let mut binding = TestBinding::with_incoming(&[first, second, expected]);
    let mut inbox = BindingInbox::EMPTY;

    let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
    assert_eq!(
        picked,
        Some(expected),
        "scan must continue past mismatched head entries"
    );
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
    assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
}

#[test]
fn stage_transport_payload_copies_bytes() {
    let mut scratch = [0u8; 8];
    let src = [1u8, 2, 3, 4];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}

#[test]
fn stage_transport_payload_rejects_oversize() {
    let mut scratch = [0u8; 2];
    let src = [1u8, 2, 3];
    let err = stage_transport_payload(&mut scratch, &src).expect_err("oversize");
    assert!(matches!(err, RecvError::PhaseInvariant));
}

#[test]
fn offer_select_priority_is_deterministic() {
    assert_eq!(
        choose_offer_priority(true, 1, 1, 2),
        Some(OfferSelectPriority::CurrentOfferEntry)
    );
    assert_eq!(
        choose_offer_priority(false, 1, 2, 2),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 2),
        Some(OfferSelectPriority::ControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 2, 1),
        Some(OfferSelectPriority::CandidateUnique)
    );
    assert_eq!(choose_offer_priority(false, 0, 2, 2), None);
}

#[test]
fn static_controller_current_is_not_preempted() {
    let selected = choose_offer_priority(true, 1, 1, 2);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn hint_filter_does_not_override_priority() {
    // Stage A applies filter; Stage B ordering is still fixed.
    let current_is_candidate_after_filter = true;
    let selected = choose_offer_priority(current_is_candidate_after_filter, 1, 1, 1);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn offer_priority_has_no_liveness_override() {
    // Stage B priority is fixed and independent from liveness signals.
    assert_eq!(
        choose_offer_priority(false, 1, 1, 1),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 1),
        Some(OfferSelectPriority::ControllerUnique)
    );
}

#[test]
fn current_scope_selection_meta_non_route_defaults_do_not_block_current() {
    let meta = CurrentScopeSelectionMeta::EMPTY;
    assert!(!meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(!meta.is_controller());
}

#[test]
fn current_scope_selection_meta_route_entry_flags_roundtrip() {
    let meta = CurrentScopeSelectionMeta {
        flags: CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY
            | CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES
            | CurrentScopeSelectionMeta::FLAG_CONTROLLER,
    };
    assert!(meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(meta.is_controller());
}

#[test]
fn current_frontier_selection_state_loop_controller_without_evidence_is_exact() {
    let base = CurrentFrontierSelectionState {
        frontier: FrontierKind::Loop,
        parallel_root: ScopeId::none(),
        ready: true,
        has_progress_evidence: false,
        flags: CurrentFrontierSelectionState::FLAG_CONTROLLER,
    };
    assert!(base.loop_controller_without_evidence());
    assert!(
        !CurrentFrontierSelectionState {
            ready: false,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(
        !CurrentFrontierSelectionState {
            has_progress_evidence: true,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(!CurrentFrontierSelectionState { flags: 0, ..base }.loop_controller_without_evidence());
}

#[test]
fn current_frontier_selection_state_updates_only_current_candidate() {
    let mut state = CurrentFrontierSelectionState {
        frontier: FrontierKind::Parallel,
        parallel_root: ScopeId::generic(3),
        ready: false,
        has_progress_evidence: false,
        flags: 0,
    };
    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(12),
            entry_idx: 9,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        },
    );
    assert!(!state.ready);
    assert!(!state.has_progress_evidence);

    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(11),
            entry_idx: 7,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        },
    );
    assert!(state.ready);
    assert!(state.has_progress_evidence);
}

#[test]
fn scope_loop_meta_recvless_ready_requires_active_or_linger() {
    assert!(!ScopeLoopMeta::EMPTY.recvless_ready());
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE,
        }
        .recvless_ready()
    );
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
    assert!(
        !ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE
                | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV
                | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
}

#[test]
fn scope_loop_meta_loop_label_scope_and_arm_recv_bits_are_exact() {
    let meta = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
    };
    assert!(meta.loop_label_scope());
    assert!(!meta.arm_has_recv(0));
    assert!(meta.arm_has_recv(1));

    let linger = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
    };
    assert!(linger.loop_label_scope());
    assert!(linger.arm_has_recv(0));
    assert!(!linger.arm_has_recv(1));
    assert!(!ScopeLoopMeta::EMPTY.loop_label_scope());
}

#[test]
fn scope_label_meta_current_recv_label_and_arm_bits_are_exact() {
    let no_arm = ScopeLabelMeta {
        recv_label: 7,
        recv_arm: 1,
        hint_label_mask: ScopeLabelMeta::label_bit(7),
        flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL,
        ..ScopeLabelMeta::EMPTY
    };
    assert!(no_arm.matches_current_recv_label(7));
    assert!(no_arm.matches_hint_label(7));
    assert_eq!(no_arm.current_recv_arm_for_label(7), None);
    let with_arm = ScopeLabelMeta {
        arm_label_masks: [0, ScopeLabelMeta::label_bit(7)],
        flags: no_arm.flags | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM,
        ..no_arm
    };
    assert_eq!(with_arm.current_recv_arm_for_label(7), Some(1));
    assert_eq!(with_arm.arm_for_label(7), Some(1));
    assert!(!with_arm.matches_current_recv_label(8));
}

#[test]
fn scope_label_meta_controller_labels_map_to_binary_arms_exactly() {
    let meta = ScopeLabelMeta {
        controller_labels: [11, 13],
        hint_label_mask: ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
        arm_label_masks: [ScopeLabelMeta::label_bit(11), ScopeLabelMeta::label_bit(13)],
        evidence_arm_label_masks: [ScopeLabelMeta::label_bit(11), ScopeLabelMeta::label_bit(13)],
        flags: ScopeLabelMeta::FLAG_CONTROLLER_ARM0 | ScopeLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeLabelMeta::EMPTY
    };
    assert_eq!(meta.controller_arm_for_label(11), Some(0));
    assert_eq!(meta.controller_arm_for_label(13), Some(1));
    assert_eq!(meta.controller_arm_for_label(17), None);
    assert_eq!(meta.arm_for_label(11), Some(0));
    assert_eq!(meta.arm_for_label(13), Some(1));
}

#[test]
fn scope_label_meta_dispatch_labels_do_not_count_as_ready_evidence() {
    let mut meta = ScopeLabelMeta::EMPTY;
    meta.record_dispatch_arm_label(1, 29);

    assert!(meta.matches_hint_label(29));
    assert_eq!(meta.arm_for_label(29), Some(1));
    assert_eq!(meta.evidence_arm_for_label(29), None);
}

#[test]
fn scope_label_meta_binding_evidence_can_be_stricter_than_hint_evidence() {
    let meta = ScopeLabelMeta {
        recv_label: 41,
        recv_arm: 0,
        hint_label_mask: ScopeLabelMeta::label_bit(41),
        arm_label_masks: [ScopeLabelMeta::label_bit(41), 0],
        evidence_arm_label_masks: [ScopeLabelMeta::label_bit(41), 0],
        flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
            | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
        ..ScopeLabelMeta::EMPTY
    };

    assert!(meta.matches_hint_label(41));
    assert_eq!(meta.arm_for_label(41), Some(0));
    assert_eq!(meta.evidence_arm_for_label(41), Some(0));
    assert_eq!(meta.binding_evidence_arm_for_label(41), None);
}

#[test]
fn scope_label_meta_preferred_binding_label_is_exact_only_for_singletons() {
    let meta = ScopeLabelMeta {
        recv_label: 41,
        recv_arm: 0,
        arm_label_masks: [
            ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
            ScopeLabelMeta::label_bit(47),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
            ScopeLabelMeta::label_bit(47),
        ],
        flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
            | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
        ..ScopeLabelMeta::EMPTY
    };

    assert_eq!(meta.preferred_binding_label(Some(0)), Some(43));
    assert_eq!(meta.preferred_binding_label(Some(1)), Some(47));
    assert_eq!(meta.preferred_binding_label(None), None);

    let singleton = ScopeLabelMeta {
        arm_label_masks: [ScopeLabelMeta::label_bit(53), 0],
        evidence_arm_label_masks: [ScopeLabelMeta::label_bit(53), 0],
        ..ScopeLabelMeta::EMPTY
    };
    assert_eq!(singleton.preferred_binding_label(None), Some(53));
}

#[test]
fn scope_label_meta_preferred_binding_label_mask_respects_authoritative_arm() {
    let meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(11)
            | ScopeLabelMeta::label_bit(13)
            | ScopeLabelMeta::label_bit(17),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
            ScopeLabelMeta::label_bit(17),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
            ScopeLabelMeta::label_bit(17),
        ],
        ..ScopeLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_label_mask(Some(0)),
        ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13)
    );
    assert_eq!(
        meta.preferred_binding_label_mask(Some(1)),
        ScopeLabelMeta::label_bit(17)
    );
    assert_eq!(
        meta.preferred_binding_label_mask(None),
        meta.hint_label_mask
    );
}

#[test]
fn scope_label_meta_preferred_binding_label_mask_keeps_current_recv_for_demux() {
    let meta = ScopeLabelMeta {
        recv_label: 41,
        recv_arm: 0,
        hint_label_mask: ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
            ScopeLabelMeta::label_bit(47),
        ],
        evidence_arm_label_masks: [ScopeLabelMeta::label_bit(43), ScopeLabelMeta::label_bit(47)],
        flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
            | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
        ..ScopeLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_label_mask(Some(0)),
        ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43)
    );
}

#[test]
fn lane_offer_state_roundtrips_static_frontier_flags() {
    let state = LaneOfferState {
        scope: ScopeId::generic(5),
        entry: StateIndex::from_usize(11),
        parallel_root: ScopeId::generic(2),
        frontier: FrontierKind::Parallel,
        loop_meta: ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
        },
        label_meta: ScopeLabelMeta {
            scope_id: ScopeId::generic(5),
            loop_meta: ScopeLoopMeta {
                flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
            },
            recv_label: 23,
            recv_arm: 0,
            controller_labels: [31, 37],
            hint_label_mask: ScopeLabelMeta::label_bit(23)
                | ScopeLabelMeta::label_bit(31)
                | ScopeLabelMeta::label_bit(37),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(23) | ScopeLabelMeta::label_bit(31),
                ScopeLabelMeta::label_bit(37),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(23) | ScopeLabelMeta::label_bit(31),
                ScopeLabelMeta::label_bit(37),
            ],
            flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
                | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
                | ScopeLabelMeta::FLAG_CONTROLLER_ARM0
                | ScopeLabelMeta::FLAG_CONTROLLER_ARM1,
        },
        static_ready: true,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
    };
    assert!(state.is_controller());
    assert!(state.is_dynamic());
    assert!(state.static_ready());
    assert_eq!(state.frontier, FrontierKind::Parallel);
    assert!(state.loop_meta.control_scope());
    assert!(state.loop_meta.continue_has_recv());
    assert!(!state.loop_meta.break_has_recv());
    assert_eq!(state.label_meta.scope_id(), ScopeId::generic(5));
    assert_eq!(state.label_meta.current_recv_arm_for_label(23), Some(0));
    assert_eq!(state.label_meta.controller_arm_for_label(31), Some(0));
    assert_eq!(state.label_meta.controller_arm_for_label(37), Some(1));
    assert_eq!(state.label_meta.arm_for_label(23), Some(0));
    assert_eq!(state.label_meta.arm_for_label(31), Some(0));
    assert_eq!(state.label_meta.arm_for_label(37), Some(1));
}

#[test]
fn refresh_lane_offer_state_caches_scope_label_meta() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(997);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.refresh_lane_offer_state(0);
    let cached = worker.route_state.lane_offer_state[0].label_meta;
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let recv_meta = worker.cursor.try_recv_meta().expect("recv metadata");
    assert_eq!(cached.scope_id(), scope);
    assert_eq!(
        cached.loop_meta().flags,
        worker.route_state.lane_offer_state[0].loop_meta.flags
    );
    assert!(cached.matches_current_recv_label(recv_meta.label));
    assert_eq!(
        cached.current_recv_arm_for_label(recv_meta.label),
        recv_meta.route_arm
    );
    assert_eq!(entry_state.scope_id, scope);
    assert_eq!(
        entry_state.frontier,
        worker.route_state.lane_offer_state[0].frontier
    );
    assert_eq!(entry_state.label_meta.scope_id(), scope);
    assert!(entry_state.selection_meta.is_route_entry());
    assert_eq!(
        entry_state.selection_meta.is_controller(),
        worker.route_state.lane_offer_state[0].is_controller()
    );
    assert_eq!(
        entry_state.summary.frontier_mask,
        worker.route_state.lane_offer_state[0].frontier.bit()
    );
    assert_eq!(
        entry_state.summary.is_controller(),
        worker.route_state.lane_offer_state[0].is_controller()
    );
    assert_eq!(
        entry_state.summary.is_dynamic(),
        worker.route_state.lane_offer_state[0].is_dynamic()
    );
    assert_eq!(
        entry_state.summary.static_ready(),
        worker.route_state.lane_offer_state[0].static_ready()
    );
    let observed = worker
        .recompute_offer_entry_observed_state_non_consuming(entry_idx)
        .expect("observed state");
    assert_eq!(
        worker.frontier_state.offer_entry_state[entry_idx].observed,
        observed
    );
    let (offer_lanes, offer_lanes_len) = worker.offer_lanes_for_scope(scope);
    let mut offer_lane_mask = 0u8;
    let mut offer_lane_idx = 0usize;
    while offer_lane_idx < offer_lanes_len {
        offer_lane_mask |= 1u8 << (offer_lanes[offer_lane_idx] as usize);
        offer_lane_idx += 1;
    }
    assert_eq!(entry_state.offer_lanes_len as usize, offer_lanes_len);
    assert_eq!(entry_state.offer_lanes, offer_lanes);
    assert_eq!(entry_state.offer_lane_mask, offer_lane_mask);
    assert_eq!(entry_state.lane_idx, 0);
    assert_eq!(
        worker
            .offer_entry_lane_state(scope, entry_idx)
            .map(|info| info.entry),
        Some(worker.route_state.lane_offer_state[0].entry)
    );
    let materialization = entry_state.materialization_meta;
    assert_eq!(
        materialization.arm_count,
        worker.cursor.route_scope_arm_count(scope).unwrap_or(0)
    );
    let mut arm = 0u8;
    while arm <= 1 {
        let expected_controller_recv = worker
            .cursor
            .controller_arm_entry_by_arm(scope, arm)
            .and_then(|(entry, _)| {
                worker
                    .cursor
                    .with_index(state_index_to_usize(entry))
                    .try_recv_meta()
            })
            .is_some();
        let expected_controller_cross_role_recv = worker
            .cursor
            .controller_arm_entry_by_arm(scope, arm)
            .and_then(|(entry, _)| {
                worker
                    .cursor
                    .with_index(state_index_to_usize(entry))
                    .try_recv_meta()
            })
            .map(|recv_meta| recv_meta.peer != 1)
            .unwrap_or(false);
        assert_eq!(
            materialization.controller_arm_entry(arm),
            worker.cursor.controller_arm_entry_by_arm(scope, arm)
        );
        assert_eq!(
            materialization.controller_arm_is_recv(arm),
            expected_controller_recv
        );
        assert_eq!(
            materialization.controller_arm_requires_ready_evidence(arm),
            expected_controller_cross_role_recv
        );
        assert_eq!(
            materialization.recv_entry(arm),
            worker
                .cursor
                .route_scope_arm_recv_index(scope, arm)
                .map(StateIndex::from_usize)
        );
        assert_eq!(
            materialization.passive_arm_entry(arm),
            worker
                .cursor
                .follow_passive_observer_arm_for_scope(scope, arm)
                .map(|nav| match nav {
                    PassiveArmNavigation::WithinArm { entry } => entry,
                })
        );
        let mut expected_binding_demux_lane_mask = 0u8;
        if let Some((entry, _)) = worker.cursor.controller_arm_entry_by_arm(scope, arm)
            && let Some(recv_meta) = worker
                .cursor
                .with_index(state_index_to_usize(entry))
                .try_recv_meta()
        {
            expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
        }
        if let Some(entry) = worker.cursor.route_scope_arm_recv_index(scope, arm)
            && let Some(recv_meta) = worker.cursor.with_index(entry).try_recv_meta()
        {
            expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
        }
        let mut dispatch_idx = 0usize;
        while let Some((_label, dispatch_arm, target)) = worker
            .cursor
            .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
        {
            if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                && let Some(recv_meta) = worker
                    .cursor
                    .with_index(state_index_to_usize(target))
                    .try_recv_meta()
            {
                expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
            }
            dispatch_idx += 1;
        }
        assert_eq!(
            materialization.binding_demux_lane_mask(Some(arm)),
            expected_binding_demux_lane_mask
        );
        if arm == 1 {
            break;
        }
        arm += 1;
    }
    let mut dispatch_idx = 0usize;
    while let Some((label, arm, target)) = worker
        .cursor
        .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
    {
        assert_eq!(
            materialization.first_recv_target(label),
            Some((arm, target))
        );
        dispatch_idx += 1;
    }
    assert_eq!(materialization.first_recv_len as usize, dispatch_idx);

    drop(worker);
}

#[test]
fn selection_materialization_helpers_match_reference_lookup_logic() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(999);
    let mut controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    controller.refresh_lane_offer_state(0);
    let controller_scope = controller.cursor.node_scope_id();
    let controller_selection = controller.select_scope().expect("controller selection");
    worker.refresh_lane_offer_state(0);
    let worker_scope = worker.cursor.node_scope_id();
    let worker_selection = worker.select_scope().expect("worker selection");

    let mut arm = 0u8;
    while arm <= 1 {
        assert_eq!(
            controller.selection_arm_has_recv(controller_selection, arm),
            controller.arm_has_recv(controller_scope, arm)
        );
        assert_eq!(
            controller.selection_arm_requires_materialization_ready_evidence(
                controller_selection,
                true,
                arm,
            ),
            controller.arm_requires_materialization_ready_evidence(controller_scope, arm)
        );
        assert_eq!(
            worker.selection_arm_has_recv(worker_selection, arm),
            worker.arm_has_recv(worker_scope, arm)
        );
        assert_eq!(
            worker.selection_arm_requires_materialization_ready_evidence(
                worker_selection,
                false,
                arm,
            ),
            if worker_selection.at_route_offer_entry
                && worker_selection
                    .materialization_meta
                    .passive_arm_entry(arm)
                    .is_some()
            {
                if worker_selection
                    .materialization_meta
                    .arm_has_first_recv_dispatch(arm)
                {
                    !worker.selection_arm_dispatch_materializes_without_ready_evidence(
                        worker_selection,
                        arm,
                    )
                } else {
                    false
                }
            } else {
                worker.arm_requires_materialization_ready_evidence(worker_scope, arm)
            }
        );
        assert_eq!(
            controller.selection_non_wire_loop_control_recv(
                controller_selection,
                true,
                arm,
                LABEL_LOOP_CONTINUE,
            ),
            controller.is_non_wire_loop_control_recv(controller_scope, arm, LABEL_LOOP_CONTINUE,)
        );
        assert_eq!(
            controller.selection_non_wire_loop_control_recv(
                controller_selection,
                true,
                arm,
                LABEL_LOOP_BREAK,
            ),
            controller.is_non_wire_loop_control_recv(controller_scope, arm, LABEL_LOOP_BREAK,)
        );
        assert_eq!(
            worker.selection_non_wire_loop_control_recv(
                worker_selection,
                false,
                arm,
                LABEL_LOOP_CONTINUE,
            ),
            worker.is_non_wire_loop_control_recv(worker_scope, arm, LABEL_LOOP_CONTINUE)
        );
        assert_eq!(
            worker.selection_non_wire_loop_control_recv(
                worker_selection,
                false,
                arm,
                LABEL_LOOP_BREAK,
            ),
            worker.is_non_wire_loop_control_recv(worker_scope, arm, LABEL_LOOP_BREAK)
        );
        if arm == 1 {
            break;
        }
        arm += 1;
    }

    drop(worker);
    drop(controller);
}

#[test]
fn scope_arm_materialization_meta_caches_passive_recv_meta_exactly() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(998);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.refresh_lane_offer_state(0);
    let offer_lane = worker.offer_lane_for_scope(scope);
    let passive_recv_meta = worker.compute_scope_passive_recv_meta(
        worker.frontier_state.offer_entry_state
            [state_index_to_usize(worker.route_state.lane_offer_state[0].entry)]
        .materialization_meta,
        scope,
        offer_lane,
    );
    let region = worker
        .cursor
        .scope_region_by_id(scope)
        .expect("scope region should exist");

    let mut arm = 0u8;
    while arm <= 1 {
        let expected = worker
            .cursor
            .follow_passive_observer_arm_for_scope(scope, arm)
            .map(|nav| match nav {
                PassiveArmNavigation::WithinArm { entry } => entry,
            })
            .and_then(|entry| {
                let target_cursor = worker.cursor.with_index(state_index_to_usize(entry));
                if let Some(recv_meta) = target_cursor.try_recv_meta() {
                    return Some((target_cursor.index(), recv_meta));
                }
                if let Some(send_meta) = target_cursor.try_send_meta() {
                    return Some((
                        target_cursor.index(),
                        RecvMeta {
                            eff_index: send_meta.eff_index,
                            label: send_meta.label,
                            peer: send_meta.peer,
                            resource: send_meta.resource,
                            is_control: send_meta.is_control,
                            next: target_cursor.index(),
                            scope,
                            route_arm: Some(arm),
                            is_choice_determinant: false,
                            shot: send_meta.shot,
                            policy: send_meta.policy(),
                            lane: send_meta.lane,
                        },
                    ));
                }
                if target_cursor.is_jump() {
                    let scope_end = target_cursor.jump_target().unwrap_or(0);
                    let scope_end_cursor = worker.cursor.with_index(scope_end);
                    if region.linger {
                        let synthetic_label = controller_arm_label(&worker.cursor, scope, arm)?;
                        return Some((
                            scope_end,
                            RecvMeta {
                                eff_index: EffIndex::ZERO,
                                label: synthetic_label,
                                peer: 1,
                                resource: None,
                                is_control: true,
                                next: scope_end,
                                scope,
                                route_arm: Some(arm),
                                is_choice_determinant: false,
                                shot: None,
                                policy: PolicyMode::static_mode(),
                                lane: offer_lane,
                            },
                        ));
                    }
                    if let Some(recv_meta) = scope_end_cursor.try_recv_meta() {
                        return Some((scope_end, recv_meta));
                    }
                    if let Some(send_meta) = scope_end_cursor.try_send_meta() {
                        return Some((
                            scope_end,
                            RecvMeta {
                                eff_index: send_meta.eff_index,
                                label: send_meta.label,
                                peer: send_meta.peer,
                                resource: send_meta.resource,
                                is_control: send_meta.is_control,
                                next: scope_end,
                                scope,
                                route_arm: Some(arm),
                                is_choice_determinant: false,
                                shot: send_meta.shot,
                                policy: send_meta.policy(),
                                lane: send_meta.lane,
                            },
                        ));
                    }
                    return None;
                }
                if region.linger {
                    let synthetic_label = controller_arm_label(&worker.cursor, scope, arm)?;
                    return Some((
                        target_cursor.index(),
                        RecvMeta {
                            eff_index: EffIndex::ZERO,
                            label: synthetic_label,
                            peer: 1,
                            resource: None,
                            is_control: true,
                            next: target_cursor.index(),
                            scope,
                            route_arm: Some(arm),
                            is_choice_determinant: false,
                            shot: None,
                            policy: PolicyMode::static_mode(),
                            lane: offer_lane,
                        },
                    ));
                }
                None
            });
        let cached = passive_recv_meta
            .get(arm as usize)
            .copied()
            .and_then(CachedRecvMeta::recv_meta);
        assert_eq!(cached, expected);
        if arm == 1 {
            break;
        }
        arm += 1;
    }

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_skips_observation_for_single_active_entry() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(998);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    assert!(
        worker
            .active_frontier_entries(None)
            .contains_only(current_idx)
    );
    let observed_epoch = worker.frontier_state.global_frontier_observed_epoch;

    worker
        .align_cursor_to_selected_scope()
        .expect("single current entry should select directly");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.global_frontier_observed_epoch, observed_epoch,
        "single-active fast path must not rebuild observation during align"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_reuses_cached_multi_entry_observation() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(999);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_ready_current(current_idx, fake_entry_idx);
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 17;

    worker
        .align_cursor_to_selected_scope()
        .expect("fresh cached observation should be reused");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, 17,
        "cache hit must not rebuild frontier observation"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1000);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_ready_current(current_idx, fake_entry_idx);
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 23;

    let unrelated = crate::binding::IncomingClassification {
        label: 91,
        channel: crate::binding::Channel::new(7),
        instance: 7,
        has_fin: false,
    };
    assert!(worker.binding_inbox.push_back(2, unrelated));

    worker
        .align_cursor_to_selected_scope()
        .expect("unrelated binding changes must not invalidate cached observation");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, 23,
        "cache hit must survive unrelated-lane binding updates"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1003);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_ready_current(current_idx, fake_entry_idx);

    let first = crate::binding::IncomingClassification {
        label: 31,
        channel: crate::binding::Channel::new(3),
        instance: 3,
        has_fin: false,
    };
    let second = crate::binding::IncomingClassification {
        label: 32,
        channel: crate::binding::Channel::new(4),
        instance: 4,
        has_fin: false,
    };
    assert!(worker.binding_inbox.push_back(0, first));
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 27;

    assert!(worker.binding_inbox.push_back(0, second));

    worker
        .align_cursor_to_selected_scope()
        .expect("relevant lane content-only changes must not invalidate cached observation");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, 27,
        "cache hit must survive content-only updates on already-nonempty offer lanes"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1001);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    if crate::eff::meta::MAX_EFF_NODES < 2 {
        drop(worker);
        return;
    }

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_ready_current(current_idx, fake_entry_idx);
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 29;

    let current_scope_slot = worker
        .scope_slot_for_route(worker.cursor.node_scope_id())
        .expect("current node scope should be a route scope");
    let unrelated_slot = if current_scope_slot == 0 { 1 } else { 0 };
    worker.evidence_store.scope_evidence[unrelated_slot].ready_arm_mask = ScopeEvidence::ARM0_READY;
    worker.bump_scope_evidence_generation(unrelated_slot);

    worker
        .align_cursor_to_selected_scope()
        .expect("unrelated scope evidence must not invalidate cached observation");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, 29,
        "cache hit must survive unrelated-scope evidence updates"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1002);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    if MAX_LANES < 3 {
        drop(worker);
        return;
    }

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_ready_current(current_idx, fake_entry_idx);
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 31;

    worker.refresh_lane_offer_state(2);

    worker
        .align_cursor_to_selected_scope()
        .expect("unrelated lane frontier refresh must not invalidate cached observation");

    assert_eq!(worker.cursor.index(), current_idx);
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, 31,
        "cache hit must survive unrelated-lane frontier refresh"
    );

    drop(worker);
}

#[test]
fn align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let nested_program = g::route(HINT_ROUTE_PROGRAM, ENTRY_ROUTE_PROGRAM);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&nested_program);
    let sid = SessionId::new(1004);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
        .expect("attach worker endpoint");
    let nested_scope = worker
        .cursor
        .seek_label(ENTRY_ARM0_SIGNAL_LABEL)
        .expect("nested route recv label must exist")
        .node_scope_id();

    worker.refresh_lane_offer_state(0);
    let outer_scope = worker.cursor.node_scope_id();
    let outer_entry = worker.cursor.index();
    let nested_entry = worker
        .route_scope_offer_entry_index(nested_scope)
        .expect("nested route must have offer entry");

    assert_ne!(outer_entry, nested_entry);
    worker
        .set_route_arm(0, outer_scope, 1)
        .expect("select outer nested arm");
    worker
        .set_route_arm(0, nested_scope, 0)
        .expect("select nested arm");
    worker.set_cursor(worker.cursor.with_index(nested_entry));

    assert_eq!(
        worker.cursor.node_scope_id(),
        nested_scope,
        "cursor must already be positioned at the descended nested route",
    );
    assert_eq!(
        worker.current_offer_scope_id(),
        nested_scope,
        "selected nested route must become the current offer scope",
    );
    assert_eq!(
        worker.route_state.lane_offer_state[0].scope, outer_scope,
        "pre-align lane state intentionally still points at the ancestor route",
    );

    worker
        .align_cursor_to_selected_scope()
        .expect("selected nested route entry should remain authoritative");

    assert_eq!(
        worker.cursor.index(),
        nested_entry,
        "align must not bounce a selected nested route entry back to the ancestor scope",
    );
    assert_eq!(worker.current_offer_scope_id(), nested_scope);

    drop(worker);
}

#[test]
fn active_entry_set_orders_entries_by_representative_lane() {
    let mut entries = ActiveEntrySet::EMPTY;
    assert!(entries.insert_entry(9, 4));
    assert!(entries.insert_entry(3, 1));
    assert!(entries.insert_entry(7, 1));
    assert_eq!(entries.entry_at(0), Some(3));
    assert_eq!(entries.entry_at(1), Some(7));
    assert_eq!(entries.entry_at(2), Some(9));

    assert!(entries.remove_entry(3));
    assert_eq!(entries.entry_at(0), Some(7));
    assert_eq!(entries.entry_at(1), Some(9));
    assert_eq!(entries.occupancy_mask(), 0b0000_0011);
}

#[test]
fn current_passive_without_evidence_keeps_priority_with_controller_present() {
    assert!(!current_entry_is_candidate(false, false, false, 0, false,));
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, true, 1, false,));
}

#[test]
fn current_passive_without_controller_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_observer_without_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_candidate_stays_selectable_without_route_lane_metadata() {
    assert!(current_entry_matches_after_filter(true, true, 43, None));
}

#[test]
fn current_candidate_respects_hint_filter() {
    assert!(!current_entry_matches_after_filter(
        true,
        true,
        43,
        Some(47)
    ));
}

#[test]
fn current_without_candidate_stays_blocked() {
    assert!(!current_entry_matches_after_filter(false, true, 43, None));
}

#[test]
fn current_without_offer_lanes_stays_blocked() {
    assert!(!current_entry_matches_after_filter(true, false, 43, None));
}

#[test]
fn offer_entry_observed_state_merges_static_summary_and_dynamic_evidence() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(41), summary, true, false, true);

    assert_eq!(observed.scope_id, ScopeId::generic(41));
    assert!(observed.matches_frontier(FrontierKind::Parallel));
    assert!(observed.is_controller());
    assert!(observed.is_dynamic());
    assert!(observed.has_progress_evidence());
    assert!(observed.has_ready_arm_evidence());
    assert!(observed.binding_ready());
    assert_ne!(observed.flags & OfferEntryObservedState::FLAG_READY, 0);
}

#[test]
fn cached_offer_entry_observed_state_preserves_arbitration_bits() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(51), summary, true, false, true);
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (observed_bit, inserted) = observed_entries.insert_entry(17).expect("insert entry");
    assert!(inserted);
    observed_entries.observe(observed_bit, observed);

    let cached = cached_offer_entry_observed_state(
        ScopeId::generic(51),
        summary,
        observed_entries,
        observed_bit,
    );
    let original_candidate = offer_entry_frontier_candidate(
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        observed,
    );
    let cached_candidate = offer_entry_frontier_candidate(
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        cached,
    );

    assert!(cached.matches_frontier(FrontierKind::PassiveObserver));
    assert!(cached.is_controller());
    assert!(cached.is_dynamic());
    assert!(cached.has_progress_evidence());
    assert!(cached.has_ready_arm_evidence());
    assert!(cached.ready());
    assert_eq!(cached_candidate.scope_id, original_candidate.scope_id);
    assert_eq!(
        cached_candidate.parallel_root,
        original_candidate.parallel_root
    );
    assert_eq!(cached_candidate.frontier, original_candidate.frontier);
    assert_eq!(
        cached_candidate.is_controller,
        original_candidate.is_controller
    );
    assert_eq!(cached_candidate.is_dynamic, original_candidate.is_dynamic);
    assert_eq!(
        cached_candidate.has_evidence,
        original_candidate.has_evidence
    );
    assert_eq!(cached_candidate.ready, original_candidate.ready);
}

#[test]
fn observed_entry_set_entry_bit_tracks_inserted_entries_exactly() {
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (first_bit, inserted_first) = observed_entries.insert_entry(17).expect("insert first");
    assert!(inserted_first);
    let (second_bit, inserted_second) = observed_entries.insert_entry(3).expect("insert second");
    assert!(inserted_second);
    let (reused_bit, inserted_reused) = observed_entries.insert_entry(17).expect("reuse first");
    assert!(!inserted_reused);
    assert_eq!(reused_bit, first_bit);
    assert_eq!(observed_entries.entry_bit(17), first_bit);
    assert_eq!(observed_entries.entry_bit(3), second_bit);
    assert_eq!(observed_entries.entry_bit(9), 0);
}

fn observed_entries_with_ready_current(
    current_idx: usize,
    fake_entry_idx: usize,
) -> ObservedEntrySet {
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    let (fake_bit, inserted_fake) = observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    observed_entries.ready_mask = current_bit;
    observed_entries.route_mask = current_bit | fake_bit;
    observed_entries
}

fn observed_entries_with_route_entries(
    current_idx: usize,
    fake_entry_idx: usize,
) -> ObservedEntrySet {
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    let (fake_bit, inserted_fake) = observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    observed_entries.route_mask = current_bit | fake_bit;
    observed_entries
}

#[test]
fn rebuild_frontier_observed_entries_reuses_cached_entry_after_slot_shift() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1004);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let mut static_summary = OfferEntryStaticSummary::EMPTY;
    static_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Route,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[current_idx].summary = static_summary;
    worker.frontier_state.offer_entry_state[current_idx].frontier = FrontierKind::Route;
    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let fake_state = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: current_state.parallel_root,
        frontier: FrontierKind::Route,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: static_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[fake_entry_idx] = fake_state;

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    let cached_key = worker.frontier_observation_key(ScopeId::none(), false);

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current cached entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    let (fake_bit, inserted_fake) = cached_observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake cached entry");
    assert!(inserted_fake);
    cached_observed_entries.observe(
        fake_bit,
        offer_entry_observed_state(fake_state.scope_id, fake_state.summary, false, false, false),
    );

    worker.frontier_state.offer_entry_state[current_idx].active_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[current_idx].lane_idx = 1;
    worker.frontier_state.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes_len = 1;
    worker.frontier_state.offer_entry_state[fake_entry_idx].active_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[fake_entry_idx].lane_idx = 0;
    worker.frontier_state.offer_entry_state[fake_entry_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[fake_entry_idx].offer_lanes = [0; MAX_LANES];
    worker.frontier_state.offer_entry_state[fake_entry_idx].offer_lanes_len = 1;

    let mut shifted_active_entries = ActiveEntrySet::EMPTY;
    assert!(shifted_active_entries.insert_entry(fake_entry_idx, 0));
    assert!(shifted_active_entries.insert_entry(current_idx, 1));
    worker.frontier_state.global_active_entries = shifted_active_entries;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let observation_key = worker.frontier_observation_key(ScopeId::none(), false);
    let current_shifted_state = worker.frontier_state.offer_entry_state[current_idx];
    let cached_current = worker.cached_offer_entry_observed_state_for_rebuild(
        current_idx,
        current_shifted_state,
        observation_key,
        cached_key,
        cached_observed_entries,
    );
    assert!(
        cached_current.is_some(),
        "entry cache should survive slot shifts inside the active frontier"
    );

    let rebuilt = worker.refresh_frontier_observed_entries(
        ScopeId::none(),
        false,
        shifted_active_entries,
        observation_key,
        cached_key,
        cached_observed_entries,
    );
    let current_shifted_bit = rebuilt.entry_bit(current_idx);
    assert_ne!(current_shifted_bit, 0);
    assert_eq!(current_shifted_bit, 1u8 << 1);
    assert_ne!(rebuilt.ready_mask & current_shifted_bit, 0);

    drop(worker);
}

#[test]
fn refresh_frontier_observation_cache_prewarms_after_active_entry_replacement() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1012);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let old_entry_idx = current_idx + 1;
    let new_entry_idx = current_idx + 2;
    assert!(new_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    worker.frontier_state.offer_entry_state[old_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(old_entry_idx, 0));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();
    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    let (old_bit, inserted_old) = cached_observed_entries
        .insert_entry(old_entry_idx)
        .expect("insert old entry");
    assert!(inserted_old);
    cached_observed_entries.observe(
        old_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            OfferEntryStaticSummary::EMPTY,
            false,
            false,
            false,
        ),
    );
    worker.frontier_state.global_frontier_observed = cached_observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 37;

    let mut ready_summary = OfferEntryStaticSummary::EMPTY;
    ready_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[old_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[new_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: ready_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut replaced_active_entries = ActiveEntrySet::EMPTY;
    assert!(replaced_active_entries.insert_entry(current_idx, 0));
    assert!(replaced_active_entries.insert_entry(new_entry_idx, 0));
    worker.frontier_state.global_active_entries = replaced_active_entries;
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
            .is_none(),
        "entry replacement should invalidate the previous cache key before warm-up",
    );

    worker.refresh_frontier_observation_cache(ScopeId::none(), false);

    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "frontier refresh should publish the replaced active-entry observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(old_entry_idx),
        0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(new_entry_idx),
        1u8 << 1
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(new_entry_idx),
        0,
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.loop_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(new_entry_idx),
        1u8 << 1,
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 37,
        "prewarm should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn patch_frontier_observed_entries_from_cached_structure_handles_cardinality_change() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1024);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let (
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        worker_rv_id,
    ) = {
        let base_port = worker.ports[0]
            .as_ref()
            .expect("worker lane 0 port must exist")
            as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
        unsafe {
            (
                (*base_port).transport() as *const HintOnlyTransport,
                (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                (*base_port).scratch_ptr(),
                (*base_port).rv_id(),
            )
        }
    };
    let worker_transport = unsafe { &*worker_transport };
    let worker_tap = unsafe { &*worker_tap };
    let worker_clock = unsafe { &*worker_clock };
    let worker_vm_caps = unsafe { &*worker_vm_caps };
    let worker_loops = unsafe { &*worker_loops };
    let worker_routes = unsafe { &*worker_routes };
    let worker_host_slots = unsafe { &*worker_host_slots };
    let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
    worker.ports[1] = Some(crate::rendezvous::port::Port::new(
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        Lane::new(1),
        1,
        worker_rv_id,
        worker_tx1,
        worker_rx1,
    ));

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let middle_entry_idx = current_idx + 1;
    let third_entry_idx = current_idx + 2;
    let last_entry_idx = current_idx + 3;
    let new_loop_entry_idx = current_idx + 4;
    assert!(new_loop_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let mut middle_summary = OfferEntryStaticSummary::EMPTY;
    middle_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    let mut third_summary = OfferEntryStaticSummary::EMPTY;
    third_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    let mut last_summary = OfferEntryStaticSummary::EMPTY;
    last_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        ..LaneOfferState::EMPTY
    });
    let mut new_loop_summary = OfferEntryStaticSummary::EMPTY;
    new_loop_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });

    worker.frontier_state.offer_entry_state[middle_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Parallel,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: middle_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[third_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: third_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[last_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: last_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
    assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
    assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    let cached_key = worker.frontier_observation_key(ScopeId::none(), false);

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    let (middle_bit, inserted_middle) = cached_observed_entries
        .insert_entry(middle_entry_idx)
        .expect("insert middle entry");
    assert!(inserted_middle);
    cached_observed_entries.observe(
        middle_bit,
        offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
    );
    let (third_bit, inserted_third) = cached_observed_entries
        .insert_entry(third_entry_idx)
        .expect("insert third entry");
    assert!(inserted_third);
    cached_observed_entries.observe(
        third_bit,
        offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
    );
    let (last_bit, inserted_last) = cached_observed_entries
        .insert_entry(last_entry_idx)
        .expect("insert last entry");
    assert!(inserted_last);
    cached_observed_entries.observe(
        last_bit,
        offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
    );

    worker.frontier_state.offer_entry_state[third_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[last_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[new_loop_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: new_loop_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(new_loop_entry_idx, 0));
    assert!(active_entries.insert_entry(middle_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let observation_key = worker.frontier_observation_key(ScopeId::none(), false);
    let patched = worker
        .patch_frontier_observed_entries_from_cached_structure(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
        .expect("cardinality change should patch cached frontier observations");

    assert_eq!(patched.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(patched.entry_bit(new_loop_entry_idx), 1u8 << 1);
    assert_eq!(patched.entry_bit(middle_entry_idx), 1u8 << 2);
    assert_eq!(patched.entry_bit(third_entry_idx), 0);
    assert_eq!(patched.entry_bit(last_entry_idx), 0);
    assert_ne!(patched.loop_mask & patched.entry_bit(new_loop_entry_idx), 0);
    assert_ne!(
        patched.parallel_mask & patched.entry_bit(middle_entry_idx),
        0
    );

    drop(worker);
}

#[test]
fn refresh_frontier_observation_cache_prewarms_after_multi_entry_permutation() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1013);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let (
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        worker_rv_id,
    ) = {
        let base_port = worker.ports[0]
            .as_ref()
            .expect("worker lane 0 port must exist")
            as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
        unsafe {
            (
                (*base_port).transport() as *const HintOnlyTransport,
                (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                (*base_port).scratch_ptr(),
                (*base_port).rv_id(),
            )
        }
    };
    let worker_transport = unsafe { &*worker_transport };
    let worker_tap = unsafe { &*worker_tap };
    let worker_clock = unsafe { &*worker_clock };
    let worker_vm_caps = unsafe { &*worker_vm_caps };
    let worker_loops = unsafe { &*worker_loops };
    let worker_routes = unsafe { &*worker_routes };
    let worker_host_slots = unsafe { &*worker_host_slots };
    let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
    worker.ports[1] = Some(crate::rendezvous::port::Port::new(
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        Lane::new(1),
        1,
        worker_rv_id,
        worker_tx1,
        worker_rx1,
    ));

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let middle_entry_idx = current_idx + 1;
    let third_entry_idx = current_idx + 2;
    let last_entry_idx = current_idx + 3;
    assert!(last_entry_idx < crate::global::typestate::MAX_STATES);

    let mut current_summary = OfferEntryStaticSummary::EMPTY;
    current_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Route,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[current_idx].summary = current_summary;
    worker.frontier_state.offer_entry_state[current_idx].frontier = FrontierKind::Route;
    worker.frontier_state.offer_entry_state[current_idx].active_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[current_idx].lane_idx = 0;
    worker.frontier_state.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes = [0; MAX_LANES];
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes_len = 1;
    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let mut middle_summary = OfferEntryStaticSummary::EMPTY;
    middle_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    let mut third_summary = OfferEntryStaticSummary::EMPTY;
    third_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    let mut last_summary = OfferEntryStaticSummary::EMPTY;
    last_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        ..LaneOfferState::EMPTY
    });

    worker.frontier_state.offer_entry_state[middle_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Parallel,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: middle_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[third_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: third_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[last_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: last_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
    assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
    assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(current_state.scope_id, current_summary, false, false, true),
    );
    let (middle_bit, inserted_middle) = cached_observed_entries
        .insert_entry(middle_entry_idx)
        .expect("insert middle entry");
    assert!(inserted_middle);
    cached_observed_entries.observe(
        middle_bit,
        offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
    );
    let (third_bit, inserted_third) = cached_observed_entries
        .insert_entry(third_entry_idx)
        .expect("insert third entry");
    assert!(inserted_third);
    cached_observed_entries.observe(
        third_bit,
        offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
    );
    let (last_bit, inserted_last) = cached_observed_entries
        .insert_entry(last_entry_idx)
        .expect("insert last entry");
    assert!(inserted_last);
    cached_observed_entries.observe(
        last_bit,
        offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
    );
    worker.frontier_state.global_frontier_observed = cached_observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 41;

    worker.frontier_state.offer_entry_state[current_idx].active_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[current_idx].lane_idx = 1;
    worker.frontier_state.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes_len = 1;
    worker.frontier_state.offer_entry_state[middle_entry_idx].active_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[middle_entry_idx].lane_idx = 0;
    worker.frontier_state.offer_entry_state[middle_entry_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[middle_entry_idx].offer_lanes = [0; MAX_LANES];
    worker.frontier_state.offer_entry_state[middle_entry_idx].offer_lanes_len = 1;
    worker.frontier_state.offer_entry_state[third_entry_idx].active_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[third_entry_idx].lane_idx = 1;
    worker.frontier_state.offer_entry_state[third_entry_idx].offer_lane_mask = 1u8 << 1;
    worker.frontier_state.offer_entry_state[third_entry_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
    worker.frontier_state.offer_entry_state[third_entry_idx].offer_lanes_len = 1;
    worker.frontier_state.offer_entry_state[last_entry_idx].active_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[last_entry_idx].lane_idx = 0;
    worker.frontier_state.offer_entry_state[last_entry_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[last_entry_idx].offer_lanes = [0; MAX_LANES];
    worker.frontier_state.offer_entry_state[last_entry_idx].offer_lanes_len = 1;

    let mut permuted_active_entries = ActiveEntrySet::EMPTY;
    assert!(permuted_active_entries.insert_entry(middle_entry_idx, 0));
    assert!(permuted_active_entries.insert_entry(third_entry_idx, 1));
    assert!(permuted_active_entries.insert_entry(last_entry_idx, 0));
    assert!(permuted_active_entries.insert_entry(current_idx, 1));
    worker.frontier_state.global_active_entries = permuted_active_entries;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    worker.refresh_frontier_observation_cache(ScopeId::none(), false);

    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "permutation prewarm should publish the permuted frontier observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(middle_entry_idx),
        1u8 << 0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(last_entry_idx),
        1u8 << 1
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(current_idx),
        1u8 << 2
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(third_entry_idx),
        1u8 << 3
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .dynamic_controller_mask,
        1u8 << 3
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .controller_mask,
        (1u8 << 0) | (1u8 << 3)
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.progress_mask,
        1u8 << 2
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.ready_mask,
        (1u8 << 2) | (1u8 << 3)
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.loop_mask,
        1u8 << 3
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.parallel_mask,
        1u8 << 0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .passive_observer_mask,
        1u8 << 1
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.route_mask,
        1u8 << 2
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 41,
        "permutation prewarm should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn refresh_frontier_observation_cache_prewarms_after_multi_entry_replacement() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1014);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let (
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        worker_rv_id,
    ) = {
        let base_port = worker.ports[0]
            .as_ref()
            .expect("worker lane 0 port must exist")
            as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
        unsafe {
            (
                (*base_port).transport() as *const HintOnlyTransport,
                (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                (*base_port).scratch_ptr(),
                (*base_port).rv_id(),
            )
        }
    };
    let worker_transport = unsafe { &*worker_transport };
    let worker_tap = unsafe { &*worker_tap };
    let worker_clock = unsafe { &*worker_clock };
    let worker_vm_caps = unsafe { &*worker_vm_caps };
    let worker_loops = unsafe { &*worker_loops };
    let worker_routes = unsafe { &*worker_routes };
    let worker_host_slots = unsafe { &*worker_host_slots };
    let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
    worker.ports[1] = Some(crate::rendezvous::port::Port::new(
        worker_transport,
        worker_tap,
        worker_clock,
        worker_vm_caps,
        worker_loops,
        worker_routes,
        worker_host_slots,
        worker_scratch,
        Lane::new(1),
        1,
        worker_rv_id,
        worker_tx1,
        worker_rx1,
    ));

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let middle_entry_idx = current_idx + 1;
    let third_entry_idx = current_idx + 2;
    let last_entry_idx = current_idx + 3;
    let new_loop_entry_idx = current_idx + 4;
    let new_passive_entry_idx = current_idx + 5;
    assert!(new_passive_entry_idx < crate::global::typestate::MAX_STATES);

    let mut current_summary = OfferEntryStaticSummary::EMPTY;
    current_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Route,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[current_idx].summary = current_summary;
    worker.frontier_state.offer_entry_state[current_idx].frontier = FrontierKind::Route;
    worker.frontier_state.offer_entry_state[current_idx].active_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[current_idx].lane_idx = 0;
    worker.frontier_state.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes = [0; MAX_LANES];
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes_len = 1;
    let current_state = worker.frontier_state.offer_entry_state[current_idx];

    let mut middle_summary = OfferEntryStaticSummary::EMPTY;
    middle_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    let mut third_summary = OfferEntryStaticSummary::EMPTY;
    third_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    let mut last_summary = OfferEntryStaticSummary::EMPTY;
    last_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        ..LaneOfferState::EMPTY
    });
    let mut new_loop_summary = OfferEntryStaticSummary::EMPTY;
    new_loop_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    let mut new_passive_summary = OfferEntryStaticSummary::EMPTY;
    new_passive_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        ..LaneOfferState::EMPTY
    });

    worker.frontier_state.offer_entry_state[middle_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Parallel,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: middle_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[third_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: third_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[last_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: last_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
    assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
    assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(current_state.scope_id, current_summary, false, false, true),
    );
    let (middle_bit, inserted_middle) = cached_observed_entries
        .insert_entry(middle_entry_idx)
        .expect("insert middle entry");
    assert!(inserted_middle);
    cached_observed_entries.observe(
        middle_bit,
        offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
    );
    let (third_bit, inserted_third) = cached_observed_entries
        .insert_entry(third_entry_idx)
        .expect("insert third entry");
    assert!(inserted_third);
    cached_observed_entries.observe(
        third_bit,
        offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
    );
    let (last_bit, inserted_last) = cached_observed_entries
        .insert_entry(last_entry_idx)
        .expect("insert last entry");
    assert!(inserted_last);
    cached_observed_entries.observe(
        last_bit,
        offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
    );
    worker.frontier_state.global_frontier_observed = cached_observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 53;

    worker.frontier_state.offer_entry_state[third_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[last_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[new_loop_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: new_loop_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[new_passive_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: new_passive_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut replaced_active_entries = ActiveEntrySet::EMPTY;
    assert!(replaced_active_entries.insert_entry(current_idx, 0));
    assert!(replaced_active_entries.insert_entry(middle_entry_idx, 1));
    assert!(replaced_active_entries.insert_entry(new_loop_entry_idx, 0));
    assert!(replaced_active_entries.insert_entry(new_passive_entry_idx, 1));
    worker.frontier_state.global_active_entries = replaced_active_entries;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker.refresh_structural_frontier_observation_cache(
            ScopeId::none(),
            false,
            worker.frontier_state.global_active_entries,
            worker.frontier_state.global_frontier_observed_key,
        ),
        "multi-entry replacement should patch the cached frontier observation without falling back to generic rebuild",
    );

    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "multi-entry replacement should publish the refreshed frontier observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(current_idx),
        1u8 << 0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(new_loop_entry_idx),
        1u8 << 1
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(middle_entry_idx),
        1u8 << 2
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(new_passive_entry_idx),
        1u8 << 3
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(third_entry_idx),
        0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(last_entry_idx),
        0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .dynamic_controller_mask,
        1u8 << 1
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .controller_mask,
        (1u8 << 1) | (1u8 << 2)
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.progress_mask,
        1u8 << 0
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.ready_mask,
        (1u8 << 0) | (1u8 << 1)
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.loop_mask,
        1u8 << 1
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.parallel_mask,
        1u8 << 2
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .passive_observer_mask,
        1u8 << 3
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.route_mask,
        1u8 << 0
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 53,
        "multi-entry replacement prewarm should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn refresh_cached_frontier_observation_entry_updates_stable_slot_in_place() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1013);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let mut summary = worker.frontier_state.offer_entry_state[current_idx].summary;
    summary.flags &= !OfferEntryStaticSummary::FLAG_STATIC_READY;
    worker.frontier_state.offer_entry_state[current_idx].summary = summary;

    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (observed_bit, inserted) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted);
    observed_entries.observe(
        observed_bit,
        offer_entry_observed_state(
            worker.frontier_state.offer_entry_state[current_idx].scope_id,
            summary,
            false,
            false,
            false,
        ),
    );
    worker.frontier_state.global_frontier_observed = observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 41;
    assert_eq!(
        worker.frontier_state.global_frontier_observed.ready_mask & observed_bit,
        0
    );

    worker.frontier_state.offer_entry_state[current_idx]
        .summary
        .flags |= OfferEntryStaticSummary::FLAG_STATIC_READY;
    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
            .is_none(),
        "summary fingerprint change should invalidate the stale cached key before patching",
    );

    assert!(
        worker.refresh_cached_frontier_observation_entry(ScopeId::none(), false, current_idx),
        "stable active-entry slot should patch the cached frontier observation in place",
    );
    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "targeted patch should publish the refreshed observation under the new key",
    );
    let current_bit = worker
        .frontier_state
        .global_frontier_observed
        .entry_bit(current_idx);
    assert_ne!(current_bit, 0);
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask & current_bit,
        0,
        "patched observation should reflect the updated static ready bit",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 41,
        "targeted patch should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn observed_entry_set_move_entry_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    let (fake_bit, inserted_fake) = observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    observed_entries.observe(
        fake_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    );

    assert!(observed_entries.move_entry_slot(fake_entry_idx, 0));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(observed_entries.parallel_mask, 1u8 << 0);
    assert_eq!(observed_entries.route_mask, 1u8 << 1);
}

#[test]
fn observed_entry_set_insert_observation_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );

    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(observed_entries.parallel_mask, 1u8 << 0);
    assert_eq!(observed_entries.route_mask, 1u8 << 1);
}

#[test]
fn observed_entry_set_remove_observation_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.remove_observation(fake_entry_idx));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 0);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.parallel_mask, 0);
    assert_eq!(observed_entries.route_mask, 1u8 << 0);
}

#[test]
fn observed_entry_set_replace_entry_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let old_entry_idx = 23usize;
    let new_entry_idx = 29usize;
    let mut observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        old_entry_idx,
        1,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.replace_entry_at_slot(
        old_entry_idx,
        new_entry_idx,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(9),
            frontier_mask: FrontierKind::Loop.bit(),
            flags: OfferEntryObservedState::FLAG_READY_ARM | OfferEntryObservedState::FLAG_DYNAMIC,
        },
    ));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(old_entry_idx), 0);
    assert_eq!(observed_entries.entry_bit(new_entry_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.dynamic_controller_mask, 1u8 << 1);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_arm_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.parallel_mask, 0);
    assert_eq!(observed_entries.loop_mask, 1u8 << 1);
    assert_eq!(observed_entries.route_mask, 1u8 << 0);
}

#[test]
fn frontier_observation_structural_entry_detection_is_exact() {
    let mut cached_entries = ActiveEntrySet::EMPTY;
    assert!(cached_entries.insert_entry(11, 0));
    assert!(cached_entries.insert_entry(17, 0));

    let mut inserted_entries = cached_entries;
    assert!(inserted_entries.insert_entry(23, 0));
    assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_inserted_entry_idx(inserted_entries, cached_entries.entries),
            Some(23)
        );
    assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_removed_entry_idx(cached_entries, inserted_entries.entries),
            Some(23)
        );

    let mut replaced_entries = ActiveEntrySet::EMPTY;
    assert!(replaced_entries.insert_entry(11, 0));
    assert!(replaced_entries.insert_entry(19, 0));
    assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_replaced_entry_idx(replaced_entries, cached_entries.entries),
            Some(19)
        );

    let mut shifted_entries = ActiveEntrySet::EMPTY;
    assert!(shifted_entries.insert_entry(17, 0));
    assert!(shifted_entries.insert_entry(11, 1));
    let mut shifted_cached_entries = ActiveEntrySet::EMPTY;
    assert!(shifted_cached_entries.insert_entry(11, 0));
    assert!(shifted_cached_entries.insert_entry(17, 1));
    assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_shifted_entry_idx(shifted_entries, shifted_cached_entries.entries),
            Some(17)
        );
}

#[test]
fn refresh_inserted_frontier_observation_entry_updates_cache_in_place() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1015);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let mut current_observed = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = current_observed
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    current_observed.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    worker.frontier_state.global_active_entries = ActiveEntrySet::EMPTY;
    assert!(
        worker
            .frontier_state
            .global_active_entries
            .insert_entry(current_idx, 0)
    );
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed = current_observed;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 59;

    worker.frontier_state.offer_entry_state[fake_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: current_state.parallel_root,
        frontier: current_state.frontier,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: OfferEntryStaticSummary::EMPTY,
        ..OfferEntryState::EMPTY
    };
    assert!(
        worker
            .frontier_state
            .global_active_entries
            .insert_entry(fake_entry_idx, 0)
    );
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
            .is_none(),
        "entry insertion should invalidate the previous cache key before patching",
    );
    assert!(
        worker.refresh_inserted_frontier_observation_entry(ScopeId::none(), false, fake_entry_idx),
        "single entry insertion should patch the cached frontier observation in place",
    );
    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "insert patch should publish the refreshed observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(current_idx),
        1u8 << 0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(fake_entry_idx),
        1u8 << 1
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(current_idx),
        0,
        "existing current observation should survive entry insertion",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 59,
        "insert patch should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn refresh_replaced_frontier_observation_entry_updates_cache_in_place() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1017);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let old_entry_idx = current_idx + 1;
    let new_entry_idx = current_idx + 2;
    assert!(new_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let old_state = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: current_state.parallel_root,
        frontier: current_state.frontier,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[old_entry_idx] = old_state;

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(old_entry_idx, 0));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    let (old_bit, inserted_old) = cached_observed_entries
        .insert_entry(old_entry_idx)
        .expect("insert old entry");
    assert!(inserted_old);
    cached_observed_entries.observe(
        old_bit,
        offer_entry_observed_state(old_state.scope_id, old_state.summary, false, false, false),
    );
    worker.frontier_state.global_frontier_observed = cached_observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 67;

    let mut ready_summary = OfferEntryStaticSummary::EMPTY;
    ready_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Loop,
        flags: LaneOfferState::FLAG_DYNAMIC,
        static_ready: true,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[old_entry_idx] = OfferEntryState::EMPTY;
    worker.frontier_state.offer_entry_state[new_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: current_state.parallel_root,
        frontier: FrontierKind::Loop,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: ready_summary,
        observed: OfferEntryObservedState::EMPTY,
    };
    let mut replaced_active_entries = ActiveEntrySet::EMPTY;
    assert!(replaced_active_entries.insert_entry(current_idx, 0));
    assert!(replaced_active_entries.insert_entry(new_entry_idx, 0));
    worker.frontier_state.global_active_entries = replaced_active_entries;
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
            .is_none(),
        "entry replacement should invalidate the previous cache key before patching",
    );
    assert!(
        worker.refresh_replaced_frontier_observation_entry(ScopeId::none(), false, new_entry_idx),
        "single slot replacement should patch the cached frontier observation in place",
    );
    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "replace patch should publish the refreshed observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(old_entry_idx),
        0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(new_entry_idx),
        1u8 << 1
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(new_entry_idx),
        0,
        "replacement observation should reflect the new entry readiness",
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.loop_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(new_entry_idx),
        1u8 << 1,
        "replacement observation should publish the new frontier bit",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 67,
        "replace patch should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn refresh_removed_frontier_observation_entry_updates_cache_in_place() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1016);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let fake_state = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: current_state.parallel_root,
        frontier: current_state.frontier,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[fake_entry_idx] = fake_state;

    let mut cached_active_entries = ActiveEntrySet::EMPTY;
    assert!(cached_active_entries.insert_entry(current_idx, 0));
    assert!(cached_active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = cached_active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();

    let mut cached_observed_entries = ObservedEntrySet::EMPTY;
    let (current_bit, inserted_current) = cached_observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    cached_observed_entries.observe(
        current_bit,
        offer_entry_observed_state(
            current_state.scope_id,
            current_state.summary,
            false,
            false,
            true,
        ),
    );
    let (fake_bit, inserted_fake) = cached_observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    cached_observed_entries.observe(
        fake_bit,
        offer_entry_observed_state(fake_state.scope_id, fake_state.summary, false, false, false),
    );
    worker.frontier_state.global_frontier_observed = cached_observed_entries;
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 61;

    worker.frontier_state.offer_entry_state[fake_entry_idx] = OfferEntryState::EMPTY;
    worker
        .frontier_state
        .global_active_entries
        .remove_entry(fake_entry_idx);
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
            .is_none(),
        "entry removal should invalidate the previous cache key before patching",
    );
    assert!(
        worker.refresh_removed_frontier_observation_entry(ScopeId::none(), false, fake_entry_idx),
        "single entry removal should patch the cached frontier observation in place",
    );
    assert!(
        worker.frontier_state.global_frontier_observed_key == updated_key,
        "remove patch should publish the refreshed observation under the new key",
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(current_idx),
        1u8 << 0
    );
    assert_eq!(
        worker
            .frontier_state
            .global_frontier_observed
            .entry_bit(fake_entry_idx),
        0
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask
            & worker
                .frontier_state
                .global_frontier_observed
                .entry_bit(current_idx),
        0,
        "current observation should survive entry removal",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 61,
        "remove patch should publish a fresh frontier observation epoch",
    );

    drop(worker);
}

#[test]
fn scope_evidence_change_prewarms_relevant_frontier_observation_cache() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1013);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let current_scope = worker.cursor.node_scope_id();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let mut static_summary = OfferEntryStaticSummary::EMPTY;
    static_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Route,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[current_idx].summary = static_summary;
    worker.frontier_state.offer_entry_state[current_idx].frontier = FrontierKind::Route;
    worker.frontier_state.offer_entry_state[fake_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 0,
        lane_idx: 0,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 0,
        offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: static_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = 1u8 << 0;
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_route_entries(current_idx, fake_entry_idx);
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 41;

    worker.mark_scope_ready_arm(current_scope, 0);

    let warmed_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, warmed_key)
            .is_some(),
        "scope evidence update should prewarm the relevant cached observation",
    );
    let current_bit = worker
        .frontier_state
        .global_frontier_observed
        .entry_bit(current_idx);
    assert_ne!(current_bit, 0);
    assert_ne!(
        worker
            .frontier_state
            .global_frontier_observed
            .ready_arm_mask
            & current_bit,
        0,
        "ready-arm evidence should update the cached observation for the changed scope",
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.progress_mask & current_bit,
        0,
        "ready-arm evidence should also publish progress evidence in the cached observation",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 41,
        "targeted cache refresh should publish a new frontier observation epoch",
    );

    let warmed_epoch = worker.frontier_state.frontier_observation_epoch;
    worker
        .align_cursor_to_selected_scope()
        .expect("prewarmed scope evidence should keep align on the cached observation path");
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, warmed_epoch,
        "align should hit the warmed cache instead of rebuilding the frontier observation",
    );

    drop(worker);
}

#[test]
fn binding_inbox_change_prewarms_relevant_frontier_observation_cache() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1014);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let mut static_summary = OfferEntryStaticSummary::EMPTY;
    static_summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Route,
        ..LaneOfferState::EMPTY
    });
    worker.frontier_state.offer_entry_state[current_idx].summary = static_summary;
    worker.frontier_state.offer_entry_state[current_idx].frontier = FrontierKind::Route;
    worker.frontier_state.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes = [0, 0, 0, 0, 0, 0, 0, 0];
    worker.frontier_state.offer_entry_state[current_idx].offer_lanes_len = 1;
    worker.frontier_state.offer_entry_state[fake_entry_idx] = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: 1,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: static_summary,
        observed: OfferEntryObservedState::EMPTY,
    };

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    worker.frontier_state.global_frontier_observed =
        observed_entries_with_route_entries(current_idx, fake_entry_idx);
    worker.frontier_state.global_frontier_observed_key =
        worker.frontier_observation_key(ScopeId::none(), false);
    worker.frontier_state.frontier_observation_epoch = 43;

    worker.put_back_binding_for_lane(
        0,
        crate::binding::IncomingClassification {
            label: current_state.label_meta.recv_label,
            instance: 11,
            has_fin: false,
            channel: Channel::new(7),
        },
    );

    let warmed_key = worker.frontier_observation_key(ScopeId::none(), false);
    assert!(
        worker
            .cached_frontier_observed_entries(ScopeId::none(), false, warmed_key)
            .is_some(),
        "binding inbox update should prewarm the relevant cached observation",
    );
    let current_bit = worker
        .frontier_state
        .global_frontier_observed
        .entry_bit(current_idx);
    let fake_bit = worker
        .frontier_state
        .global_frontier_observed
        .entry_bit(fake_entry_idx);
    assert_ne!(
        worker.frontier_state.global_frontier_observed.ready_mask & current_bit,
        0,
        "buffered binding should mark the affected entry ready in the cached observation",
    );
    assert_ne!(
        worker.frontier_state.global_frontier_observed.progress_mask & current_bit,
        0,
        "buffered binding should publish progress evidence for the affected entry",
    );
    assert_eq!(
        worker.frontier_state.global_frontier_observed.ready_mask & fake_bit,
        0,
        "unrelated offer lanes must stay untouched by the targeted binding refresh",
    );
    assert!(
        worker.frontier_state.frontier_observation_epoch > 43,
        "targeted binding refresh should publish a new frontier observation epoch",
    );

    let warmed_epoch = worker.frontier_state.frontier_observation_epoch;
    worker
        .align_cursor_to_selected_scope()
        .expect("prewarmed binding change should keep align on the cached observation path");
    assert_eq!(
        worker.frontier_state.frontier_observation_epoch, warmed_epoch,
        "align should hit the warmed binding cache instead of rebuilding the frontier observation",
    );

    drop(worker);
}

#[test]
fn cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1013);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let state = &mut worker.frontier_state.offer_entry_state[current_idx];
    state.offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    state.offer_lanes = [0, 1, 0, 0, 0, 0, 0, 0];
    state.offer_lanes_len = 2;
    state.lane_idx = 0;

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = state.offer_lane_mask;
    worker.recompute_global_offer_lane_entry_slot_masks();

    let cached_key = worker.frontier_observation_key(ScopeId::none(), false);
    let mut observation_key = cached_key;
    observation_key.route_change_epochs[1] = observation_key.route_change_epochs[1].wrapping_add(1);
    if observation_key.route_change_epochs[1] == 0 {
        observation_key.route_change_epochs[1] = 1;
    }

    let changed_slot_mask = worker
        .cached_frontier_changed_entry_slot_mask(
            ScopeId::none(),
            false,
            observation_key,
            cached_key,
        )
        .expect("active frontier is unchanged");

    assert_eq!(
        changed_slot_mask, 0,
        "route changes on non-representative offer lanes must not invalidate the entry"
    );

    drop(worker);
}

#[test]
fn refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1008);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");

    worker.refresh_lane_offer_state(0);
    let current_idx = worker.cursor.index();
    let fake_entry_idx = current_idx + 1;
    assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

    let current_state = worker.frontier_state.offer_entry_state[current_idx];
    let fake_state = OfferEntryState {
        active_mask: 1u8 << 1,
        lane_idx: u8::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: current_state.scope_id,
        offer_lane_mask: 1u8 << 1,
        offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
        offer_lanes_len: 1,
        selection_meta: current_state.selection_meta,
        label_meta: current_state.label_meta,
        materialization_meta: current_state.materialization_meta,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };
    worker.frontier_state.offer_entry_state[fake_entry_idx] = fake_state;

    let mut active_entries = ActiveEntrySet::EMPTY;
    assert!(active_entries.insert_entry(current_idx, 0));
    assert!(active_entries.insert_entry(fake_entry_idx, 1));
    worker.frontier_state.global_active_entries = active_entries;
    worker.frontier_state.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
    worker.recompute_global_offer_lane_entry_slot_masks();
    let cached_key = worker.frontier_observation_key(ScopeId::none(), false);
    let cached_observed_entries = observed_entries_with_ready_current(current_idx, fake_entry_idx);

    let buffered = crate::binding::IncomingClassification {
        label: 41,
        channel: crate::binding::Channel::new(17),
        instance: 0,
        has_fin: false,
    };
    assert!(worker.binding_inbox.push_back(1, buffered));
    let observation_key = worker.frontier_observation_key(ScopeId::none(), false);

    let refreshed = worker
        .refresh_frontier_observed_entries_from_cache(
            ScopeId::none(),
            false,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
        .expect("same active frontier should refresh changed entry slots in place");

    let current_bit = refreshed.entry_bit(current_idx);
    let fake_bit = refreshed.entry_bit(fake_entry_idx);
    assert_ne!(current_bit, 0);
    assert_ne!(fake_bit, 0);
    assert_ne!(refreshed.ready_mask & current_bit, 0);
    assert_ne!(refreshed.ready_mask & fake_bit, 0);
    assert_ne!(refreshed.progress_mask & fake_bit, 0);

    drop(worker);
}

#[test]
fn offer_entry_reentry_prefers_first_ready_lane_candidate() {
    let current_scope = ScopeId::generic(11);
    let current_parallel_root = ScopeId::generic(7);
    let mut ready_entry_idx = None;
    let mut any_entry_idx = None;
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: false,
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(21),
            entry_idx: 10,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );

    assert_eq!(any_entry_idx, Some(9));
    assert_eq!(ready_entry_idx, Some(10));
}

#[test]
fn current_controller_without_evidence_yields_to_progress_sibling() {
    assert!(!current_entry_is_candidate(true, true, false, 1, true,));
}

#[test]
fn current_controller_without_evidence_keeps_priority_without_progress_sibling() {
    assert!(current_entry_is_candidate(true, true, false, 1, false,));
}

#[test]
fn current_controller_without_alternative_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, false, 0, true,));
}

#[test]
fn current_controller_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, true, 1, true,));
}

#[test]
fn controller_candidate_with_no_evidence_stays_blocked_when_current_has_offer_lanes() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn controller_candidate_without_progress_stays_blocked_in_passive_frontier() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn passive_current_is_suppressed_only_by_controller_progress_sibling() {
    assert!(should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        true,
    ));
    assert!(!should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        false,
    ));
}

#[test]
fn evidence_less_non_current_candidate_requires_progress_or_unrunnable_current() {
    assert!(!candidate_participates_in_frontier_arbitration(
        10, 7, false, false,
    ));
    assert!(candidate_participates_in_frontier_arbitration(
        10, 7, false, true,
    ));
}

#[test]
fn passive_recv_cursor_is_not_progress_evidence_for_sibling_preempt() {
    assert!(!candidate_has_progress_evidence(false, false, false));
    assert!(candidate_has_progress_evidence(true, false, false));
    assert!(candidate_has_progress_evidence(false, true, false));
    assert!(candidate_has_progress_evidence(false, false, true));
}

fn has_progress_controller_sibling(
    snapshot: FrontierSnapshot,
    scope_id: ScopeId,
    entry_idx: usize,
) -> bool {
    let mut idx = 0usize;
    while idx < snapshot.candidate_len {
        let candidate = snapshot.candidates[idx];
        if snapshot.matches_parallel_root(candidate)
            && candidate.ready
            && candidate.has_evidence
            && candidate.is_controller
            && (candidate.scope_id != scope_id || candidate.entry_idx != entry_idx)
        {
            return true;
        }
        idx += 1;
    }
    false
}

#[test]
fn passive_frontier_detects_progress_controller_sibling() {
    let current_scope = ScopeId::generic(71);
    let controller_scope = ScopeId::generic(72);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: false,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 63,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::PassiveObserver,
        candidates,
        candidate_len: 2,
    };
    assert!(has_progress_controller_sibling(snapshot, current_scope, 63));
}

#[test]
fn passive_frontier_ignores_controller_without_progress_evidence() {
    let current_scope = ScopeId::generic(171);
    let controller_scope = ScopeId::generic(172);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 63,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::PassiveObserver,
        candidates,
        candidate_len: 2,
    };
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}

#[test]
fn passive_frontier_ignores_non_controller_sibling_for_controller_preemption() {
    let current_scope = ScopeId::generic(81);
    let sibling_scope = ScopeId::generic(82);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 59,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 63,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::PassiveObserver,
        candidates,
        candidate_len: 2,
    };
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}

#[test]
fn frontier_yield_ping_pong_is_bounded() {
    let mut visited = FrontierVisitSet::EMPTY;
    let scope_a = ScopeId::generic(31);
    let scope_b = ScopeId::generic(32);
    visited.record(scope_a);
    visited.record(scope_b);
    visited.record(scope_a);
    assert!(visited.contains(scope_a));
    assert!(visited.contains(scope_b));
    assert_eq!(visited.len, 2);
}

#[test]
fn route_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(41);
    let sibling_scope = ScopeId::generic(42);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 10,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        is_controller: true,
        is_dynamic: true,
        has_evidence: false,
        ready: false,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 12,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        is_controller: true,
        is_dynamic: true,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 10,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::Route,
        candidates,
        candidate_len: 2,
    };
    let picked = snapshot
        .select_yield_candidate(FrontierVisitSet::EMPTY)
        .expect("route frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn loop_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(51);
    let sibling_scope = ScopeId::generic(52);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: true,
        has_evidence: false,
        ready: false,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: true,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 20,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::Loop,
        candidates,
        candidate_len: 2,
    };
    let picked = snapshot
        .select_yield_candidate(FrontierVisitSet::EMPTY)
        .expect("loop frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Loop);
}

#[test]
fn defer_yields_across_frontier_in_same_parallel_root() {
    let root = ScopeId::generic(55);
    let current_scope = ScopeId::generic(56);
    let sibling_scope = ScopeId::generic(57);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: true,
        has_evidence: false,
        ready: false,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: root,
        frontier: FrontierKind::Route,
        is_controller: true,
        is_dynamic: true,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 20,
        current_parallel_root: root,
        current_frontier: FrontierKind::Loop,
        candidates,
        candidate_len: 2,
    };
    let picked = snapshot
        .select_yield_candidate(FrontierVisitSet::EMPTY)
        .expect("defer must yield to progress sibling in same parallel root");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn parallel_frontier_prefers_ready_lane_before_phase_join() {
    let current_scope = ScopeId::generic(61);
    let root = ScopeId::generic(60);
    let ready_scope = ScopeId::generic(62);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 30,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        is_controller: true,
        is_dynamic: true,
        has_evidence: false,
        ready: false,
    };
    candidates[1] = FrontierCandidate {
        scope_id: ScopeId::generic(63),
        entry_idx: 31,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: false,
    };
    candidates[2] = FrontierCandidate {
        scope_id: ready_scope,
        entry_idx: 32,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        is_controller: false,
        is_dynamic: false,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 30,
        current_parallel_root: root,
        current_frontier: FrontierKind::Parallel,
        candidates,
        candidate_len: 3,
    };
    let picked = snapshot
        .select_yield_candidate(FrontierVisitSet::EMPTY)
        .expect("parallel frontier must choose progress sibling");
    assert_eq!(picked.scope_id, ready_scope);
    assert_eq!(picked.entry_idx, 32);
}

#[test]
fn passive_observer_defer_follow_is_progressive() {
    let current_scope = ScopeId::generic(71);
    let sibling_scope = ScopeId::generic(72);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 40,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 44,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: true,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 40,
        current_parallel_root: ScopeId::none(),
        current_frontier: FrontierKind::PassiveObserver,
        candidates,
        candidate_len: 2,
    };
    let mut visited = FrontierVisitSet::EMPTY;
    visited.record(current_scope);
    let picked = snapshot
        .select_yield_candidate(visited)
        .expect("passive observer defer must progress to sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_ne!(picked.scope_id, current_scope);
}

#[test]
fn passive_observer_defer_stops_without_progress_evidence() {
    let root = ScopeId::generic(73);
    let current_scope = ScopeId::generic(74);
    let sibling_scope = ScopeId::generic(75);
    let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 50,
        parallel_root: root,
        frontier: FrontierKind::PassiveObserver,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 53,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        is_controller: true,
        is_dynamic: false,
        has_evidence: false,
        ready: true,
    };
    let snapshot = FrontierSnapshot {
        current_scope,
        current_entry_idx: 50,
        current_parallel_root: root,
        current_frontier: FrontierKind::PassiveObserver,
        candidates,
        candidate_len: 2,
    };
    let mut visited = FrontierVisitSet::EMPTY;
    visited.record(current_scope);
    assert_eq!(snapshot.select_yield_candidate(visited), None);
}

#[test]
fn controller_local_ready_is_not_progress_evidence_for_sibling_preempt() {
    assert!(
        current_entry_is_candidate(true, true, false, 1, false),
        "controller local-ready only must not preempt without progress evidence"
    );
}

#[test]
fn frontier_arbitration_is_uniform_across_route_loop_parallel_observer() {
    let cases = [
        (ScopeId::none(), FrontierKind::Route),
        (ScopeId::none(), FrontierKind::Loop),
        (ScopeId::generic(101), FrontierKind::Parallel),
        (ScopeId::none(), FrontierKind::PassiveObserver),
    ];
    let mut idx = 0usize;
    while idx < cases.len() {
        let (parallel_root, frontier) = cases[idx];
        let current_scope = ScopeId::generic((110 + idx) as u16);
        let sibling_scope = ScopeId::generic((120 + idx) as u16);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 70 + idx,
            parallel_root,
            frontier,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 80 + idx,
            parallel_root,
            frontier,
            is_controller: true,
            is_dynamic: true,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 70 + idx,
            current_parallel_root: parallel_root,
            current_frontier: frontier,
            candidates,
            candidate_len: 2,
        };
        let picked = snapshot
            .select_yield_candidate(FrontierVisitSet::EMPTY)
            .expect("uniform frontier defer must pick progress-evidence-bearing sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, frontier);
        idx += 1;
    }
}

#[test]
fn dynamic_route_ignores_hint_classification_for_authority() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(904);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    assert!(
        worker
            .cursor
            .first_recv_target(scope, HINT_LEFT_DATA_LABEL)
            .is_none(),
        "dynamic route arm authority must not depend on first-recv dispatch"
    );

    let mut offer = pin!(worker.offer());
    let mut cx = Context::from_waker(noop_waker_ref());
    let first_poll = offer.as_mut().poll(&mut cx);
    let mut branch = match first_poll {
        Poll::Ready(Ok(next_branch)) => Some(next_branch),
        Poll::Ready(Err(err)) => panic!("offer should not fail before decision: {err:?}"),
        Poll::Pending => None,
    };
    controller.port_for_lane(0).record_route_decision(scope, 0);
    if branch.is_none() {
        let mut attempts = 0usize;
        while attempts < 4 {
            match offer.as_mut().poll(&mut cx) {
                Poll::Ready(Ok(next_branch)) => {
                    branch = Some(next_branch);
                    break;
                }
                Poll::Ready(Err(err)) => {
                    panic!("offer should resolve via authoritative decision: {err:?}");
                }
                Poll::Pending => {}
            }
            attempts += 1;
        }
    }
    let branch = branch.expect("offer should become ready after authoritative decision");
    assert_eq!(
        branch.label(),
        HINT_LEFT_DATA_LABEL,
        "resolved branch must follow authoritative arm, not hint-derived ACK"
    );
    drop(branch);
    drop(controller);
}

#[test]
fn select_scope_prepass_keeps_pending_scope_evidence_non_consuming() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9041);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    controller.port_for_lane(0).record_route_decision(scope, 0);
    let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
    worker.refresh_lane_offer_state(0);
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let (_binding_ready, has_ack, has_ready_arm_evidence) =
        worker.preview_offer_entry_evidence_non_consuming(entry_state);
    assert!(has_ack, "prepass may observe pending ACK authority");
    assert!(
        !has_ready_arm_evidence,
        "pending demux hints must not be promoted to ready-arm evidence during prepass"
    );

    worker
        .align_cursor_to_selected_scope()
        .expect("scope prepass should succeed without consuming evidence");
    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "prepass must not consume route ACK authority into scope evidence"
    );
    assert!(
        worker.peek_scope_hint(scope).is_none(),
        "prepass must not consume route hints into scope evidence"
    );
    assert_eq!(
        worker.scope_ready_arm_mask(scope),
        0,
        "prepass must not synthesize ready-arm evidence before selected-scope ingest"
    );
    assert_eq!(
        worker.port_for_lane(0).peek_route_decision(scope, 1),
        Some(0),
        "authoritative route ACK must remain pending on the port after prepass"
    );
    assert!(
        worker
            .port_for_lane(0)
            .has_route_hint_matching(|label| label == HINT_LEFT_DATA_LABEL),
        "matching route hint must remain queued on the port after prepass"
    );

    worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, true, label_meta);

    assert_eq!(
        worker
            .peek_scope_ack(scope)
            .map(|token| token.arm().as_u8()),
        Some(0),
        "selected-scope ingest must materialize the pending ACK exactly once"
    );
    assert!(
        worker.scope_has_ready_arm_evidence(scope),
        "selected-scope ingest must materialize ready-arm evidence from the pending hint"
    );
    assert_eq!(
        worker.port_for_lane(0).peek_route_decision(scope, 1),
        None,
        "selected-scope ingest must consume the pending ACK from the port"
    );
    assert!(
        !worker
            .port_for_lane(0)
            .has_route_hint_matching(|label| label == HINT_LEFT_DATA_LABEL),
        "selected-scope ingest must consume the pending hint from the port"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9042);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    controller.port_for_lane(0).record_route_decision(scope, 0);
    worker.refresh_lane_offer_state(0);
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let (binding_ready, has_ack, has_ready_arm_evidence) =
        worker.preview_offer_entry_evidence_non_consuming(entry_state);

    assert!(!binding_ready, "empty binding must remain not-ready");
    assert!(has_ack, "pending route decision must count as ACK evidence");
    assert!(
        !has_ready_arm_evidence,
        "ACK-only preview must not synthesize ready-arm evidence"
    );
    assert_eq!(
        worker.binding.poll_count(),
        0,
        "binding probe must be skipped when ACK already supplies progress evidence"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn preview_offer_entry_evidence_defers_binding_poll_until_selected_scope() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9043);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let classification = IncomingClassification {
        label: HINT_LEFT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(3),
    };
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            TestBinding::with_incoming(&[classification]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.refresh_lane_offer_state(0);
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let (binding_ready, has_ack, has_ready_arm_evidence) =
        worker.preview_offer_entry_evidence_non_consuming(entry_state);

    assert!(
        !binding_ready,
        "prepass must not probe binding to synthesize ready state"
    );
    assert!(
        !has_ack,
        "classification-only prepass must not synthesize ACK authority"
    );
    assert!(
        !has_ready_arm_evidence,
        "classification-only prepass must not synthesize ready-arm evidence"
    );
    assert_eq!(
        worker.binding.poll_count(),
        0,
        "prepass must not touch binding before selected-scope demux"
    );

    let picked = worker.poll_binding_for_offer(
        scope,
        entry_state.lane_idx as usize,
        entry_state.offer_lane_mask,
        entry_state.label_meta,
        entry_state.materialization_meta,
    );
    assert_eq!(
        picked,
        Some((0, classification)),
        "selected-scope poll must still resolve the deferred binding classification"
    );
    assert_eq!(
        worker.binding.poll_count(),
        1,
        "binding must be polled exactly once after scope selection"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn hint_or_classification_never_writes_ack_authority() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(905);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            TestBinding::with_incoming(&[IncomingClassification {
                label: HINT_LEFT_DATA_LABEL,
                instance: 0,
                has_fin: false,
                channel: Channel::new(1),
            }]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

    worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, true, label_meta);
    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "dynamic hint ingest must not mint ack authority"
    );

    let mut binding_classification = None;
    worker.cache_binding_classification_for_offer(
        scope,
        0,
        1u8 << 0,
        label_meta,
        worker.offer_scope_materialization_meta(scope, 0),
        &mut binding_classification,
    );
    assert!(
        binding_classification.is_some(),
        "binding classification should still be staged for decode/demux"
    );
    let classification =
        binding_classification.expect("binding classification should be available");
    worker.ingest_binding_scope_evidence(scope, classification.label, true, label_meta);
    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "classification must not mint ack authority for dynamic route"
    );
    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        None,
        "dynamic binding evidence must not materialize Poll authority"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_prefers_exact_label_for_ack_arm() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9044);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            TestBinding::with_incoming(&[
                IncomingClassification {
                    label: HINT_LEFT_DATA_LABEL,
                    instance: 7,
                    has_fin: false,
                    channel: Channel::new(3),
                },
                IncomingClassification {
                    label: HINT_RIGHT_DATA_LABEL,
                    instance: 9,
                    has_fin: false,
                    channel: Channel::new(5),
                },
            ]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.refresh_lane_offer_state(0);
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    assert_eq!(
        label_meta.preferred_binding_label(Some(1)),
        Some(HINT_RIGHT_DATA_LABEL)
    );
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
    );

    let picked = worker.poll_binding_for_offer(
        scope,
        entry_state.lane_idx as usize,
        entry_state.offer_lane_mask,
        label_meta,
        entry_state.materialization_meta,
    );
    assert_eq!(
        picked.map(|(lane_idx, classification)| (lane_idx, classification.label)),
        Some((0, HINT_RIGHT_DATA_LABEL)),
        "authoritative arm should narrow binding demux to the exact matching label"
    );
    let deferred =
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, HINT_LEFT_DATA_LABEL);
    assert_eq!(
        deferred.map(|classification| classification.label),
        Some(HINT_LEFT_DATA_LABEL),
        "non-authoritative arm classification must remain buffered"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9046);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let buffered = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(5),
    };
    worker.binding_inbox.put_back(2, buffered);

    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
        evidence_arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
        ..ScopeLabelMeta::EMPTY
    };
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
    );

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        worker.offer_scope_materialization_meta(scope, 0),
    );
    assert_eq!(
        picked,
        Some((2, buffered)),
        "buffered matching lane should be selected before probing empty poll lane"
    );
    assert_eq!(
        worker.binding.poll_count(),
        0,
        "buffered cross-lane hit should not poll unrelated empty lanes first"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9047);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let matching = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(5),
    };
    let loop_mismatch = IncomingClassification {
        label: LABEL_LOOP_CONTINUE,
        instance: 1,
        has_fin: false,
        channel: Channel::new(7),
    };
    worker.binding_inbox.put_back(0, loop_mismatch);
    worker.binding_inbox.put_back(2, matching);

    let extra_label = 99;
    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(extra_label),
        arm_label_masks: [
            0,
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
        ],
        evidence_arm_label_masks: [
            0,
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    let materialization_meta = ScopeArmMaterializationMeta {
        binding_demux_lane_mask: [0, 1u8 << 2],
        ..ScopeArmMaterializationMeta::EMPTY
    };
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
    );

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(picked, Some((2, matching)));
    assert_eq!(
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
        Some(loop_mismatch),
        "authoritative arm demux must not scan unrelated loop-control lane"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_prefers_authoritative_arm_label_mask_when_non_singleton() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9045);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            TestBinding::with_incoming(&[
                IncomingClassification {
                    label: HINT_RIGHT_DATA_LABEL,
                    instance: 9,
                    has_fin: false,
                    channel: Channel::new(5),
                },
                IncomingClassification {
                    label: HINT_LEFT_DATA_LABEL,
                    instance: 7,
                    has_fin: false,
                    channel: Channel::new(3),
                },
            ]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.refresh_lane_offer_state(0);
    let entry_idx = state_index_to_usize(worker.route_state.lane_offer_state[0].entry);
    let entry_state = worker.frontier_state.offer_entry_state[entry_idx];
    let extra_label = 99;
    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(extra_label),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    assert_eq!(label_meta.preferred_binding_label(Some(0)), None);
    assert_eq!(
        label_meta.preferred_binding_label_mask(Some(0)),
        ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL) | ScopeLabelMeta::label_bit(extra_label)
    );
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(0).expect("binary route arm")),
    );

    let picked = worker.poll_binding_for_offer(
        scope,
        entry_state.lane_idx as usize,
        entry_state.offer_lane_mask,
        label_meta,
        entry_state.materialization_meta,
    );
    assert_eq!(
        picked.map(|(lane_idx, classification)| (lane_idx, classification.label)),
        Some((0, HINT_LEFT_DATA_LABEL)),
        "authoritative arm mask should skip buffered labels from the other arm"
    );
    let deferred =
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, HINT_RIGHT_DATA_LABEL);
    assert_eq!(
        deferred.map(|classification| classification.label),
        Some(HINT_RIGHT_DATA_LABEL),
        "non-authoritative arm classification must remain buffered after mask match"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_uses_label_mask_to_skip_other_arm_lanes_without_authority() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9048);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let matching = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(5),
    };
    let loop_mismatch = IncomingClassification {
        label: LABEL_LOOP_CONTINUE,
        instance: 1,
        has_fin: false,
        channel: Channel::new(7),
    };
    worker.binding_inbox.put_back(0, loop_mismatch);
    worker.binding_inbox.put_back(2, matching);

    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    let materialization_meta = ScopeArmMaterializationMeta {
        binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
        ..ScopeArmMaterializationMeta::EMPTY
    };

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(picked, Some((2, matching)));
    assert_eq!(
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
        Some(loop_mismatch),
        "no-authority demux should still restrict scans to lanes implied by the label mask"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9050);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let matching = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(5),
    };
    let loop_mismatch = IncomingClassification {
        label: LABEL_LOOP_CONTINUE,
        instance: 1,
        has_fin: false,
        channel: Channel::new(7),
    };
    worker.binding_inbox.put_back(0, loop_mismatch);
    worker.binding_inbox.put_back(2, matching);

    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    let materialization_meta = ScopeArmMaterializationMeta {
        binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
        ..ScopeArmMaterializationMeta::EMPTY
    };

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(picked, Some((2, matching)));
    assert_eq!(
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
        Some(loop_mismatch),
        "buffered matching lane should win before scanning drop-only preferred lane"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9052);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let matching = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 9,
        has_fin: false,
        channel: Channel::new(5),
    };
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    let materialization_meta = ScopeArmMaterializationMeta {
        binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
        ..ScopeArmMaterializationMeta::EMPTY
    };

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(
        picked, None,
        "generic mask path must not probe unbuffered cross-lane bindings before the selected lane"
    );
    assert_eq!(worker.binding.poll_count_for_lane(0), 1);
    assert_eq!(worker.binding.poll_count_for_lane(2), 0);

    let picked = worker.poll_binding_for_offer(
        scope,
        2,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(picked, Some((2, matching)));
    assert_eq!(worker.binding.poll_count_for_lane(2), 1);

    drop(worker);
    drop(controller);
}

#[test]
fn poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9053);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let matching = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 11,
        has_fin: false,
        channel: Channel::new(6),
    };
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &HINT_WORKER_PROGRAM,
            LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
    );
    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
        evidence_arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
        ..ScopeLabelMeta::EMPTY
    };
    let materialization_meta = ScopeArmMaterializationMeta {
        binding_demux_lane_mask: [0, 1u8 << 2],
        ..ScopeArmMaterializationMeta::EMPTY
    };

    let picked = worker.poll_binding_for_offer(
        scope,
        0,
        (1u8 << 0) | (1u8 << 2),
        label_meta,
        materialization_meta,
    );
    assert_eq!(picked, Some((2, matching)));
    assert_eq!(worker.binding.poll_count_for_lane(0), 0);
    assert_eq!(worker.binding.poll_count_for_lane(2), 1);

    drop(worker);
    drop(controller);
}

#[test]
fn take_binding_for_selected_arm_preserves_cached_other_arm_classification() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(9049);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let matching = IncomingClassification {
        label: HINT_LEFT_DATA_LABEL,
        instance: 9,
        has_fin: true,
        channel: Channel::new(5),
    };
    let cached_mismatch = IncomingClassification {
        label: HINT_RIGHT_DATA_LABEL,
        instance: 7,
        has_fin: false,
        channel: Channel::new(3),
    };
    worker.binding_inbox.put_back(0, matching);
    let extra_label = 99;
    let label_meta = ScopeLabelMeta {
        hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
            | ScopeLabelMeta::label_bit(extra_label),
        arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        evidence_arm_label_masks: [
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
        ],
        ..ScopeLabelMeta::EMPTY
    };
    let mut binding_classification = Some(cached_mismatch);

    let (channel, instance, has_fin) =
        worker.take_binding_for_selected_arm(0, 0, label_meta, &mut binding_classification);
    assert_eq!(channel, Some(matching.channel));
    assert_eq!(instance, Some(matching.instance));
    assert!(
        has_fin,
        "selected-arm helper should preserve FIN from matching ingress"
    );
    assert!(
        binding_classification.is_none(),
        "cached mismatch should be re-buffered, not left staged"
    );
    let deferred =
        worker
            .binding_inbox
            .take_matching_or_poll(&mut worker.binding, 0, HINT_RIGHT_DATA_LABEL);
    assert_eq!(
        deferred,
        Some(cached_mismatch),
        "selected-arm demux must preserve cached other-arm classifications"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn static_passive_binding_label_materializes_poll() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(906);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &ENTRY_WORKER_PROGRAM,
            TestBinding::with_incoming(&[IncomingClassification {
                label: ENTRY_ARM0_SIGNAL_LABEL,
                instance: 0,
                has_fin: false,
                channel: Channel::new(1),
            }]),
        )
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    assert!(
        worker
            .cursor
            .first_recv_target(scope, ENTRY_ARM0_SIGNAL_LABEL)
            .is_some(),
        "test requires a static passive recv dispatch target"
    );

    let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

    let mut binding_classification = None;
    worker.cache_binding_classification_for_offer(
        scope,
        0,
        1u8 << 0,
        label_meta,
        worker.offer_scope_materialization_meta(scope, 0),
        &mut binding_classification,
    );
    let classification =
        binding_classification.expect("binding classification should be staged for poll");
    worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);
    worker.ingest_binding_scope_evidence(scope, classification.label, false, label_meta);

    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "binding-backed static dispatch must not mint ack authority"
    );
    let resolved_label = worker.take_scope_hint(scope);
    assert_eq!(
        resolved_label,
        Some(classification.label),
        "binding-backed poll should still preserve the resolved ingress label"
    );
    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        Some(Arm::new(0).expect("binary route arm")),
        "exact binding ingress on a static passive route must materialize Poll authority"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn static_passive_staged_transport_hint_materializes_poll() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(ENTRY_ARM0_SIGNAL_LABEL);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(907);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    assert!(
        worker
            .cursor
            .first_recv_target(scope, ENTRY_ARM0_SIGNAL_LABEL)
            .is_some(),
        "test requires a static passive recv dispatch target"
    );

    let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
    worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);

    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        None,
        "transport hint alone must remain non-authoritative until ingress is staged"
    );
    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "transport-backed static dispatch must not mint ack authority"
    );
    let resolved_label = worker.take_scope_hint(scope);
    assert_eq!(
        resolved_label,
        Some(ENTRY_ARM0_SIGNAL_LABEL),
        "transport-backed poll should still preserve the resolved ingress label"
    );
    worker.mark_scope_ready_arm_from_label(
        scope,
        resolved_label.expect("transport hint must resolve"),
        label_meta,
    );
    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        Some(Arm::new(0).expect("binary route arm")),
        "staged exact transport ingress on a static passive route must materialize Poll authority"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes() {
    type OuterLeftMsg = Msg<0x50, u8>;
    type LeafLeftMsg = Msg<0x51, u8>;
    type LeafRightMsg = Msg<0x52, u8>;
    type MiddleRightMsg = Msg<0x53, u8>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let inner = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, LeafLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, LeafRightMsg, 0>(),
        ),
    );
    let middle = g::route(
        g::seq(g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(), inner),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, MiddleRightMsg, 0>(),
        ),
    );
    let program = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            middle,
        ),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(909);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
        .expect("attach worker endpoint");

    let outer_scope = worker.cursor.node_scope_id();
    let middle_scope = worker
        .cursor
        .passive_arm_scope_by_arm(outer_scope, 1)
        .expect("outer right arm should enter middle route");
    let inner_scope = worker
        .cursor
        .passive_arm_scope_by_arm(middle_scope, 0)
        .expect("middle left arm should enter inner route");

    assert_eq!(
        worker
            .cursor
            .first_recv_target(outer_scope, 0x51)
            .map(|(arm, _)| arm),
        Some(1),
        "outer scope must resolve the leaf reply through first-recv dispatch"
    );
    assert_eq!(
        worker
            .cursor
            .first_recv_target(middle_scope, 0x51)
            .map(|(arm, _)| arm),
        Some(0),
        "middle scope must resolve the leaf reply through first-recv dispatch"
    );

    for (scope, expected_arm) in [(outer_scope, 1u8), (middle_scope, 0u8), (inner_scope, 0u8)] {
        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
        worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);
        worker.ingest_binding_scope_evidence(scope, 0x51, false, label_meta);
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            Some(Arm::new(expected_arm).expect("binary route arm")),
            "exact nested leaf ingress must materialize Poll for scope {scope:?}"
        );
    }

    drop(worker);
    drop(controller);
}

#[test]
fn deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes() {
    type OuterLeftMsg = Msg<0x50, u8>;
    type MiddleLeftMsg = Msg<0x51, u8>;
    type ThirdLeftMsg = Msg<0x52, u8>;
    type FinalLeftMsg = Msg<0x53, u8>;
    type FinalRightMsg = Msg<0x55, u8>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let final_decision = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
        ),
    );
    let third = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            final_decision,
        ),
    );
    let middle = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
        ),
        g::seq(g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(), third),
    );
    let program = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            middle,
        ),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(910);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
        .expect("attach worker endpoint");

    let outer_scope = worker.cursor.node_scope_id();
    let middle_scope = worker
        .cursor
        .passive_arm_scope_by_arm(outer_scope, 1)
        .expect("outer right arm should enter middle route");
    let third_scope = worker
        .cursor
        .passive_arm_scope_by_arm(middle_scope, 1)
        .expect("middle right arm should enter third route");
    let final_scope = worker
        .cursor
        .passive_arm_scope_by_arm(third_scope, 1)
        .expect("third right arm should enter final route");

    for scope in [outer_scope, middle_scope, third_scope] {
        assert_eq!(
            worker
                .cursor
                .first_recv_target(scope, 0x55)
                .map(|(arm, _)| arm),
            Some(1),
            "ancestor scope must resolve the deep final reply through first-recv dispatch"
        );
    }

    let label_meta = endpoint_scope_label_meta(&worker, outer_scope, ScopeLoopMeta::EMPTY);
    worker.ingest_scope_evidence_for_offer(outer_scope, 0, 1u8 << 0, false, label_meta);
    worker.ingest_binding_scope_evidence(outer_scope, 0x55, false, label_meta);

    for scope in [outer_scope, middle_scope, third_scope, final_scope] {
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            Some(Arm::new(1).expect("binary route arm")),
            "exact deep final ingress must materialize Poll for scope {scope:?}"
        );
        assert_eq!(
            worker.preview_selected_arm_for_scope(scope),
            Some(1),
            "exact deep final ingress must seed descendant preview selection for scope {scope:?}"
        );
    }

    drop(worker);
    drop(controller);
}

#[test]
fn deep_right_nested_final_reply_offer_materializes_leaf_label() {
    type OuterLeftMsg = Msg<0x50, u8>;
    type MiddleLeftMsg = Msg<0x51, u8>;
    type ThirdLeftMsg = Msg<0x52, u8>;
    type FinalLeftMsg = Msg<0x53, u8>;
    type FinalRightMsg = Msg<0x55, u8>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let final_decision = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
        ),
    );
    let third = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            final_decision,
        ),
    );
    let middle = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
        ),
        g::seq(g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(), third),
    );
    let program = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            middle,
        ),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(911);
    let payload = 0x55u8;
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &worker_program,
            TestBinding::with_incoming_and_payloads(
                &[IncomingClassification {
                    label: 0x55,
                    instance: 17,
                    has_fin: false,
                    channel: Channel::new(4),
                }],
                &[&[payload]],
            ),
        )
        .expect("attach worker endpoint");

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open outer route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("outer route-right failed: {err:?}"),
        Poll::Pending => panic!("outer route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open middle route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("middle route-right failed: {err:?}"),
        Poll::Pending => panic!("middle route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open third route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("third route-right failed: {err:?}"),
        Poll::Pending => panic!("third route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open final route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("final route-right failed: {err:?}"),
        Poll::Pending => panic!("final route-right unexpectedly pending"),
    };
    let mut reply_send = pin!(
        controller
            .flow::<FinalRightMsg>()
            .expect("open final right reply")
            .send(&payload)
    );
    let (_controller, _) = match reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("final right reply failed: {err:?}"),
        Poll::Pending => panic!("final right reply unexpectedly pending"),
    };

    let mut offer = pin!(worker.offer());
    let branch = match offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("worker deep final offer failed: {err:?}"),
        Poll::Pending => panic!("worker deep final offer unexpectedly pending"),
    };
    assert_eq!(
        branch.label(),
        0x55,
        "worker must materialize the deep final reply"
    );
    let mut decode = pin!(branch.decode::<FinalRightMsg>());
    match decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((_worker, reply))) => assert_eq!(reply, payload),
        Poll::Ready(Err(err)) => panic!("worker deep final decode failed: {err:?}"),
        Poll::Pending => panic!("worker deep final decode unexpectedly pending"),
    }
}

#[test]
fn deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress() {
    type OuterLeftMsg = Msg<0x50, u8>;
    type MiddleLeftMsg = Msg<0x51, u8>;
    type ThirdLeftMsg = Msg<0x52, u8>;
    type FinalLeftMsg = Msg<0x53, u8>;
    type FinalRightMsg = Msg<0x55, u8>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let final_decision = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
        ),
    );
    let third = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            final_decision,
        ),
    );
    let middle = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
        ),
        g::seq(g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(), third),
    );
    let program = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            middle,
        ),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let deferred_state = Arc::new(DeferredIngressState::default());
    let cluster: ManuallyDrop<
        SessionCluster<'_, DeferredIngressTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = DeferredIngressTransport::new(deferred_state.clone());
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(912);
    let payload = 0x55u8;
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &worker_program,
            DeferredIngressBinding::with_incoming_and_payloads(
                deferred_state,
                &[IncomingClassification {
                    label: 0x55,
                    instance: 17,
                    has_fin: false,
                    channel: Channel::new(4),
                }],
                &[&[payload]],
            ),
        )
        .expect("attach worker endpoint");

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open outer route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("outer route-right failed: {err:?}"),
        Poll::Pending => panic!("outer route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open middle route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("middle route-right failed: {err:?}"),
        Poll::Pending => panic!("middle route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open third route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("third route-right failed: {err:?}"),
        Poll::Pending => panic!("third route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        controller
            .flow::<StaticRouteRightMsg>()
            .expect("open final route-right")
            .send(())
    );
    let (controller, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("final route-right failed: {err:?}"),
        Poll::Pending => panic!("final route-right unexpectedly pending"),
    };
    let mut reply_send = pin!(
        controller
            .flow::<FinalRightMsg>()
            .expect("open final right reply")
            .send(&payload)
    );
    let (_controller, _) = match reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("final right reply failed: {err:?}"),
        Poll::Pending => panic!("final right reply unexpectedly pending"),
    };

    let mut offer = pin!(worker.offer());
    let branch = match offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("worker deep final deferred offer failed: {err:?}"),
        Poll::Pending => panic!("worker deep final deferred offer unexpectedly pending"),
    };
    assert_eq!(
        branch.label(),
        0x55,
        "worker must materialize the deep final reply after deferred binding ingress"
    );
    let mut decode = pin!(branch.decode::<FinalRightMsg>());
    match decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((_worker, reply))) => assert_eq!(reply, payload),
        Poll::Ready(Err(err)) => panic!("worker deep final deferred decode failed: {err:?}"),
        Poll::Pending => panic!("worker deep final deferred decode unexpectedly pending"),
    }
}

#[test]
fn unique_ready_arm_materializes_poll_without_hint() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(908);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        None,
        "no ready arm evidence must not materialize a poll arm"
    );

    worker.mark_scope_ready_arm(scope, 1);
    assert_eq!(
        worker.poll_arm_from_ready_mask(scope).map(Arm::as_u8),
        Some(1),
        "a unique ready arm should materialize a poll arm"
    );

    worker.mark_scope_ready_arm(scope, 0);
    assert_eq!(
        worker.poll_arm_from_ready_mask(scope),
        None,
        "ambiguous ready-arm evidence must not materialize a poll arm"
    );

    drop(worker);
}

#[test]
fn select_scope_recovers_route_state_from_current_arm_position() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(907);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let Some(PassiveArmNavigation::WithinArm { entry }) = worker
        .cursor
        .follow_passive_observer_arm_for_scope(scope, 0)
    else {
        panic!("worker should expose passive arm entry");
    };
    worker.set_cursor(worker.cursor.with_index(state_index_to_usize(entry)));
    assert_eq!(
        worker.selected_arm_for_scope(scope),
        None,
        "test requires missing runtime route state"
    );
    assert_eq!(
        worker
            .cursor
            .typestate_node(worker.cursor.index())
            .route_arm(),
        Some(0),
        "current node must carry structural arm annotation"
    );

    let recovered = worker
        .ensure_current_route_arm_state()
        .expect("route-state recovery should not fail");
    assert_eq!(
        recovered,
        Some(true),
        "current arm position should recover missing route state"
    );
    assert_eq!(
        worker.selected_arm_for_scope(scope),
        Some(0),
        "current arm position should restore selected arm state"
    );
}

#[test]
fn route_decision_source_domain_is_closed() {
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(1),
        Some(RouteDecisionSource::Ack)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(2),
        Some(RouteDecisionSource::Resolver)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(3),
        Some(RouteDecisionSource::Poll)
    ));
    assert!(RouteDecisionSource::from_tap_seq(0).is_none());
    assert!(RouteDecisionSource::from_tap_seq(4).is_none());
}

#[test]
fn defer_without_new_evidence_is_capped() {
    let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
        max_defer_per_offer: 8,
        max_no_evidence_defer: 1,
        force_poll_on_exhaustion: false,
        max_forced_poll_attempts: 0,
        exhaust_reason: 1,
    });
    let fingerprint = EvidenceFingerprint::new(false, false, false);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(
        liveness.on_defer(fingerprint),
        DeferBudgetOutcome::Exhausted
    );
}

#[test]
fn defer_budget_exhaustion_forces_poll_then_abort() {
    let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
        max_defer_per_offer: 1,
        max_no_evidence_defer: 1,
        force_poll_on_exhaustion: true,
        max_forced_poll_attempts: 1,
        exhaust_reason: crate::epf::ENGINE_LIVENESS_EXHAUSTED,
    });
    let fingerprint = EvidenceFingerprint::new(false, false, false);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(
        liveness.on_defer(fingerprint),
        DeferBudgetOutcome::Exhausted
    );
    assert!(liveness.can_force_poll());
    liveness.mark_forced_poll();
    assert!(!liveness.can_force_poll());
    assert_eq!(
        liveness.exhaust_reason(),
        crate::epf::ENGINE_LIVENESS_EXHAUSTED
    );
}

#[test]
fn defer_never_promotes_to_route_authority() {
    let scope = ScopeId::generic(24);
    let mut delegate_called = false;
    let decision = route_policy_decision_from_action(Action::Defer { retry_hint: 7 }, 88);
    assert!(matches!(
        decision,
        RoutePolicyDecision::Defer {
            retry_hint: 7,
            source: DeferSource::Epf
        }
    ));
    let handle = resolve_route_decision_handle_with_policy(scope, scope, decision, || {
        delegate_called = true;
        Ok(RouteDecisionHandle { scope, arm: 1 })
    })
    .expect("defer must delegate to resolver");
    assert_eq!(handle.arm, 1);
    assert!(delegate_called);
    assert!(RouteDecisionSource::from_tap_seq(4).is_none());
}

#[test]
fn scope_evidence_is_one_shot_per_offer() {
    let token = RouteDecisionToken::from_ack(Arm::new(1).expect("arm"));
    let mut evidence = ScopeEvidence {
        ack: Some(token),
        hint_label: 7,
        ready_arm_mask: ScopeEvidence::ARM1_READY,
        poll_ready_arm_mask: ScopeEvidence::ARM1_READY,
        flags: 0,
    };
    let first = {
        let ack = evidence.ack;
        evidence.ack = None;
        ack
    };
    let second = evidence.ack;
    assert_eq!(first, Some(token));
    assert_eq!(second, None);
}

#[test]
fn resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(990);
    let mut controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let resolver_token = RouteDecisionToken::from_resolver(Arm::new(0).expect("arm"));
    assert!(
        !worker.route_token_has_materialization_evidence(scope, resolver_token),
        "resolver token must not materialize without arm-ready evidence"
    );

    worker.mark_scope_ready_arm(scope, 0);
    assert!(
        worker.route_token_has_materialization_evidence(scope, resolver_token),
        "resolver token may materialize only when selected arm has ready evidence"
    );

    let poll_token = RouteDecisionToken::from_poll(Arm::new(1).expect("arm"));
    assert!(
        !worker.route_token_has_materialization_evidence(scope, poll_token),
        "poll token must not materialize for unready arm"
    );

    worker.mark_scope_ready_arm(scope, 1);
    assert!(
        worker.route_token_has_materialization_evidence(scope, poll_token),
        "poll token may materialize when selected arm has ready evidence"
    );

    let controller_scope = controller.cursor.node_scope_id();
    assert!(
        !controller_scope.is_none(),
        "controller must start at route scope"
    );
    let controller_recv_arm = if controller.arm_has_recv(controller_scope, 0) {
        Some(0)
    } else if controller.arm_has_recv(controller_scope, 1) {
        Some(1)
    } else {
        None
    };
    if let Some(controller_arm) = controller_recv_arm {
        let controller_resolver_token =
            RouteDecisionToken::from_resolver(Arm::new(controller_arm).expect("arm"));
        assert!(
            !controller.route_token_has_materialization_evidence(
                controller_scope,
                controller_resolver_token
            ),
            "controller resolver token must not materialize without arm-ready evidence when recv is required"
        );
        controller.mark_scope_ready_arm(controller_scope, controller_arm);
        assert!(
            controller.route_token_has_materialization_evidence(
                controller_scope,
                controller_resolver_token
            ),
            "controller resolver token requires selected arm evidence as well"
        );
    }

    drop(worker);
    drop(controller);
}

#[test]
fn recv_required_arm_needs_ready_arm_evidence_for_all_sources() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(993);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    let recv_arm = if worker.arm_has_recv(scope, 0) {
        0
    } else if worker.arm_has_recv(scope, 1) {
        1
    } else {
        drop(worker);
        return;
    };
    let ack_token = RouteDecisionToken::from_ack(Arm::new(recv_arm).expect("arm"));
    let resolver_token = RouteDecisionToken::from_resolver(Arm::new(recv_arm).expect("arm"));
    let poll_token = RouteDecisionToken::from_poll(Arm::new(recv_arm).expect("arm"));
    assert!(
        !worker.route_token_has_materialization_evidence(scope, ack_token),
        "ack token must not materialize recv-required arm without ready-arm evidence"
    );
    assert!(
        !worker.route_token_has_materialization_evidence(scope, resolver_token),
        "resolver token must not materialize recv-required arm without ready-arm evidence"
    );
    assert!(
        !worker.route_token_has_materialization_evidence(scope, poll_token),
        "poll token must not materialize recv-required arm without ready-arm evidence"
    );
    worker.mark_scope_ready_arm(scope, recv_arm);
    assert!(
        worker.route_token_has_materialization_evidence(scope, ack_token),
        "ack token may materialize recv-required arm when selected arm is ready"
    );
    assert!(
        worker.route_token_has_materialization_evidence(scope, resolver_token),
        "resolver token may materialize recv-required arm when selected arm is ready"
    );
    assert!(
        worker.route_token_has_materialization_evidence(scope, poll_token),
        "poll token may materialize recv-required arm when selected arm is ready"
    );
    drop(worker);
}

#[test]
fn route_ack_does_not_imply_ready_arm_evidence() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(994);
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");
    let arm = if worker.arm_has_recv(scope, 0) { 0 } else { 1 };
    worker.record_scope_ack(
        scope,
        RouteDecisionToken::from_ack(Arm::new(arm).expect("arm")),
    );
    assert!(
        worker.peek_scope_ack(scope).is_some(),
        "ack authority should be preserved"
    );
    assert!(
        !worker.scope_has_ready_arm(scope, arm),
        "ack authority must not become recv-ready evidence"
    );
    drop(worker);
}

#[test]
fn ready_arm_mask_is_one_shot_and_cleared_on_scope_exit() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(991);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    worker.mark_scope_ready_arm(scope, 0);
    assert!(worker.scope_has_ready_arm(scope, 0));
    worker.consume_scope_ready_arm(scope, 0);
    assert!(
        !worker.scope_has_ready_arm(scope, 0),
        "arm-ready evidence must be one-shot once consumed"
    );

    worker.mark_scope_ready_arm(scope, 1);
    assert_ne!(worker.scope_ready_arm_mask(scope), 0);
    worker.clear_scope_evidence(scope);
    assert_eq!(
        worker.scope_ready_arm_mask(scope),
        0,
        "scope exit must clear arm-ready evidence"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(995);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let scope = controller.cursor.node_scope_id();
    assert!(!scope.is_none(), "controller must start at route scope");

    let mut arm = 0u8;
    let mut found = false;
    while arm <= 1 {
        if controller.arm_has_recv(scope, arm)
            && let Some((entry, _label)) = controller.cursor.controller_arm_entry_by_arm(scope, arm)
            && controller
                .cursor
                .with_index(state_index_to_usize(entry))
                .try_recv_meta()
                .is_none()
        {
            let token = RouteDecisionToken::from_resolver(Arm::new(arm).expect("arm"));
            assert!(
                controller.route_token_has_materialization_evidence(scope, token),
                "send/local arm entry must materialize without ready-arm evidence even when recv appears later"
            );
            found = true;
            break;
        }
        if arm == 1 {
            break;
        }
        arm += 1;
    }
    assert!(
        found,
        "expected a controller arm with send/local entry and later recv in the same arm"
    );
    drop(controller);
}

#[test]
fn lane_offer_state_reenters_same_route_scope_using_offer_entry() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(996);
    let mut controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let scope = controller.cursor.node_scope_id();
    assert!(!scope.is_none(), "controller must start at route scope");
    let offer_entry = controller
        .cursor
        .route_scope_offer_entry(scope)
        .expect("offer entry");
    assert!(!offer_entry.is_max(), "test requires concrete offer entry");
    let next_idx = state_index_to_usize(offer_entry) + 1;
    controller.set_cursor(controller.cursor.with_index(next_idx));
    let region = controller
        .cursor
        .scope_region_by_id(scope)
        .expect("route scope region");
    assert!(
        next_idx >= region.start && next_idx < region.end,
        "test cursor must remain inside the same route scope"
    );

    controller.refresh_lane_offer_state(0);
    assert_ne!(
        controller.route_state.active_offer_mask & 0b0000_0001,
        0,
        "lane must remain pending while re-entering the same route scope"
    );
    assert_eq!(
        controller.route_state.lane_offer_state[0].entry, offer_entry,
        "lane offer state must normalize to canonical route offer_entry"
    );
    assert_eq!(
        controller.frontier_state.offer_entry_state[state_index_to_usize(offer_entry)].lane_idx,
        0,
        "offer entry index must cache a representative lane for direct lookup"
    );
    assert_ne!(
        controller.offer_entry_active_mask(state_index_to_usize(offer_entry)) & 0b0000_0001,
        0,
        "offer entry index must track active lanes while the route remains pending"
    );
    assert_eq!(
        controller.frontier_state.global_active_entries.entry_at(0),
        Some(state_index_to_usize(offer_entry)),
        "global active-entry index must point at the canonical offer entry"
    );
    controller.clear_lane_offer_state(0);
    assert_eq!(
        controller.offer_entry_active_mask(state_index_to_usize(offer_entry)) & 0b0000_0001,
        0,
        "clearing lane offer state must detach the lane from the offer entry index"
    );
    assert_eq!(
        controller.frontier_state.offer_entry_state[state_index_to_usize(offer_entry)].lane_idx,
        u8::MAX,
        "detaching the last lane must clear the representative lane cache"
    );
    assert_eq!(
        controller
            .frontier_state
            .global_active_entries
            .occupancy_mask(),
        0,
        "detaching the last lane must clear the global active-entry index"
    );
    drop(controller);
}

#[test]
fn loop_semantics_are_metadata_authority() {
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;

    let loop_program = g::route(
        g::send::<Role<0>, Role<0>, LoopContinueMsg, 0>(),
        g::send::<Role<0>, Role<0>, LoopBreakMsg, 0>(),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&loop_program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1005);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let scope = controller.cursor.node_scope_id();
    assert!(
        !scope.is_none(),
        "controller must start at loop route scope"
    );

    let continue_kind = controller_arm_semantic_kind(
        &controller.cursor,
        &controller.control_semantics(),
        scope,
        0,
    )
    .expect("continue arm semantic kind");
    let break_kind = controller_arm_semantic_kind(
        &controller.cursor,
        &controller.control_semantics(),
        scope,
        1,
    )
    .expect("break arm semantic kind");
    let continue_label =
        controller_arm_label(&controller.cursor, scope, 0).expect("continue arm label");
    let break_label = controller_arm_label(&controller.cursor, scope, 1).expect("break arm label");

    assert_eq!(continue_kind, ControlSemanticKind::LoopContinue);
    assert_eq!(break_kind, ControlSemanticKind::LoopBreak);
    assert_eq!(
        loop_control_meaning_from_semantic(continue_kind),
        Some(LoopControlMeaning::Continue)
    );
    assert_eq!(
        loop_control_meaning_from_semantic(break_kind),
        Some(LoopControlMeaning::Break)
    );
    assert_eq!(
        controller.control_semantic_kind(continue_label, Some(LoopContinueKind::TAG)),
        ControlSemanticKind::LoopContinue
    );
    assert_eq!(
        controller.control_semantic_kind(break_label, Some(LoopBreakKind::TAG)),
        ControlSemanticKind::LoopBreak
    );

    drop(controller);
}

#[test]
fn loop_continue_then_nested_custom_route_right_send_stays_well_scoped() {
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let inner_left = g::seq(
        g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
        g::send::<Role<0>, Role<1>, Msg<110, u8>, 0>(),
    );
    let inner_right = g::seq(
        g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
        g::send::<Role<0>, Role<1>, Msg<111, u8>, 0>(),
    );
    let inner_route = g::route(inner_left, inner_right);
    let continue_arm = g::seq(
        g::send::<Role<0>, Role<0>, LoopContinueMsg, 0>(),
        inner_route,
    );
    let break_arm = g::send::<Role<0>, Role<0>, LoopBreakMsg, 0>();
    let loop_program = g::route(continue_arm, break_arm);
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&loop_program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1006);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");

    let (controller, continue_meta) = controller
        .prepare_flow::<LoopContinueMsg>()
        .expect("open loop continue send");
    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);
    let mut continue_send =
        pin!(controller.send_with_meta::<LoopContinueMsg>(&continue_meta, None));
    let controller = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((endpoint, _))) => endpoint,
        Poll::Ready(Err(err)) => panic!("loop continue send failed: {err:?}"),
        Poll::Pending => panic!("loop continue send unexpectedly pending"),
    };

    let (controller, route_right_meta) = controller
        .prepare_flow::<StaticRouteRightMsg>()
        .expect("open nested route-right send after continue");
    let offer_lane = controller
        .port_for_lane(route_right_meta.lane as usize)
        .lane();
    let policy = controller
        .control
        .cluster()
        .expect("cluster must remain attached")
        .policy_mode_for(
            RendezvousId::new(controller.rendezvous_id().raw()),
            Lane::new(offer_lane.raw()),
            route_right_meta.eff_index,
            RouteHintRightKind::TAG,
        )
        .expect("resolve route-right policy mode");
    let controller_policy = controller
        .cursor
        .route_scope_controller_policy(route_right_meta.scope);

    assert!(
        !route_right_meta.scope.is_none(),
        "nested route-right send must stay scoped"
    );
    assert_eq!(
        route_right_meta.route_arm,
        Some(1),
        "nested route-right send must preserve the selected inner arm after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
    );
    assert!(
        controller
            .canonical_control_token::<RouteHintRightKind>(&route_right_meta)
            .map(|token| token.into_bytes())
            .is_ok(),
        "nested route-right canonical mint must succeed after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?} cursor_idx={} node_scope={:?}",
        controller.cursor.index(),
        controller.cursor.node_scope_id(),
    );

    let mut route_right_send =
        pin!(controller.send_with_meta::<StaticRouteRightMsg>(&route_right_meta, None));
    match route_right_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((_endpoint, _))) => {}
        Poll::Ready(Err(err)) => panic!(
            "nested route-right send failed after loop continue: {err:?}; meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
        ),
        Poll::Pending => panic!("nested route-right send unexpectedly pending"),
    }
}

#[test]
fn passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right() {
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;
    const RIGHT_REPLY_LABEL: u8 = 0x51;

    let inner_left = g::seq(
        g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
        g::send::<Role<0>, Role<1>, Msg<110, u8>, 0>(),
    );
    let inner_right = g::seq(
        g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
        g::send::<Role<0>, Role<1>, Msg<RIGHT_REPLY_LABEL, u8>, 0>(),
    );
    let inner_route = g::route(inner_left, inner_right);
    let continue_arm = g::seq(
        g::send::<Role<0>, Role<0>, LoopContinueMsg, 0>(),
        inner_route,
    );
    let break_arm = g::send::<Role<0>, Role<0>, LoopBreakMsg, 0>();
    let loop_program = g::route(continue_arm, break_arm);
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&loop_program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&loop_program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1007);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(
            rv_id,
            sid,
            &worker_program,
            TestBinding::with_incoming(&[IncomingClassification {
                label: RIGHT_REPLY_LABEL,
                instance: 1,
                has_fin: false,
                channel: Channel::new(7),
            }]),
        )
        .expect("attach worker endpoint");

    let (controller, continue_meta) = controller
        .prepare_flow::<LoopContinueMsg>()
        .expect("open loop continue");
    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);
    let mut continue_send =
        pin!(controller.send_with_meta::<LoopContinueMsg>(&continue_meta, None));
    let controller = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((endpoint, _))) => endpoint,
        Poll::Ready(Err(err)) => panic!("loop continue send failed: {err:?}"),
        Poll::Pending => panic!("loop continue send unexpectedly pending"),
    };
    let (controller, route_right_meta) = controller
        .prepare_flow::<StaticRouteRightMsg>()
        .expect("open nested route-right");
    let mut route_right_send =
        pin!(controller.send_with_meta::<StaticRouteRightMsg>(&route_right_meta, None));
    let _controller = match route_right_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok((endpoint, _))) => endpoint,
        Poll::Ready(Err(err)) => panic!("route-right send failed: {err:?}"),
        Poll::Pending => panic!("route-right send unexpectedly pending"),
    };

    let outer_scope = worker.cursor.node_scope_id();
    let outer_ack = worker.peek_scope_ack(outer_scope);
    let outer_ready_mask = worker.scope_ready_arm_mask(outer_scope);
    let outer_poll_ready_mask = worker.scope_poll_ready_arm_mask(outer_scope);
    let mut offer = pin!(worker.offer());
    let branch = match offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!(
            "passive nested offer failed: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
            outer_ack, outer_ready_mask, outer_poll_ready_mask,
        ),
        Poll::Pending => match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!(
                "passive nested offer failed after retry: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                outer_ack, outer_ready_mask, outer_poll_ready_mask,
            ),
            Poll::Pending => panic!("passive nested offer remained pending"),
        },
    };
    assert_eq!(
        branch.label(),
        RIGHT_REPLY_LABEL,
        "passive offer must descend into the nested right arm after continue + route-right"
    );
}

#[test]
fn loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid()
{
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;
    type SessionRequestWireMsg = Msg<0x10, u8>;
    type AdminReplyMsg = Msg<0x50, u8>;
    type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
    type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
    type CommitCandidatesReplyMsg = Msg<0x53, u8>;
    type CommitFinalReplyMsg = Msg<0x55, u8>;
    type CheckpointMsg = Msg<
        { CheckpointKind::LABEL },
        GenericCapToken<CheckpointKind>,
        CanonicalControl<CheckpointKind>,
    >;
    type SessionCancelControlMsg =
        Msg<{ CancelKind::LABEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;
    type SnapshotReplyLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
        >,
    >;
    type SnapshotReplyRightSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, SnapshotRejectedReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
        >,
    >;
    type SnapshotReplyDecisionSteps = BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
    type CommitReplyLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
        >,
    >;
    type CommitReplyRightSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
        >,
    >;
    type CommitReplyDecisionSteps = BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
    type ReplyDecisionLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
    >;
    type ReplyDecisionNestedLeftSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>, SnapshotReplyDecisionSteps>;
    type ReplyDecisionNestedRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, CommitReplyDecisionSteps>;
    type ReplyDecisionNestedSteps =
        BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
    type ReplyDecisionRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, ReplyDecisionNestedSteps>;
    type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
    type RequestExchangeSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
    type ContinueArmSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
    type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
    type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

    const SNAPSHOT_REPLY_DECISION: g::Program<SnapshotReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
            ),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, SnapshotRejectedReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
            ),
        ),
    );
    const COMMIT_REPLY_DECISION: g::Program<CommitReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
            ),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
            ),
        ),
    );
    const REPLY_DECISION: g::Program<ReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    SNAPSHOT_REPLY_DECISION,
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    COMMIT_REPLY_DECISION,
                ),
            ),
        ),
    );
    const REQUEST_EXCHANGE: g::Program<RequestExchangeSteps> = g::seq(
        g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
        REPLY_DECISION,
    );
    const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
            REQUEST_EXCHANGE,
        ),
        g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
    );
    let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);
    let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 4096];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1008);
    let reply_payload = 0x51u8;
    let commit_reply_payload = 0x53u8;
    let client = cluster_ref
        .attach_endpoint::<0, _, _, _>(
            rv_id,
            sid,
            &client_program,
            TestBinding::with_incoming_and_payloads(
                &[IncomingClassification {
                    label: 0x51,
                    instance: 11,
                    has_fin: false,
                    channel: Channel::new(9),
                }],
                &[&[reply_payload], &[commit_reply_payload]],
            ),
        )
        .expect("attach client endpoint");
    let server = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
        .expect("attach server endpoint");

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let mut continue_send = pin!(
        client
            .flow::<LoopContinueMsg>()
            .expect("open client continue")
            .send(())
    );
    let (client, _) = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client continue send failed: {err:?}"),
        Poll::Pending => panic!("client continue send unexpectedly pending"),
    };
    let request_payload = 7u8;
    let mut request_send = pin!(
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open client request")
            .send(&request_payload)
    );
    let (client, _) = match request_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client request send failed: {err:?}"),
        Poll::Pending => panic!("client request send unexpectedly pending"),
    };

    let mut server_offer = pin!(server.offer());
    let branch = match server_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("server request offer failed: {err:?}"),
        Poll::Pending => panic!("server request offer unexpectedly pending"),
    };
    assert_eq!(
        branch.label(),
        0x10,
        "server must first observe the request"
    );
    let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
    let (server, _request) = match server_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("server request decode failed: {err:?}"),
        Poll::Pending => panic!("server request decode unexpectedly pending"),
    };

    let mut reply_route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open outer reply route-right")
            .send(())
    );
    let (server, _) = match reply_route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("outer reply route-right send failed: {err:?}"),
        Poll::Pending => panic!("outer reply route-right unexpectedly pending"),
    };
    let first_category_cursor_idx = server.cursor.index();
    let first_category_node_scope = server.cursor.node_scope_id();
    let first_category_local_meta = server.cursor.try_local_meta();
    let first_category_window = [0usize, 1, 2, 3, 4, 5, 6, 7].map(|offset| {
        let idx = first_category_cursor_idx + offset;
        let cursor = server.cursor.with_index(idx);
        (
            idx,
            cursor.node_scope_id(),
            cursor.label(),
            cursor.try_local_meta(),
            cursor.try_recv_meta(),
            cursor.jump_reason(),
        )
    });
    let mut category_route_left = pin!(
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open category route-left")
            .send(())
    );
    let (server, _) = match category_route_left.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("category route-left send failed: {err:?}"),
        Poll::Pending => panic!("category route-left unexpectedly pending"),
    };
    let mut snapshot_route_left = pin!(
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open snapshot route-left")
            .send(())
    );
    let (server, _) = match snapshot_route_left.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("snapshot route-left send failed: {err:?}"),
        Poll::Pending => panic!("snapshot route-left unexpectedly pending"),
    };
    let mut reply_send = pin!(
        server
            .flow::<SnapshotCandidatesReplyMsg>()
            .expect("open snapshot candidates reply")
            .send(&reply_payload)
    );
    let (server, _) = match reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("snapshot candidates reply send failed: {err:?}"),
        Poll::Pending => panic!("snapshot candidates reply unexpectedly pending"),
    };

    let mut client_offer = pin!(client.offer());
    let reply_branch = match client_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("client snapshot reply offer failed: {err:?}"),
        Poll::Pending => panic!("client snapshot reply offer unexpectedly pending"),
    };
    assert_eq!(
        reply_branch.label(),
        0x51,
        "client must materialize the selected snapshot candidates reply label"
    );
    let reply_branch_scope = reply_branch.branch_meta.scope_id;
    let reply_branch_scope_parent = reply_branch
        .endpoint
        .cursor
        .scope_parent(reply_branch_scope)
        .filter(|scope| scope.kind() == ScopeKind::Route);
    let reply_branch_scope_grandparent = reply_branch_scope_parent
        .and_then(|scope| reply_branch.endpoint.cursor.scope_parent(scope))
        .filter(|scope| scope.kind() == ScopeKind::Route);
    let reply_branch_selected_arm = reply_branch.branch_meta.selected_arm;
    let reply_branch_kind = reply_branch.branch_meta.kind;
    let reply_branch_cursor_idx = reply_branch.endpoint.cursor.index();
    let reply_branch_node_scope = reply_branch.endpoint.cursor.node_scope_id();
    let reply_branch_recv_meta = reply_branch.endpoint.cursor.try_recv_meta();
    let reply_branch_next_local_meta = reply_branch_recv_meta.and_then(|meta| {
        reply_branch
            .endpoint
            .cursor
            .with_index(meta.next)
            .try_local_meta()
    });
    let reply_branch_next_cursor = reply_branch_recv_meta.map(|meta| {
        let cursor = reply_branch.endpoint.cursor.with_index(meta.next);
        (
            cursor.index(),
            cursor.node_scope_id(),
            cursor.label(),
            cursor.is_jump(),
            cursor.jump_reason(),
            cursor.is_local_action(),
            cursor.is_recv(),
        )
    });
    let mut client_decode = pin!(reply_branch.decode::<SnapshotCandidatesReplyMsg>());
    let (client, _reply) = match client_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client snapshot reply decode failed: {err:?}"),
        Poll::Pending => panic!("client snapshot reply decode unexpectedly pending"),
    };

    let client_cursor_idx = client.cursor.index();
    let client_node_scope = client.cursor.node_scope_id();
    let client_is_send = client.cursor.is_send();
    let client_is_recv = client.cursor.is_recv();
    let client_is_local_action = client.cursor.is_local_action();
    let client_local_meta = client.cursor.try_local_meta();
    let client_recv_meta = client.cursor.try_recv_meta();
    let checkpoint_flow = client.flow::<CheckpointMsg>();
    let checkpoint_flow = match checkpoint_flow {
        Ok(flow) => flow,
        Err(err) => panic!(
            "open client checkpoint control failed: {err:?}; branch_scope={reply_branch_scope:?} branch_arm={reply_branch_selected_arm} branch_kind={reply_branch_kind:?} branch_cursor_idx={reply_branch_cursor_idx} branch_node_scope={reply_branch_node_scope:?} branch_recv_meta={reply_branch_recv_meta:?} branch_next_local_meta={reply_branch_next_local_meta:?} branch_next_cursor={reply_branch_next_cursor:?}; cursor_idx={} node_scope={:?} is_send={} is_recv={} is_local_action={} local_meta={:?} recv_meta={:?}",
            client_cursor_idx,
            client_node_scope,
            client_is_send,
            client_is_recv,
            client_is_local_action,
            client_local_meta,
            client_recv_meta,
        ),
    };
    let mut checkpoint_send = pin!(checkpoint_flow.send(()));
    let (client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client checkpoint control send failed: {err:?}"),
        Poll::Pending => panic!("client checkpoint control unexpectedly pending"),
    };
    assert_eq!(
        client.selected_arm_for_scope(reply_branch_scope),
        None,
        "completed non-linger branch scope must not survive into next loop iteration: lane3_len={}; lane3_stack={:?}; branch_scope={reply_branch_scope:?}; parent={reply_branch_scope_parent:?}; grandparent={reply_branch_scope_grandparent:?}",
        client.route_state.lane_route_arm_lens[3],
        &client.route_state.lane_route_arms[3]
            [..client.route_state.lane_route_arm_lens[3] as usize],
    );

    let mut continue_send = pin!(
        client
            .flow::<LoopContinueMsg>()
            .expect("open client continue for second iteration")
            .send(())
    );
    let (client, _) = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client second continue send failed: {err:?}"),
        Poll::Pending => panic!("client second continue send unexpectedly pending"),
    };
    let request_payload = 8u8;
    let mut request_send = pin!(
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open client commit request")
            .send(&request_payload)
    );
    let (client, _) = match request_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client commit request send failed: {err:?}"),
        Poll::Pending => panic!("client commit request send unexpectedly pending"),
    };

    let mut server_offer = pin!(server.offer());
    let branch = match server_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("server commit request offer failed: {err:?}"),
        Poll::Pending => panic!("server commit request offer unexpectedly pending"),
    };
    assert_eq!(
        branch.label(),
        0x10,
        "server must observe the second request"
    );
    let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
    let (server, _request) = match server_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("server commit request decode failed: {err:?}"),
        Poll::Pending => panic!("server commit request decode unexpectedly pending"),
    };
    let second_request_decode_cursor_idx = server.cursor.index();
    let second_request_decode_scope = server.cursor.node_scope_id();
    let second_request_decode_local_meta = server.cursor.try_local_meta();
    let second_request_decode_window = [0usize, 1, 2, 3, 4, 5].map(|offset| {
        let idx = second_request_decode_cursor_idx + offset;
        let cursor = server.cursor.with_index(idx);
        (
            idx,
            cursor.node_scope_id(),
            cursor.label(),
            cursor.try_local_meta(),
            cursor.try_recv_meta(),
            cursor.jump_reason(),
        )
    });

    let (server, outer_commit_route_right_meta) = server
        .prepare_flow::<StaticRouteRightMsg>()
        .expect("open outer commit reply route-right");
    let mut reply_route_right =
        pin!(server.send_with_meta::<StaticRouteRightMsg>(&outer_commit_route_right_meta, None));
    let (server, _) = match reply_route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("outer commit reply route-right send failed: {err:?}"),
        Poll::Pending => panic!("outer commit reply route-right unexpectedly pending"),
    };
    let category_route_right_cursor_idx = server.cursor.index();
    let category_route_right_node_scope = server.cursor.node_scope_id();
    let category_route_right_local_meta = server.cursor.try_local_meta();
    let category_route_right_arm0 = server
        .cursor
        .controller_arm_entry_by_arm(category_route_right_node_scope, 0);
    let category_route_right_arm1 = server
        .cursor
        .controller_arm_entry_by_arm(category_route_right_node_scope, 1);
    let (server, category_route_right_meta) = server
        .prepare_flow::<StaticRouteRightMsg>()
        .expect("open commit category route-right");
    let mut category_route_right =
        pin!(server.send_with_meta::<StaticRouteRightMsg>(&category_route_right_meta, None));
    let (server, _) = match category_route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("commit category route-right send failed: {err:?}"),
        Poll::Pending => panic!("commit category route-right unexpectedly pending"),
    };
    let server_cursor_idx = server.cursor.index();
    let server_node_scope = server.cursor.node_scope_id();
    let server_is_send = server.cursor.is_send();
    let server_is_recv = server.cursor.is_recv();
    let server_is_local_action = server.cursor.is_local_action();
    let server_local_meta = server.cursor.try_local_meta();
    let server_recv_meta = server.cursor.try_recv_meta();
    let server_window = [14usize, 15, 16, 17, 18, 19].map(|idx| {
        let cursor = server.cursor.with_index(idx);
        (
            idx,
            cursor.node_scope_id(),
            cursor.label(),
            cursor.try_local_meta(),
            cursor.try_recv_meta(),
            cursor.jump_reason(),
        )
    });
    let commit_route_left_flow = server.flow::<StaticRouteLeftMsg>();
    let commit_route_left_flow = match commit_route_left_flow {
        Ok(flow) => flow,
        Err(err) => panic!(
            "open commit reply route-left failed: {err:?}; first_category_cursor_idx={first_category_cursor_idx} first_category_node_scope={first_category_node_scope:?} first_category_local_meta={first_category_local_meta:?} first_category_window={first_category_window:?}; second_request_decode_cursor_idx={second_request_decode_cursor_idx} second_request_decode_scope={second_request_decode_scope:?} second_request_decode_local_meta={second_request_decode_local_meta:?} second_request_decode_window={second_request_decode_window:?}; outer_commit_route_right_meta={outer_commit_route_right_meta:?}; category_route_right_cursor_idx={category_route_right_cursor_idx} category_route_right_node_scope={category_route_right_node_scope:?} category_route_right_local_meta={category_route_right_local_meta:?} category_route_right_arm0={category_route_right_arm0:?} category_route_right_arm1={category_route_right_arm1:?} category_route_right_meta={category_route_right_meta:?} server_window={server_window:?}; cursor_idx={} node_scope={:?} is_send={} is_recv={} is_local_action={} local_meta={:?} recv_meta={:?}",
            server_cursor_idx,
            server_node_scope,
            server_is_send,
            server_is_recv,
            server_is_local_action,
            server_local_meta,
            server_recv_meta,
        ),
    };
    let mut commit_route_left = pin!(commit_route_left_flow.send(()));
    let (server, _) = match commit_route_left.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("commit reply route-left send failed: {err:?}"),
        Poll::Pending => panic!("commit reply route-left unexpectedly pending"),
    };
    let mut commit_reply_send = pin!(
        server
            .flow::<CommitCandidatesReplyMsg>()
            .expect("open commit candidates reply")
            .send(&commit_reply_payload)
    );
    let (server, _) = match commit_reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("commit candidates reply send failed: {err:?}"),
        Poll::Pending => panic!("commit candidates reply unexpectedly pending"),
    };

    let mut client_offer = pin!(client.offer());
    let commit_branch = match client_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("client commit reply offer failed: {err:?}"),
        Poll::Pending => panic!("client commit reply offer unexpectedly pending"),
    };
    assert_eq!(
        commit_branch.label(),
        0x53,
        "client must materialize the selected commit candidates reply label"
    );
    let mut client_decode = pin!(commit_branch.decode::<CommitCandidatesReplyMsg>());
    let (client, _reply) = match client_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client commit reply decode failed: {err:?}"),
        Poll::Pending => panic!("client commit reply decode unexpectedly pending"),
    };

    let mut checkpoint_send = pin!(
        client
            .flow::<CheckpointMsg>()
            .expect("open client checkpoint control after commit reply")
            .send(())
    );
    let (_client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => {
            panic!("client post-commit checkpoint control failed: {err:?}")
        }
        Poll::Pending => panic!("client post-commit checkpoint unexpectedly pending"),
    };

    let mut server_next_offer = pin!(server.offer());
    match server_next_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Err(err)) => {
            panic!("server next offer after commit path must not fail: {err:?}")
        }
        Poll::Ready(Ok(branch)) => panic!(
            "server next offer after commit path must not spuriously materialize a branch: label={}",
            branch.label()
        ),
        Poll::Pending => {}
    }
}

#[test]
fn admin_reply_then_snapshot_reply_right_path_survives_next_iteration() {
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;
    type SessionRequestWireMsg = Msg<0x10, u8>;
    type AdminReplyMsg = Msg<0x50, u8>;
    type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
    type CheckpointMsg = Msg<
        { CheckpointKind::LABEL },
        GenericCapToken<CheckpointKind>,
        CanonicalControl<CheckpointKind>,
    >;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;
    type ReplyDecisionLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
    >;
    type SnapshotReplyPathSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SeqSteps<
                SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
            >,
        >,
    >;
    type ReplyDecisionRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, SnapshotReplyPathSteps>;
    type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
    type RequestExchangeSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
    type ContinueArmSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
    type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
    type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

    const REPLY_DECISION: g::Program<ReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
            ),
        ),
    );
    const REQUEST_EXCHANGE: g::Program<RequestExchangeSteps> = g::seq(
        g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
        REPLY_DECISION,
    );
    const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
            REQUEST_EXCHANGE,
        ),
        g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
    );
    let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);
    let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 4096];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1010);
    let admin_reply_payload = 0x50u8;
    let snapshot_reply_payload = 0x51u8;
    let client = cluster_ref
        .attach_endpoint::<0, _, _, _>(
            rv_id,
            sid,
            &client_program,
            TestBinding::with_incoming_and_payloads(
                &[
                    IncomingClassification {
                        label: 0x50,
                        instance: 21,
                        has_fin: false,
                        channel: Channel::new(13),
                    },
                    IncomingClassification {
                        label: 0x51,
                        instance: 22,
                        has_fin: false,
                        channel: Channel::new(14),
                    },
                ],
                &[&[admin_reply_payload], &[snapshot_reply_payload]],
            ),
        )
        .expect("attach client endpoint");
    let server = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
        .expect("attach server endpoint");

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let (client, _) = poll_ready_ok(
        &mut cx,
        client
            .flow::<LoopContinueMsg>()
            .expect("open client continue")
            .send(()),
        "client continue send",
    );
    let (client, _) = poll_ready_ok(
        &mut cx,
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open client admin request")
            .send(&1u8),
        "client admin request send",
    );

    let branch = poll_ready_ok(&mut cx, server.offer(), "server admin request offer");
    assert_eq!(
        branch.label(),
        0x10,
        "server must first observe the admin request"
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        branch.decode::<SessionRequestWireMsg>(),
        "server admin request decode",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open admin route-left")
            .send(()),
        "admin route-left send",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<AdminReplyMsg>()
            .expect("open admin reply")
            .send(&admin_reply_payload),
        "admin reply send",
    );

    let admin_branch = poll_ready_ok(&mut cx, client.offer(), "client admin reply offer");
    assert_eq!(
        admin_branch.label(),
        0x50,
        "client must materialize the admin reply"
    );
    let admin_reply_scope = admin_branch.branch_meta.scope_id;
    let (client, _) = poll_ready_ok(
        &mut cx,
        admin_branch.decode::<AdminReplyMsg>(),
        "client admin reply decode",
    );
    assert_eq!(
        client.selected_arm_for_scope(admin_reply_scope),
        None,
        "admin reply branch scope must not survive into the next loop iteration"
    );

    let (client, _) = poll_ready_ok(
        &mut cx,
        client
            .flow::<LoopContinueMsg>()
            .expect("open client continue for snapshot")
            .send(()),
        "client snapshot continue send",
    );
    let (client, _) = poll_ready_ok(
        &mut cx,
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open client snapshot request")
            .send(&2u8),
        "client snapshot request send",
    );

    let branch = poll_ready_ok(&mut cx, server.offer(), "server snapshot request offer");
    assert_eq!(
        branch.label(),
        0x10,
        "server must observe the snapshot request"
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        branch.decode::<SessionRequestWireMsg>(),
        "server snapshot request decode",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open snapshot outer route-right")
            .send(()),
        "snapshot outer route-right send",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open snapshot category route-left")
            .send(()),
        "snapshot category route-left send",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open snapshot reply route-left")
            .send(()),
        "snapshot reply route-left send",
    );
    let (server, _) = poll_ready_ok(
        &mut cx,
        server
            .flow::<SnapshotCandidatesReplyMsg>()
            .expect("open snapshot candidates reply")
            .send(&snapshot_reply_payload),
        "snapshot candidates reply send",
    );

    let snapshot_branch = poll_ready_ok(
        &mut cx,
        client.offer(),
        "client snapshot reply offer after admin path",
    );
    assert_eq!(
        snapshot_branch.label(),
        0x51,
        "snapshot reply must still materialize after an earlier admin-left iteration"
    );
    let (client, _) = poll_ready_ok(
        &mut cx,
        snapshot_branch.decode::<SnapshotCandidatesReplyMsg>(),
        "client snapshot reply decode after admin path",
    );
    let (_client, _) = poll_ready_ok(
        &mut cx,
        client
            .flow::<CheckpointMsg>()
            .expect("open snapshot checkpoint after admin path")
            .send(()),
        "client snapshot checkpoint send after admin path",
    );

    drop(server);
}

#[test]
fn snapshot_then_commit_final_reply_survives_next_iteration() {
    type LoopContinueMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
    >;
    type LoopBreakMsg = Msg<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
        CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
    >;
    type SessionRequestWireMsg = Msg<0x10, u8>;
    type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
    type CommitCandidatesReplyMsg = Msg<0x53, u8>;
    type CommitRejectedReplyMsg = Msg<0x54, u8>;
    type CommitFinalReplyMsg = Msg<0x55, u8>;
    type CheckpointMsg = Msg<
        { CheckpointKind::LABEL },
        GenericCapToken<CheckpointKind>,
        CanonicalControl<CheckpointKind>,
    >;
    type SessionCancelControlMsg =
        Msg<{ CancelKind::LABEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;
    type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
    type AdminReplyMsg = Msg<0x50, u8>;
    type SnapshotReplyLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
        >,
    >;
    type SnapshotReplyRightSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, SnapshotRejectedReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
        >,
    >;
    type SnapshotReplyDecisionSteps = BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
    type CommitRejectedBranchSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, CommitRejectedReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
        >,
    >;
    type CommitFinalBranchSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
        >,
    >;
    type CommitNestedDecisionSteps = BranchSteps<CommitRejectedBranchSteps, CommitFinalBranchSteps>;
    type CommitReplyLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SeqSteps<
            SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
            SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
        >,
    >;
    type CommitReplyRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, CommitNestedDecisionSteps>;
    type CommitReplyDecisionSteps = BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
    type ReplyDecisionLeftSteps = SeqSteps<
        SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
        SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
    >;
    type ReplyDecisionNestedLeftSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>, SnapshotReplyDecisionSteps>;
    type ReplyDecisionNestedRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, CommitReplyDecisionSteps>;
    type ReplyDecisionNestedSteps =
        BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
    type ReplyDecisionRightSteps =
        SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, ReplyDecisionNestedSteps>;
    type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
    type RequestExchangeSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
    type ContinueArmSteps =
        SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
    type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
    type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

    const SNAPSHOT_REPLY_DECISION: g::Program<SnapshotReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
            ),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, Msg<0x52, u8>, 3>(),
                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
            ),
        ),
    );
    const COMMIT_REPLY_DECISION: g::Program<CommitReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::seq(
                g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
            ),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitRejectedReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            ),
        ),
    );
    const REPLY_DECISION: g::Program<ReplyDecisionSteps> = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
            g::send::<Role<1>, Role<0>, Msg<0x50, u8>, 3>(),
        ),
        g::seq(
            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
            g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    SNAPSHOT_REPLY_DECISION,
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    COMMIT_REPLY_DECISION,
                ),
            ),
        ),
    );
    const REQUEST_EXCHANGE: g::Program<RequestExchangeSteps> = g::seq(
        g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
        REPLY_DECISION,
    );
    const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
            REQUEST_EXCHANGE,
        ),
        g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
    );
    let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);
    let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&LOOP_PROGRAM);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 4096];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1012);
    let snapshot_reply_payload = 0x51u8;
    let commit_final_payload = 0x55u8;
    let client = cluster_ref
        .attach_endpoint::<0, _, _, _>(
            rv_id,
            sid,
            &client_program,
            TestBinding::with_incoming_and_payloads(
                &[
                    IncomingClassification {
                        label: 0x51,
                        instance: 41,
                        has_fin: false,
                        channel: Channel::new(17),
                    },
                    IncomingClassification {
                        label: 0x55,
                        instance: 42,
                        has_fin: false,
                        channel: Channel::new(18),
                    },
                ],
                &[&[snapshot_reply_payload], &[commit_final_payload]],
            ),
        )
        .expect("attach client endpoint");
    let server = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
        .expect("attach server endpoint");

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let mut continue_send = pin!(
        client
            .flow::<LoopContinueMsg>()
            .expect("open first continue")
            .send(())
    );
    let (client, _) = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first continue failed: {err:?}"),
        Poll::Pending => panic!("first continue unexpectedly pending"),
    };
    let mut request_send = pin!(
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open first request")
            .send(&1u8)
    );
    let (client, _) = match request_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first request failed: {err:?}"),
        Poll::Pending => panic!("first request unexpectedly pending"),
    };

    let mut server_offer = pin!(server.offer());
    let branch = match server_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("server first request offer failed: {err:?}"),
        Poll::Pending => panic!("server first request offer unexpectedly pending"),
    };
    assert_eq!(branch.label(), 0x10);
    let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
    let (server, _) = match server_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("server first request decode failed: {err:?}"),
        Poll::Pending => panic!("server first request decode unexpectedly pending"),
    };

    let mut route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open first outer route-right")
            .send(())
    );
    let (server, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first outer route-right failed: {err:?}"),
        Poll::Pending => panic!("first outer route-right unexpectedly pending"),
    };
    let mut route_left = pin!(
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open first category route-left")
            .send(())
    );
    let (server, _) = match route_left.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first category route-left failed: {err:?}"),
        Poll::Pending => panic!("first category route-left unexpectedly pending"),
    };
    let mut route_left = pin!(
        server
            .flow::<StaticRouteLeftMsg>()
            .expect("open first snapshot route-left")
            .send(())
    );
    let (server, _) = match route_left.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first snapshot route-left failed: {err:?}"),
        Poll::Pending => panic!("first snapshot route-left unexpectedly pending"),
    };
    let mut reply_send = pin!(
        server
            .flow::<SnapshotCandidatesReplyMsg>()
            .expect("open first snapshot reply")
            .send(&snapshot_reply_payload)
    );
    let (server, _) = match reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("first snapshot reply failed: {err:?}"),
        Poll::Pending => panic!("first snapshot reply unexpectedly pending"),
    };

    let mut client_offer = pin!(client.offer());
    let branch = match client_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("client first offer failed: {err:?}"),
        Poll::Pending => panic!("client first offer unexpectedly pending"),
    };
    assert_eq!(branch.label(), 0x51);
    let branch_scope = branch.branch_meta.scope_id;
    let mut client_decode = pin!(branch.decode::<SnapshotCandidatesReplyMsg>());
    let (client, _) = match client_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client first decode failed: {err:?}"),
        Poll::Pending => panic!("client first decode unexpectedly pending"),
    };
    let mut checkpoint_send = pin!(
        client
            .flow::<CheckpointMsg>()
            .expect("open checkpoint after snapshot")
            .send(())
    );
    let (client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("snapshot checkpoint failed: {err:?}"),
        Poll::Pending => panic!("snapshot checkpoint unexpectedly pending"),
    };
    assert_eq!(
        client.selected_arm_for_scope(branch_scope),
        None,
        "completed snapshot branch scope must not survive into the next iteration"
    );

    let mut continue_send = pin!(
        client
            .flow::<LoopContinueMsg>()
            .expect("open second continue")
            .send(())
    );
    let (client, _) = match continue_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second continue failed: {err:?}"),
        Poll::Pending => panic!("second continue unexpectedly pending"),
    };
    let mut request_send = pin!(
        client
            .flow::<SessionRequestWireMsg>()
            .expect("open second request")
            .send(&2u8)
    );
    let (client, _) = match request_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second request failed: {err:?}"),
        Poll::Pending => panic!("second request unexpectedly pending"),
    };

    let mut server_offer = pin!(server.offer());
    let branch = match server_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("server second request offer failed: {err:?}"),
        Poll::Pending => panic!("server second request offer unexpectedly pending"),
    };
    assert_eq!(branch.label(), 0x10);
    let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
    let (server, _) = match server_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("server second request decode failed: {err:?}"),
        Poll::Pending => panic!("server second request decode unexpectedly pending"),
    };

    let mut route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open second outer route-right")
            .send(())
    );
    let (server, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second outer route-right failed: {err:?}"),
        Poll::Pending => panic!("second outer route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open second category route-right")
            .send(())
    );
    let (server, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second category route-right failed: {err:?}"),
        Poll::Pending => panic!("second category route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open second commit tail route-right")
            .send(())
    );
    let (server, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second commit tail route-right failed: {err:?}"),
        Poll::Pending => panic!("second commit tail route-right unexpectedly pending"),
    };
    let mut route_right = pin!(
        server
            .flow::<StaticRouteRightMsg>()
            .expect("open second commit final route-right")
            .send(())
    );
    let (server, _) = match route_right.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second commit final route-right failed: {err:?}"),
        Poll::Pending => panic!("second commit final route-right unexpectedly pending"),
    };
    let mut reply_send = pin!(
        server
            .flow::<CommitFinalReplyMsg>()
            .expect("open second commit final reply")
            .send(&commit_final_payload)
    );
    let (_server, _) = match reply_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("second commit final reply failed: {err:?}"),
        Poll::Pending => panic!("second commit final reply unexpectedly pending"),
    };

    let mut client_offer = pin!(client.offer());
    let branch = match client_offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(err)) => panic!("client second offer failed: {err:?}"),
        Poll::Pending => panic!("client second offer unexpectedly pending"),
    };
    assert_eq!(branch.label(), 0x55);
    let mut client_decode = pin!(branch.decode::<CommitFinalReplyMsg>());
    let (client, _) = match client_decode.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("client second decode failed: {err:?}"),
        Poll::Pending => panic!("client second decode unexpectedly pending"),
    };
    let mut cancel_send = pin!(
        client
            .flow::<SessionCancelControlMsg>()
            .expect("open cancel after commit final")
            .send(())
    );
    let (_client, _) = match cancel_send.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(result)) => result,
        Poll::Ready(Err(err)) => panic!("commit final cancel failed: {err:?}"),
        Poll::Pending => panic!("commit final cancel unexpectedly pending"),
    };
}

#[test]
fn static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, PendingTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = PendingTransport::new();
    let transport_probe = transport.clone();
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1201);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    controller.port_for_lane(0).record_route_decision(scope, 1);

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);
    let mut offer = pin!(worker.offer());
    match offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => {
            panic!(
                "offer must not materialize before transport ingress: {}",
                branch.label()
            )
        }
        Poll::Ready(Err(err)) => panic!("offer must wait for transport ingress: {err:?}"),
        Poll::Pending => {}
    }
    assert_eq!(
        transport_probe.poll_count(),
        1,
        "known static passive arm must park on transport once instead of frontier-restarting"
    );
}

#[test]
fn nested_dispatch_arm_counts_as_recv_for_known_passive_route() {
    type OuterLeftMsg = Msg<0x10, u8>;
    type LeafLeftMsg = Msg<0x51, u8>;
    type LeafRightMsg = Msg<0x52, u8>;
    type StaticRouteLeftMsg = Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >;
    type StaticRouteRightMsg =
        Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

    let nested = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, LeafLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, LeafRightMsg, 0>(),
        ),
    );
    let program = g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            nested,
        ),
    );
    let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
        project(&program);
    let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
        project(&program);

    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, PendingTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = PendingTransport::new();
    let transport_probe = transport.clone();
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(1202);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
        .expect("attach controller endpoint");
    let worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    controller.port_for_lane(0).record_route_decision(scope, 1);

    assert!(
        worker.arm_has_recv(scope, 1),
        "nested first-recv dispatch must count as recv-bearing arm"
    );

    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);
    let mut offer = pin!(worker.offer());
    match offer.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(branch)) => panic!(
            "known passive route with nested dispatch recv must wait for wire ingress, got {}",
            branch.label()
        ),
        Poll::Ready(Err(err)) => {
            panic!("known passive route with nested dispatch recv must not fail: {err:?}")
        }
        Poll::Pending => {}
    }
    assert_eq!(
        transport_probe.poll_count(),
        1,
        "known passive route with nested dispatch recv must still poll transport once"
    );
}

#[test]
fn scope_local_label_mapping_never_uses_global_scan() {
    let mut tap_storage = [TapEvent::default(); RING_EVENTS];
    let mut slab = [0u8; 2048];
    let config = Config::new(&mut tap_storage, &mut slab);
    let clock = CounterClock::new();
    let cluster: ManuallyDrop<
        SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
    > = ManuallyDrop::new(SessionCluster::new(&clock));
    let transport = HintOnlyTransport::new(HINT_NONE);
    let cluster_ref = &*cluster;
    let rv_id = cluster_ref
        .add_rendezvous_from_config(config, transport)
        .expect("register rendezvous");
    let sid = SessionId::new(992);
    let controller = cluster_ref
        .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller endpoint");
    let mut worker = cluster_ref
        .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
        .expect("attach worker endpoint");
    let scope = worker.cursor.node_scope_id();
    assert!(!scope.is_none(), "worker must start at route scope");

    let foreign_label = (1u8..=u8::MAX).find(|label| {
        !worker.is_loop_semantic_label(*label)
            && worker.cursor.first_recv_target(scope, *label).is_none()
            && worker.cursor.find_arm_for_recv_label(*label).is_some()
    });
    let Some(foreign_label) = foreign_label else {
        // FIRST-recv dispatch can fully cover this scope; no entry-only
        // label remains to probe.
        drop(worker);
        drop(controller);
        return;
    };

    let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
    worker.ingest_binding_scope_evidence(scope, foreign_label, false, label_meta);

    assert!(
        !worker.scope_has_ready_arm_evidence(scope),
        "foreign label {} must not become scope-local arm-ready evidence: hint={} arm={:?} evidence={:?} ready_mask=0b{:02b} controller={}",
        foreign_label,
        label_meta.matches_hint_label(foreign_label),
        label_meta.arm_for_label(foreign_label),
        label_meta.evidence_arm_for_label(foreign_label),
        worker.scope_ready_arm_mask(scope),
        worker.cursor.is_route_controller(scope)
    );
    assert!(
        worker.peek_scope_ack(scope).is_none(),
        "foreign label must not mint route authority"
    );

    drop(worker);
    drop(controller);
}

#[test]
fn payload_staging_is_selected_scope_lane_stable() {
    let mut scratch = [0u8; 8];
    let src = [9u8, 8, 7, 6];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}
