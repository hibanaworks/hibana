#![cfg(feature = "std")]
mod common;
#[path = "support/local_only.rs"]
mod local_only_support;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use common::{TestRx, TestTransport, TestTransportError, TestTx};
use core::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
};
use hibana::g::{self, Msg, Role};
use hibana::integration::program::{MessageSpec, RoleProgram, project};
use hibana::integration::{
    SessionKit, SessionKitStorage,
    binding::{BindingError, Channel, EndpointSlot, IngressEvidence},
    ids::SessionId,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
    transport::{FrameLabel, Outgoing, Transport},
};
use hibana::integration::{
    cap::control::RouteDecisionKind,
    ids::RendezvousId,
    policy::{DecisionArm, DecisionResolution, ResolverError},
};
use local_only_support::LocalCell;
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_resident_tls_ref;

const TEST_ROUTE_DECISION_LOGICAL: u8 = 0xA3;
const ROUTE_RIGHT_CONTROL_LOGICAL: u8 = 119;
const POLICY_AUDIT_EXT_ID: u16 = 0x0408;
const SLOT_TAG_ENDPOINT_RX: u32 = 1;
const SLOT_TAG_ROUTE: u32 = 4;

const ROUTE_POLICY_ID: u16 = 900;
type TestKitStorage =
    SessionKitStorage<'static, FlowTransport, DefaultLabelUniverse, CounterClock, 2>;
const FOREIGN_BINDING_FRAME: u8 = 250;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
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
    tap_buf: &[hibana::integration::runtime::TapEvent],
    slot_tag: u32,
) -> usize {
    tap_buf
        .iter()
        .filter(|event| event.id == POLICY_AUDIT_EXT_ID && (event.arg2 >> 24) == slot_tag)
        .count()
}

fn controller_program() -> RoleProgram<0> {
    let left_arm =
        g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
            g::send::<Role<0>, Role<1>, Msg<71, u32>, 0>(),
        );
    let right_arm = g::seq(
        g::send::<Role<0>, Role<0>, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<72, u32>, 0>(),
    );
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<Role<0>, Role<1>, Msg<73, u32>, 0>());
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let left_arm =
        g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
            g::send::<Role<0>, Role<1>, Msg<71, u32>, 0>(),
        );
    let right_arm = g::seq(
        g::send::<Role<0>, Role<0>, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
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

    fn clear_payloads(&mut self) {
        self.payloads = [StoredPayload::default(); FLOW_MAX_PAYLOADS];
    }

    fn take_payload(&mut self, channel: u64, buf: &mut [u8]) -> Result<usize, BindingError> {
        let mut idx = 0usize;
        while idx < self.payloads.len() {
            let slot = &mut self.payloads[idx];
            if slot.active && slot.channel == channel {
                if slot.len > buf.len() {
                    return Err(BindingError::ReadFailed);
                }
                buf[..slot.len].copy_from_slice(&slot.bytes[..slot.len]);
                let len = slot.len;
                *slot = StoredPayload::default();
                return Ok(len);
            }
            idx += 1;
        }
        Err(BindingError::ChannelUnavailable)
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

impl EndpointSlot for FlowBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence> {
        self.shared
            .state
            .with_mut(|state| state.take_incoming_for_lane(self.role, logical_lane))
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<hibana::integration::wire::Payload<'a>, BindingError> {
        let len = self
            .shared
            .state
            .with_mut(|state| state.take_payload(channel.raw(), buf))?;
        Ok(hibana::integration::wire::Payload::new(&buf[..len]))
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

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        if !outgoing.is_control() && outgoing.peer() == 1 {
            self.shared.state.with_mut(|shared| {
                let channel = Channel::new(shared.next_channel);
                shared.next_channel += 1;
                shared.store_payload(channel.raw(), outgoing.payload().as_bytes());
                let evidence = IngressEvidence {
                    frame_label: outgoing.frame_label(),
                    instance: 0,
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
    ) -> std::task::Poll<Result<hibana::integration::wire::Payload<'a>, Self::Error>> {
        self.inner.poll_recv(rx, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        self.inner.requeue(rx)
    }

    fn recv_frame_hint<'a>(
        &self,
        rx: &mut Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.inner.recv_frame_hint(rx)
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
        .rendezvous(rv_id)
        .role(program)
        .set_resolver::<ROUTE_POLICY_ID>(hibana::integration::policy::ResolverRef::decision_fn(
            always_left_route_resolver,
        ))
        .expect("register decision resolver");
}

fn always_left_route_resolver() -> Result<DecisionResolution, ResolverError> {
    ROUTE_RESOLVER_CALLS.with(|count| count.set(count.get().wrapping_add(1)));
    Ok(DecisionResolution::Arm(DecisionArm::Left))
}

#[path = "offer_decode_binding_regression/decode_lifecycle.rs"]
mod decode_lifecycle;
#[path = "offer_decode_binding_regression/diagnostics_dynamic.rs"]
mod diagnostics_dynamic;
#[path = "offer_decode_binding_regression/flow_preview.rs"]
mod flow_preview;
