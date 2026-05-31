use super::*;
extern crate self as hibana;
use crate::control::cap::atomic_codecs::{
    TAG_TOPOLOGY_BEGIN_CONTROL, encode_session_lane_handle, mint_session_lane_handle,
};

use crate::test_support::large_choreography::{
    fanout_program, huge_program, linear_program, localside,
};

use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN, CapHeader, CapShot,
    GenericCapToken, LocalControlKind,
};
use crate::control::cap::resource_kinds::{RouteArmHandle, RouteDecisionKind};
use crate::control::types::{Generation, Lane, SessionId};
use crate::g::Program;
use crate::g::{self, Msg};
use crate::global::compiled::lowering::CompiledProgramImage;
use crate::global::role_program;
use crate::observe::core::TapEvent;
use crate::runtime::config::{Config, CounterClock};
use crate::runtime::consts::{DefaultLabelUniverse, RING_EVENTS};
use crate::transport::{Transport, TransportError, wire::Payload};
use core::mem::size_of;
use core::{cell::UnsafeCell, mem::MaybeUninit};
use std::thread_local;

const TEST_ROUTE_DECISION_LOGICAL: u8 = 0xA3;

fn token_wire_image(
    nonce: [u8; CAP_NONCE_LEN],
    header: [u8; CAP_HEADER_LEN],
) -> [u8; CAP_TOKEN_LEN] {
    let mut bytes = [0u8; CAP_TOKEN_LEN];
    bytes[..CAP_NONCE_LEN].copy_from_slice(&nonce);
    bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
    bytes
}

#[test]
fn resolver_ref_decision_state_dispatches_borrowed_state() {
    #[derive(Clone, Copy)]
    struct DecisionState {
        preferred_arm: DecisionArm,
    }

    fn decision_resolver(state: &DecisionState) -> Result<DecisionResolution, ResolverError> {
        Ok(DecisionResolution::Arm(state.preferred_arm))
    }

    let state = DecisionState {
        preferred_arm: DecisionArm::Right,
    };
    let resolver = ResolverRef::decision_state(&state, decision_resolver);

    assert_eq!(
        resolver.resolve_decision(),
        Ok(DecisionResolution::Arm(DecisionArm::Right))
    );
}

type SharedBorrowLeft =
    g::Send<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>;
type SharedBorrowRight =
    g::Send<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL + 1 }, (), RouteDecisionKind>, 0>;

type SharedBorrowPolicyProgram<const POLICY_ID: u16> = Program<
    g::Route<g::Policy<SharedBorrowLeft, POLICY_ID>, g::Policy<SharedBorrowRight, POLICY_ID>>,
>;
type SharedBorrowRoleProgram = crate::integration::program::RoleProgram<0>;

const ROUTE_POLICY_ONE: u16 = 9901;
const ROUTE_POLICY_TWO: u16 = 9902;

fn decision_policy_program_one() -> SharedBorrowPolicyProgram<ROUTE_POLICY_ONE> {
    g::route(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ONE>(),
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL + 1 }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ONE>(),
    )
}
fn decision_policy_program_two() -> SharedBorrowPolicyProgram<ROUTE_POLICY_TWO> {
    g::route(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_TWO>(),
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL + 1 }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_TWO>(),
    )
}
// Minimal transport used by resident runtime validation.
struct DummyTransport;

impl Transport for DummyTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        core::task::Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Failed))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn recv_frame_hint<'a>(&self, _rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        None
    }
}

fn retain_large_choreography_fixture_symbols() {
    let _ = fanout_program::ROUTE_SCOPE_COUNT;
    let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = fanout_program::ACK_LABELS;
    let _ = huge_program::ROUTE_SCOPE_COUNT;
    let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = huge_program::ACK_LABELS;
    let _ = linear_program::ROUTE_SCOPE_COUNT;
    let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = linear_program::ACK_LABELS;
    let _ = huge_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = huge_program::controller_program as fn() -> role_program::RoleProgram<0>;
    let _ = linear_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = linear_program::controller_program as fn() -> role_program::RoleProgram<0>;
    let _ = fanout_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = fanout_program::controller_program as fn() -> role_program::RoleProgram<0>;
    let _ = localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
}

#[test]
fn large_choreography_fixture_symbols_are_reachable() {
    retain_large_choreography_fixture_symbols();
}

fn route_decision_header(scope_id: u16, epoch: u16, flags: u8) -> (ControlDesc, CapHeader) {
    let desc = ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
        RouteDecisionKind,
    >());
    let handle = RouteArmHandle::new(1).expect("binary route decision arm");
    (
        desc,
        CapHeader::new(
            SessionId::new(7),
            Lane::new(0),
            0,
            desc.resource_tag(),
            desc.op(),
            desc.path(),
            desc.shot(),
            desc.scope_kind(),
            flags,
            scope_id,
            epoch,
            handle.encode(),
        ),
    )
}

struct LocalAbortAckControl;

impl LocalControlKind for LocalAbortAckControl {
    const TAG: u8 = 0xA0;
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const TAP_ID: u16 = crate::observe::ids::ABORT_ACK;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::AbortAck;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}

struct LocalStateSnapshotControl;

impl LocalControlKind for LocalStateSnapshotControl {
    const TAG: u8 = 0xA5;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::STATE_SNAPSHOT_REQ;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::StateSnapshot;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}

struct LocalStateRestoreControl;

impl LocalControlKind for LocalStateRestoreControl {
    const TAG: u8 = 0xA1;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::STATE_RESTORE_REQ;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::StateRestore;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}

struct LocalTxCommitControl;

impl LocalControlKind for LocalTxCommitControl {
    const TAG: u8 = 0xA2;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::POLICY_COMMIT;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::TxCommit;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}

struct LocalTxAbortControl;

impl LocalControlKind for LocalTxAbortControl {
    const TAG: u8 = 0xA3;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::POLICY_TX_ABORT;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::TxAbort;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}

type AttachRoleProgram = crate::integration::program::RoleProgram<0>;
fn attach_program() -> AttachRoleProgram {
    role_program::project(&g::send::<0, 1, Msg<0x41, u8>, 0>())
}

type Lane1WorkerRoleProgram = crate::integration::program::RoleProgram<1>;
fn lane1_worker_program() -> Lane1WorkerRoleProgram {
    role_program::project(&g::send::<1, 0, Msg<0x42, u8>, 1>())
}

fn attach_session_lane_for_program<const ROLE: u8, const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    sid: SessionId,
    program: &crate::integration::program::RoleProgram<ROLE>,
) -> ((EndpointLeaseId, u32), Lane) {
    let handle = cluster
        .enter(
            rv_id,
            sid,
            program,
            crate::binding::BindingHandle::None(crate::binding::NoBinding),
        )
        .expect("attach test endpoint");
    let lane = cluster
        .get_local(&rv_id)
        .expect("registered rendezvous")
        .session_lane(sid)
        .expect("attached session must own a lane");
    ((handle.0, handle.1), lane)
}

fn attach_session_lane<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    sid: SessionId,
) -> ((EndpointLeaseId, u32), Lane) {
    attach_session_lane_for_program(cluster, rv_id, sid, &attach_program())
}

fn topology_handle(
    operands: TopologyOperands,
) -> crate::control::cap::atomic_codecs::TopologyHandle {
    crate::control::cap::atomic_codecs::TopologyHandle {
        src_rv: operands.src_rv.raw(),
        dst_rv: operands.dst_rv.raw(),
        src_lane: operands.src_lane.raw() as u16,
        dst_lane: operands.dst_lane.raw() as u16,
        old_gen: operands.old_gen.raw(),
        new_gen: operands.new_gen.raw(),
        seq_tx: operands.seq_tx,
        seq_rx: operands.seq_rx,
    }
}

fn prepare_topology_publication_at<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    target: RendezvousId,
    op: ControlOp,
    sid: SessionId,
    operands: TopologyOperands,
) -> Result<DescriptorTerminal, CpError> {
    cluster.prepare_topology_descriptor_terminal(target, op, sid, operands)
}

fn publish_topology_publication_at<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    target: RendezvousId,
    op: ControlOp,
    sid: SessionId,
    operands: TopologyOperands,
) -> Result<(), CpError> {
    let ticket = prepare_topology_publication_at(cluster, target, op, sid, operands)?;
    cluster.publish_descriptor_terminal(ticket);
    Ok(())
}

fn publish_topology_begin_at<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    target: RendezvousId,
    sid: SessionId,
    operands: TopologyOperands,
) -> Result<(), CpError> {
    publish_topology_publication_at(cluster, target, ControlOp::TopologyBegin, sid, operands)
}

fn publish_topology_commit_at<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    target: RendezvousId,
    sid: SessionId,
    operands: TopologyOperands,
) -> Result<(), CpError> {
    publish_topology_publication_at(cluster, target, ControlOp::TopologyCommit, sid, operands)
}

fn publish_topology_ack_handle<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    target: RendezvousId,
    sid: SessionId,
    lane: Lane,
    handle: crate::control::cap::atomic_codecs::TopologyHandle,
    generation: Option<Generation>,
) -> Result<(), CpError> {
    let descriptor = TopologyDescriptor::decode_for(ControlOp::TopologyAck, handle.encode())?;
    let operands =
        cluster.validate_topology_ack_operands(target, lane, descriptor.operands(), generation)?;
    publish_topology_publication_at(cluster, target, ControlOp::TopologyAck, sid, operands)
}

fn advance_lane_generation<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    lane: Lane,
    target: Generation,
) {
    cluster
        .get_local(&rv_id)
        .expect("registered rendezvous")
        .advance_lane_generation_to(lane, target);
}

fn session_lane_control_token_with_epoch<K: LocalControlKind>(
    sid: SessionId,
    lane: Lane,
    epoch: u16,
) -> [u8; CAP_TOKEN_LEN] {
    let desc = ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<K>());
    let handle = K::encode_local_handle(sid, lane, ScopeId::none());
    let mut header = [0u8; CAP_HEADER_LEN];
    CapHeader::new(
        sid,
        lane,
        0,
        desc.resource_tag(),
        desc.op(),
        desc.path(),
        desc.shot(),
        desc.scope_kind(),
        desc.header_flags(),
        0,
        epoch,
        handle,
    )
    .encode(&mut header);
    token_wire_image([0; CAP_NONCE_LEN], header)
}

fn prepare_descriptor_commit<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    bytes: [u8; CAP_TOKEN_LEN],
    desc: ControlDesc,
    expected_epoch: u16,
) -> Result<DescriptorTerminal, CpError> {
    let token = GenericCapToken::<()>::from_raw_bytes(bytes);
    let header = token.control_header().map_err(|_| CpError::Authorisation {
        operation: desc.op() as u8,
    })?;
    cluster.prepare_send_bound_descriptor_terminal(
        rv_id,
        bytes,
        desc,
        header.sid(),
        header.lane(),
        header.role(),
        0,
        expected_epoch,
    )
}

fn dispatch_prepared_descriptor_commit<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    bytes: [u8; CAP_TOKEN_LEN],
    desc: ControlDesc,
    expected_epoch: u16,
) -> Result<(), CpError> {
    let ticket = prepare_descriptor_commit(cluster, rv_id, bytes, desc, expected_epoch)?;
    cluster.publish_descriptor_terminal(ticket);
    Ok(())
}

struct DecodePoisonKind;

impl LocalControlKind for DecodePoisonKind {
    const TAG: u8 = 0x7C;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = 0x047C;
    const SHOT: crate::control::cap::mint::CapShot = crate::control::cap::mint::CapShot::One;
    const OP: ControlOp = ControlOp::Fence;

    fn encode_local_handle(
        _session: SessionId,
        _lane: Lane,
        _scope: ScopeId,
    ) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }
}

#[path = "tests/descriptor_headers.rs"]
mod descriptor_headers;

type StaticTestCluster<const MAX_RV: usize> =
    SessionCluster<'static, DummyTransport, DefaultLabelUniverse, CounterClock, MAX_RV>;

const CLUSTER_TEST_SLAB_CAPACITY: usize = 262_144;
#[path = "tests/resident_shape.rs"]
mod resident_shape;
struct ClusterRuntimeGuard {
    tap0: *mut [TapEvent; RING_EVENTS],
    tap1: *mut [TapEvent; RING_EVENTS],
    slab0: *mut [u8; CLUSTER_TEST_SLAB_CAPACITY],
    slab1: *mut [u8; CLUSTER_TEST_SLAB_CAPACITY],
    clock: *const CounterClock,
}

thread_local! {
    static CLUSTER_TAP0: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static CLUSTER_TAP1: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static CLUSTER_SLAB0: UnsafeCell<[u8; CLUSTER_TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; CLUSTER_TEST_SLAB_CAPACITY]) };
    static CLUSTER_SLAB1: UnsafeCell<[u8; CLUSTER_TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; CLUSTER_TEST_SLAB_CAPACITY]) };
    static CLUSTER_SLOT_1: UnsafeCell<MaybeUninit<StaticTestCluster<1>>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static CLUSTER_SLOT_2: UnsafeCell<MaybeUninit<StaticTestCluster<2>>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static CLUSTER_TEST_CLOCK: CounterClock = const { CounterClock::new() };
}

fn with_cluster_runtime<R>(f: impl FnOnce(&mut ClusterRuntimeGuard) -> R) -> R {
    CLUSTER_TAP0.with(|tap0| {
        CLUSTER_TAP1.with(|tap1| {
            CLUSTER_SLAB0.with(|slab0| {
                CLUSTER_SLAB1.with(|slab1| {
                    CLUSTER_TEST_CLOCK.with(|clock| unsafe {
                        let tap0 = &mut *tap0.get();
                        tap0.fill(TapEvent::zero());
                        let tap1 = &mut *tap1.get();
                        tap1.fill(TapEvent::zero());
                        let slab0 = &mut *slab0.get();
                        slab0.fill(0);
                        let slab1 = &mut *slab1.get();
                        slab1.fill(0);
                        let mut fixture = ClusterRuntimeGuard {
                            tap0,
                            tap1,
                            slab0,
                            slab1,
                            clock: clock as *const CounterClock,
                        };
                        f(&mut fixture)
                    })
                })
            })
        })
    })
}

impl ClusterRuntimeGuard {
    fn config0(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = unsafe { &mut *self.tap0 };
        let slab = unsafe { &mut *self.slab0 };
        Config::from_resources((tap, slab), CounterClock::new())
    }

    fn config1(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = unsafe { &mut *self.tap1 };
        let slab = unsafe { &mut *self.slab1 };
        Config::from_resources((tap, slab), CounterClock::new())
    }

    fn clock(&self) -> &'static CounterClock {
        unsafe { &*self.clock }
    }
}

fn with_cluster_fixture<R>(
    f: impl FnOnce(&'static CounterClock, Config<'static, DefaultLabelUniverse, CounterClock>) -> R,
) -> R {
    with_cluster_runtime(|fixture| {
        let config = fixture.config0();
        f(fixture.clock(), config)
    })
}

fn with_cluster_fixture_pair<R>(
    f: impl FnOnce(
        &'static CounterClock,
        Config<'static, DefaultLabelUniverse, CounterClock>,
        Config<'static, DefaultLabelUniverse, CounterClock>,
    ) -> R,
) -> R {
    with_cluster_runtime(|fixture| {
        let config0 = fixture.config0();
        let config1 = fixture.config1();
        f(fixture.clock(), config0, config1)
    })
}

fn with_test_cluster_1<R>(
    _clock: &'static CounterClock,
    f: impl FnOnce(&'static StaticTestCluster<1>) -> R,
) -> R {
    CLUSTER_SLOT_1.with(|slot| unsafe {
        let ptr = (*slot.get()).as_mut_ptr();
        SessionCluster::init_empty(ptr);
        let result = f(&*ptr);
        core::ptr::drop_in_place(ptr);
        result
    })
}

fn with_test_cluster_2<R>(
    _clock: &'static CounterClock,
    f: impl FnOnce(&'static StaticTestCluster<2>) -> R,
) -> R {
    CLUSTER_SLOT_2.with(|slot| unsafe {
        let ptr = (*slot.get()).as_mut_ptr();
        SessionCluster::init_empty(ptr);
        let result = f(&*ptr);
        core::ptr::drop_in_place(ptr);
        result
    })
}

unsafe fn drop_test_public_endpoint_for_role<const ROLE: u8, const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    handle: (crate::rendezvous::core::EndpointLeaseId, u32),
) {
    if let Some(header) = cluster.public_endpoint_header_ptr(rv_id, handle.0, handle.1) {
        let packed = crate::endpoint::carrier::PackedEndpointHandle::new(rv_id, handle.0, handle.1);
        let ops = unsafe { header.as_ref().ops() };
        unsafe {
            (ops.drop_endpoint)(header.cast(), packed);
        }
    }
}

unsafe fn drop_test_public_endpoint<const MAX_RV: usize>(
    cluster: &'static StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    handle: (crate::rendezvous::core::EndpointLeaseId, u32),
) {
    unsafe {
        drop_test_public_endpoint_for_role::<0, MAX_RV>(cluster, rv_id, handle);
    }
}

fn run_on_transient_compiled_test_stack<F>(name: &'static str, test: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = name;
    test();
}

fn route_resolver() -> Result<DecisionResolution, ResolverError> {
    Ok(DecisionResolution::Arm(DecisionArm::Left))
}

#[path = "tests/topology.rs"]
mod topology;
