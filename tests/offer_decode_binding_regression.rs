#![cfg(feature = "std")]
mod common;
#[path = "support/local_only.rs"]
mod local_only_support;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use common::{TestRx, TestTransport, TestTransportError, TestTransportMetrics, TestTx};
use core::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
};
use hibana::g::{self, Msg, Role};
use hibana::substrate::program::{MessageSpec, RoleProgram, project};
use hibana::substrate::{
    SessionKit, Transport,
    binding::{
        BindingSlot,
        advanced::{Channel, IngressEvidence, TransportOpsError},
    },
    ids::SessionId,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
    transport::Outgoing,
};
use hibana::substrate::{
    cap::{GenericCapToken, advanced::RouteDecisionKind},
    ids::RendezvousId,
    policy::{PolicySignalsProvider, ResolverContext, ResolverError, RouteResolution},
};
use local_only_support::LocalCell;
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const LABEL_ROUTE_DECISION: u8 = 57;
const LABEL_ROUTE_RIGHT_CONTROL: u8 = 119;
type RouteRightKind = route_control_kinds::RouteControl<LABEL_ROUTE_RIGHT_CONTROL, 0>;
const POLICY_AUDIT_EXT_ID: u16 = 0x0408;
const SLOT_TAG_ENDPOINT_RX: u32 = 1;
const SLOT_TAG_ROUTE: u32 = 4;

const ROUTE_POLICY_ID: u16 = 900;
type TestKit = SessionKit<'static, FlowTransport, DefaultLabelUniverse, CounterClock, 2>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static FLOW_SHARED_SLOT: UnsafeCell<MaybeUninit<FlowBindingShared>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_BINDING_SLOT: UnsafeCell<MaybeUninit<FlowBinding>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_BINDING_SLOT: UnsafeCell<MaybeUninit<FlowBinding>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static ROUTE_RESOLVER_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn reset_route_resolver_calls() {
    ROUTE_RESOLVER_CALLS.with(|count| count.set(0));
}

fn route_resolver_calls() -> usize {
    ROUTE_RESOLVER_CALLS.with(Cell::get)
}

fn count_policy_audit_ext_for_slot(
    tap_buf: &[hibana::substrate::tap::TapEvent],
    slot_tag: u32,
) -> usize {
    tap_buf
        .iter()
        .filter(|event| event.id == POLICY_AUDIT_EXT_ID && (event.arg2 >> 24) == slot_tag)
        .count()
}

fn controller_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<71, u32>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<72, u32>, 0>(),
    );
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<Role<0>, Role<1>, Msg<73, u32>, 0>());
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<71, u32>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<72, u32>, 0>(),
    );
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<Role<0>, Role<1>, Msg<73, u32>, 0>());
    project(&program)
}

#[derive(Clone, Copy)]
struct PendingInbound {
    lane: u8,
    evidence: IngressEvidence,
}

const FLOW_ROLE_SLOTS: usize = 2;
const FLOW_MAX_PENDING_PER_ROLE: usize = 4;
const FLOW_MAX_PAYLOADS: usize = 4;
const FLOW_MAX_PAYLOAD_LEN: usize = 8;

#[derive(Clone, Copy, Default)]
struct StoredPayload {
    active: bool,
    channel: u64,
    len: usize,
    bytes: [u8; FLOW_MAX_PAYLOAD_LEN],
}

#[derive(Default)]
struct FlowBindingSharedState {
    next_channel: u64,
    drain_calls: usize,
    incoming: [[Option<PendingInbound>; FLOW_MAX_PENDING_PER_ROLE]; FLOW_ROLE_SLOTS],
    payloads: [StoredPayload; FLOW_MAX_PAYLOADS],
}

impl FlowBindingSharedState {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn push_incoming(&mut self, role: u8, pending: PendingInbound) {
        let queue = self
            .incoming
            .get_mut(role as usize)
            .expect("role queue must exist");
        let mut idx = 0usize;
        while idx < queue.len() {
            if queue[idx].is_none() {
                queue[idx] = Some(pending);
                return;
            }
            idx += 1;
        }
        panic!("incoming queue exhausted");
    }

    fn take_incoming_for_lane(&mut self, role: u8, logical_lane: u8) -> Option<IngressEvidence> {
        let queue = self.incoming.get_mut(role as usize)?;
        let mut idx = 0usize;
        while idx < queue.len() {
            if let Some(entry) = queue[idx]
                && entry.lane == logical_lane
            {
                let evidence = entry.evidence;
                let mut tail = idx;
                while tail + 1 < queue.len() {
                    queue[tail] = queue[tail + 1];
                    tail += 1;
                }
                queue[queue.len() - 1] = None;
                return Some(evidence);
            }
            idx += 1;
        }
        None
    }

    fn store_payload(&mut self, channel: u64, payload: &[u8]) {
        assert!(
            payload.len() <= FLOW_MAX_PAYLOAD_LEN,
            "payload exceeds fixed test storage"
        );
        let mut idx = 0usize;
        while idx < self.payloads.len() {
            let slot = &mut self.payloads[idx];
            if !slot.active {
                slot.active = true;
                slot.channel = channel;
                slot.len = payload.len();
                slot.bytes[..payload.len()].copy_from_slice(payload);
                return;
            }
            idx += 1;
        }
        panic!("payload slots exhausted");
    }

    fn take_payload(&mut self, channel: u64, buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        let mut idx = 0usize;
        while idx < self.payloads.len() {
            let slot = &mut self.payloads[idx];
            if slot.active && slot.channel == channel {
                if slot.len > buf.len() {
                    return Err(TransportOpsError::WriteFailed {
                        expected: slot.len,
                        actual: buf.len(),
                    });
                }
                buf[..slot.len].copy_from_slice(&slot.bytes[..slot.len]);
                let len = slot.len;
                *slot = StoredPayload::default();
                return Ok(len);
            }
            idx += 1;
        }
        Err(TransportOpsError::ChannelNotFound)
    }
}

struct FlowBindingShared {
    state: LocalCell<FlowBindingSharedState>,
}

impl FlowBindingShared {
    fn new() -> Self {
        Self {
            state: LocalCell::new(FlowBindingSharedState::default()),
        }
    }

    fn reset(&self) {
        self.state.with_mut(FlowBindingSharedState::clear);
    }
}

#[derive(Clone)]
struct FlowBinding {
    role: u8,
    shared: &'static FlowBindingShared,
}

impl FlowBinding {
    fn new(role: u8, shared: &'static FlowBindingShared) -> Self {
        Self { role, shared }
    }
}

impl BindingSlot for FlowBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence> {
        self.shared
            .state
            .with_mut(|state| state.take_incoming_for_lane(self.role, logical_lane))
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<hibana::substrate::wire::Payload<'a>, TransportOpsError> {
        let len = self
            .shared
            .state
            .with_mut(|state| state.take_payload(channel.raw(), buf))?;
        Ok(hibana::substrate::wire::Payload::new(&buf[..len]))
    }

    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        None
    }
}

#[derive(Clone)]
struct FlowTransport {
    inner: TestTransport,
    shared: &'static FlowBindingShared,
}

impl FlowTransport {
    fn new(shared: &'static FlowBindingShared) -> Self {
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
        = TestRx<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(local_role, session_id)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        if outgoing.is_send() && outgoing.label() == <Msg<71, u32> as MessageSpec>::LABEL {
            self.shared.state.with_mut(|shared| {
                let channel = Channel::new(shared.next_channel);
                shared.next_channel += 1;
                shared.store_payload(channel.raw(), outgoing.payload().as_bytes());
                let evidence = IngressEvidence {
                    label: outgoing.label(),
                    instance: 0,
                    has_fin: false,
                    channel,
                };
                shared.push_incoming(
                    outgoing.peer(),
                    PendingInbound {
                        lane: outgoing.lane(),
                        evidence,
                    },
                );
            });
            return std::task::Poll::Ready(Ok(()));
        }
        self.inner.poll_send(tx, outgoing, cx)
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<hibana::substrate::wire::Payload<'a>, Self::Error>> {
        self.inner.poll_recv(rx, cx)
    }

    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx)
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.inner.requeue(rx)
    }

    fn drain_events(
        &self,
        emit: &mut dyn FnMut(hibana::substrate::transport::advanced::TransportEvent),
    ) {
        self.shared
            .state
            .with_mut(|state| state.drain_calls = state.drain_calls.wrapping_add(1));
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

fn register_route_resolvers_for_program<const ROLE: u8, T, const MAX_RV: usize>(
    cluster: &SessionKit<'_, T, DefaultLabelUniverse, CounterClock, MAX_RV>,
    rv_id: RendezvousId,
    program: &RoleProgram<ROLE>,
) where
    T: Transport + 'static,
{
    cluster
        .set_resolver::<ROUTE_POLICY_ID, ROLE>(
            rv_id,
            program,
            hibana::substrate::policy::ResolverRef::route_fn(always_left_route_resolver),
        )
        .expect("register route resolver");
}

fn always_left_route_resolver(_ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    ROUTE_RESOLVER_CALLS.with(|count| count.set(count.get().wrapping_add(1)));
    Ok(RouteResolution::Arm(0))
}

#[test]
fn flow_preview_is_policy_free_until_send_consumes_it() {
    with_fixture(|clock, tap_buf, slab| {
        reset_route_resolver_calls();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let tap_ptr = tap_buf.as_ptr();
                let tap_len = tap_buf.len();
                let tap_events = || unsafe { core::slice::from_raw_parts(tap_ptr, tap_len) };
                let config = Config::new(tap_buf, slab);
                with_tls_mut(
                    &FLOW_SHARED_SLOT,
                    |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                    |shared| {
                        shared.reset();
                        let shared_ref: &'static FlowBindingShared = shared;
                        let transport = FlowTransport::new(shared_ref);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );

                        let sid = SessionId::new(900);
                        with_tls_mut(
                            &CONTROLLER_BINDING_SLOT,
                            |ptr: *mut FlowBinding| unsafe {
                                ptr.write(FlowBinding::new(0, shared_ref))
                            },
                            |controller_binding| {
                                with_tls_mut(
                                    &CONTROLLER_ENDPOINT_SLOT,
                                    |ptr| unsafe {
                                        write_value(
                                            ptr,
                                            cluster
                                                .enter(
                                                    rv_id,
                                                    sid,
                                                    &controller_program(),
                                                    controller_binding,
                                                )
                                                .expect("attach controller"),
                                        );
                                    },
                                    |controller| {
                                        let route_policy_calls_before = route_resolver_calls();
                                        let drain_calls_before =
                                            shared_ref.state.with(|state| state.drain_calls);
                                        let route_audit_before = count_policy_audit_ext_for_slot(
                                            tap_events(),
                                            SLOT_TAG_ROUTE,
                                        );

                                        let flow = controller
                                            .flow::<Msg<
                                                { LABEL_ROUTE_DECISION },
                                                GenericCapToken<RouteDecisionKind>,
                                                RouteDecisionKind,
                                            >>()
                                            .expect("route control preview");
                                        drop(flow);

                                        assert_eq!(
                                            route_resolver_calls(),
                                            route_policy_calls_before,
                                            "dropping flow preview must not invoke route resolver",
                                        );
                                        assert_eq!(
                                            shared_ref.state.with(|state| state.drain_calls),
                                            drain_calls_before,
                                            "dropping flow preview must not flush transport events",
                                        );
                                        assert_eq!(
                                            count_policy_audit_ext_for_slot(
                                                tap_events(),
                                                SLOT_TAG_ROUTE,
                                            ),
                                            route_audit_before,
                                            "dropping flow preview must not emit route-slot policy audit",
                                        );

                                        futures::executor::block_on(async {
                                            let _token = controller
                                                .flow::<Msg<
                                                    { LABEL_ROUTE_DECISION },
                                                    GenericCapToken<RouteDecisionKind>,
                                                    RouteDecisionKind,
                                                >>()
                                                .expect("route control preview for send")
                                                .send(())
                                                .await
                                                .expect("send route control");
                                        });
                                    },
                                )
                            },
                        )
                    },
                );
            },
        );
    });
}

#[test]
fn offer_decode_binding_consumes_evidence_once() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config = Config::new(tap_buf, slab);
                with_tls_mut(
                    &FLOW_SHARED_SLOT,
                    |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                    |shared| {
                        shared.reset();
                        let shared_ref: &'static FlowBindingShared = shared;
                        let transport = FlowTransport::new(shared_ref);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
                        register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                        let sid = SessionId::new(901);
                        with_tls_mut(
                            &CONTROLLER_BINDING_SLOT,
                            |ptr: *mut FlowBinding| unsafe {
                                ptr.write(FlowBinding::new(0, shared_ref))
                            },
                            |controller_binding| {
                                with_tls_mut(
                                    &WORKER_BINDING_SLOT,
                                    |ptr: *mut FlowBinding| unsafe {
                                        ptr.write(FlowBinding::new(1, shared_ref))
                                    },
                                    |worker_binding| {
                                        with_tls_mut(
                                            &CONTROLLER_ENDPOINT_SLOT,
                                            |ptr| unsafe {
                                                write_value(
                                                    ptr,
                                                    cluster
                                                        .enter(
                                                            rv_id,
                                                            sid,
                                                            &controller_program(),
                                                            controller_binding,
                                                        )
                                                        .expect("attach controller"),
                                                );
                                            },
                                            |controller| {
                                                with_tls_mut(
                                                    &WORKER_ENDPOINT_SLOT,
                                                    |ptr| unsafe {
                                                        write_value(
                                                            ptr,
                                                            cluster
                                                                .enter(
                                                                    rv_id,
                                                                    sid,
                                                                    &worker_program(),
                                                                    worker_binding,
                                                                )
                                                                .expect("attach worker"),
                                                        );
                                                    },
                                                    |worker| {
                                                        futures::executor::block_on(async move {
                                                            let _token = controller
                                                                .flow::<Msg<
                                                                    { LABEL_ROUTE_DECISION },
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            let _outcome = controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&4444)
                                                                .await
                                                                .expect("send left data");

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            let data_value = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect("decode left data");
                                                            assert_eq!(data_value, 4444);

                                                            let _outcome = controller
                                                                .flow::<Msg<73, u32>>()
                                                                .expect("tail flow")
                                                                .send(&55)
                                                                .await
                                                                .expect("send tail");

                                                            let tail = worker
                                                                .recv::<Msg<73, u32>>()
                                                                .await
                                                                .expect(
                                                                    "recv tail after offer/decode",
                                                                );
                                                            assert_eq!(tail, 55);
                                                        })
                                                    },
                                                )
                                            },
                                        )
                                    },
                                )
                            },
                        )
                    },
                );
            },
        );
    });
}

#[test]
fn drop_public_preview_branch_preserves_offer_progression() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let tap_ptr = tap_buf.as_ptr();
                let tap_len = tap_buf.len();
                let tap_events = || unsafe { core::slice::from_raw_parts(tap_ptr, tap_len) };
                let config = Config::new(tap_buf, slab);
                with_tls_mut(
                    &FLOW_SHARED_SLOT,
                    |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                    |shared| {
                        shared.reset();
                        let shared_ref: &'static FlowBindingShared = shared;
                        let transport = FlowTransport::new(shared_ref);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
                        register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                        let sid = SessionId::new(903);
                        with_tls_mut(
                            &CONTROLLER_BINDING_SLOT,
                            |ptr: *mut FlowBinding| unsafe {
                                ptr.write(FlowBinding::new(0, shared_ref))
                            },
                            |controller_binding| {
                                with_tls_mut(
                                    &WORKER_BINDING_SLOT,
                                    |ptr: *mut FlowBinding| unsafe {
                                        ptr.write(FlowBinding::new(1, shared_ref))
                                    },
                                    |worker_binding| {
                                        with_tls_mut(
                                            &CONTROLLER_ENDPOINT_SLOT,
                                            |ptr| unsafe {
                                                write_value(
                                                    ptr,
                                                    cluster
                                                        .enter(
                                                            rv_id,
                                                            sid,
                                                            &controller_program(),
                                                            controller_binding,
                                                        )
                                                        .expect("attach controller"),
                                                );
                                            },
                                            |controller| {
                                                with_tls_mut(
                                                    &WORKER_ENDPOINT_SLOT,
                                                    |ptr| unsafe {
                                                        write_value(
                                                            ptr,
                                                            cluster
                                                                .enter(
                                                                    rv_id,
                                                                    sid,
                                                                    &worker_program(),
                                                                    worker_binding,
                                                                )
                                                                .expect("attach worker"),
                                                        );
                                                    },
                                                    |worker| {
                                                        futures::executor::block_on(async move {
                                                            let _token = controller
                                                                .flow::<Msg<
                                                                    { LABEL_ROUTE_DECISION },
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            let _outcome = controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&4444)
                                                                .await
                                                                .expect("send left data");

                                                            let endpoint_rx_audit_before =
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                );
                                                            let drain_calls_before = shared_ref
                                                                .state
                                                                .with(|state| state.drain_calls);

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "offer preview must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "offer preview must not flush transport events for EndpointRx policy",
                                                            );
                                                            drop(worker_branch);
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "dropping preview branch must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "dropping preview branch must not flush transport events",
                                                            );

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect(
                                                                    "re-offer left arm after dropped preview",
                                                                );
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "re-offer preview must stay policy-free until decode",
                                                            );
                                                            let data_value = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect(
                                                                    "decode left data after dropped preview",
                                                                );
                                                            assert_eq!(data_value, 4444);
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before + 1,
                                                                "decode consume path must emit EndpointRx policy audit once",
                                                            );
                                                            assert!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls)
                                                                    > drain_calls_before,
                                                                "decode consume path must own transport-event flushing",
                                                            );

                                                            let _outcome = controller
                                                                .flow::<Msg<73, u32>>()
                                                                .expect("tail flow")
                                                                .send(&55)
                                                                .await
                                                                .expect("send tail");

                                                            let tail = worker
                                                                .recv::<Msg<73, u32>>()
                                                                .await
                                                                .expect(
                                                                    "recv tail after dropped preview branch",
                                                                );
                                                            assert_eq!(tail, 55);
                                                        })
                                                    },
                                                )
                                            },
                                        )
                                    },
                                )
                            },
                        )
                    },
                );
            },
        );
    });
}

#[test]
fn codec_error_in_public_decode_preserves_preview_branch() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let tap_ptr = tap_buf.as_ptr();
                let tap_len = tap_buf.len();
                let tap_events = || unsafe { core::slice::from_raw_parts(tap_ptr, tap_len) };
                let config = Config::new(tap_buf, slab);
                with_tls_mut(
                    &FLOW_SHARED_SLOT,
                    |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                    |shared| {
                        shared.reset();
                        let shared_ref: &'static FlowBindingShared = shared;
                        let transport = FlowTransport::new(shared_ref);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
                        register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                        let sid = SessionId::new(904);
                        with_tls_mut(
                            &CONTROLLER_BINDING_SLOT,
                            |ptr: *mut FlowBinding| unsafe {
                                ptr.write(FlowBinding::new(0, shared_ref))
                            },
                            |controller_binding| {
                                with_tls_mut(
                                    &WORKER_BINDING_SLOT,
                                    |ptr: *mut FlowBinding| unsafe {
                                        ptr.write(FlowBinding::new(1, shared_ref))
                                    },
                                    |worker_binding| {
                                        with_tls_mut(
                                            &CONTROLLER_ENDPOINT_SLOT,
                                            |ptr| unsafe {
                                                write_value(
                                                    ptr,
                                                    cluster
                                                        .enter(
                                                            rv_id,
                                                            sid,
                                                            &controller_program(),
                                                            controller_binding,
                                                        )
                                                        .expect("attach controller"),
                                                );
                                            },
                                            |controller| {
                                                with_tls_mut(
                                                    &WORKER_ENDPOINT_SLOT,
                                                    |ptr| unsafe {
                                                        write_value(
                                                            ptr,
                                                            cluster
                                                                .enter(
                                                                    rv_id,
                                                                    sid,
                                                                    &worker_program(),
                                                                    worker_binding,
                                                                )
                                                                .expect("attach worker"),
                                                        );
                                                    },
                                                    |worker| {
                                                        futures::executor::block_on(async move {
                                                            let _token = controller
                                                                .flow::<Msg<
                                                                    { LABEL_ROUTE_DECISION },
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            let _outcome = controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&4444)
                                                                .await
                                                                .expect("send left data");

                                                            let endpoint_rx_audit_before =
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                );
                                                            let drain_calls_before = shared_ref
                                                                .state
                                                                .with(|state| state.drain_calls);

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "offer preview must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "offer preview must not flush transport events",
                                                            );
                                                            let err = worker_branch
                                                                .decode::<Msg<71, u64>>()
                                                                .await
                                                                .expect_err(
                                                                    "codec mismatch must fail without consuming preview",
                                                                );
                                                            assert!(
                                                                matches!(
                                                                    err,
                                                                    hibana::RecvError::Codec(_)
                                                                ),
                                                                "expected codec error, got {err:?}"
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "codec error must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "codec error must not flush transport events for EndpointRx policy",
                                                            );

                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "codec error must leave retry state policy-free until a successful decode",
                                                            );
                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect(
                                                                    "re-offer left arm after codec mismatch",
                                                                );
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            let data_value = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect(
                                                                    "decode left data after retrying from the endpoint",
                                                                );
                                                            assert_eq!(data_value, 4444);
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before + 1,
                                                                "successful decode must emit EndpointRx policy audit once",
                                                            );
                                                            assert!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls)
                                                                    > drain_calls_before,
                                                                "successful decode must own transport-event flushing",
                                                            );

                                                            let _outcome = controller
                                                                .flow::<Msg<73, u32>>()
                                                                .expect("tail flow")
                                                                .send(&55)
                                                                .await
                                                                .expect("send tail");

                                                            let tail = worker
                                                                .recv::<Msg<73, u32>>()
                                                                .await
                                                                .expect(
                                                                    "recv tail after codec-error retry",
                                                                );
                                                            assert_eq!(tail, 55);
                                                        })
                                                    },
                                                )
                                            },
                                        )
                                    },
                                )
                            },
                        )
                    },
                );
            },
        );
    });
}

#[test]
fn dynamic_route_passive_ignores_non_authoritative_binding_evidence() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config = Config::new(tap_buf, slab);
                with_tls_mut(
                    &FLOW_SHARED_SLOT,
                    |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                    |shared| {
                        shared.reset();
                        let shared_ref: &'static FlowBindingShared = shared;
                        let transport = FlowTransport::new(shared_ref);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
                        register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                        let sid = SessionId::new(902);
                        with_tls_mut(
                            &CONTROLLER_BINDING_SLOT,
                            |ptr: *mut FlowBinding| unsafe {
                                ptr.write(FlowBinding::new(0, shared_ref))
                            },
                            |controller_binding| {
                                with_tls_mut(
                                    &WORKER_BINDING_SLOT,
                                    |ptr: *mut FlowBinding| unsafe {
                                        ptr.write(FlowBinding::new(1, shared_ref))
                                    },
                                    |worker_binding| {
                                        with_tls_mut(
                                            &CONTROLLER_ENDPOINT_SLOT,
                                            |ptr| unsafe {
                                                write_value(
                                                    ptr,
                                                    cluster
                                                        .enter(
                                                            rv_id,
                                                            sid,
                                                            &controller_program(),
                                                            controller_binding,
                                                        )
                                                        .expect("attach controller"),
                                                );
                                            },
                                            |controller| {
                                                with_tls_mut(
                                                    &WORKER_ENDPOINT_SLOT,
                                                    |ptr| unsafe {
                                                        write_value(
                                                            ptr,
                                                            cluster
                                                                .enter(
                                                                    rv_id,
                                                                    sid,
                                                                    &worker_program(),
                                                                    worker_binding,
                                                                )
                                                                .expect("attach worker"),
                                                        );
                                                    },
                                                    |worker| {
                                                        futures::executor::block_on(async move {
                                                            shared_ref.state.with_mut(|guard| {
                                                                guard.push_incoming(
                                                                    1,
                                                                    PendingInbound {
                                                                        lane: 0,
                                                                        evidence:
                                                                            IngressEvidence {
                                                                                label: <Msg<72, u32> as MessageSpec>::LABEL,
                                                                                instance: 0,
                                                                                has_fin: false,
                                                                                channel: Channel::new(999),
                                                                            },
                                                                    },
                                                                );
                                                            });

                                                            let _token = controller
                                                                .flow::<Msg<
                                                                    { LABEL_ROUTE_DECISION },
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            let _outcome = controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&7777)
                                                                .await
                                                                .expect("send left data");

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LABEL
                                                            );
                                                            let value = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect("decode left data");
                                                            assert_eq!(value, 7777);

                                                            let _outcome = controller
                                                                .flow::<Msg<73, u32>>()
                                                                .expect("tail flow")
                                                                .send(&1)
                                                                .await
                                                                .expect("send tail");
                                                        })
                                                    },
                                                )
                                            },
                                        )
                                    },
                                )
                            },
                        )
                    },
                );
            },
        );
    });
}
