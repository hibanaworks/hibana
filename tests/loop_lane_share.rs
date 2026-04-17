#![cfg(feature = "std")]

//! Test that loop control (self-send) operates via flow().send() pattern.
//!
//! Per AGENTS.md Branch Patterns:
//! - Pattern A (Explicit Decision): Controller uses flow().send() for loop Continue/Break
//! - The self-send records the decision in RouteTable
//! - Target (passive observer) uses offer() to observe the arm via cross-role messages

mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::advanced::steps::SeqSteps,
    g::advanced::steps::{PolicySteps, RouteSteps, SendStep, StepCons, StepNil},
    g::advanced::{CanonicalControl, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        RendezvousId,
        cap::{
            GenericCapToken,
            advanced::{LoopBreakKind, LoopContinueKind},
        },
        policy::{DynamicResolution, ResolverContext, ResolverError},
    },
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        tap::TapEvent,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;
const LOOP_POLICY_ID: u16 = 99;
type LoopContinueHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopBreakHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopContinueArmSteps =
    SeqSteps<LoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>>;
type LoopBreakArmSteps =
    SeqSteps<LoopBreakHead, StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>>;
type LoopSegmentSteps = RouteSteps<LoopContinueArmSteps, LoopBreakArmSteps>;
type LoopLaneProgramSteps =
    SeqSteps<StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>, LoopSegmentSteps>;
type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type ControllerEndpoint = hibana::Endpoint<'static, 0, TestKit>;
type TargetEndpoint = hibana::Endpoint<'static, 1, TestKit>;

std::thread_local! {
    static LOOP_DECISION_INDEX: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_SLOT: UnsafeCell<MaybeUninit<ControllerEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static TARGET_SLOT: UnsafeCell<MaybeUninit<TargetEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn loop_decision_index() -> usize {
    LOOP_DECISION_INDEX.with(core::cell::Cell::get)
}

fn set_loop_decision_index(value: usize) {
    LOOP_DECISION_INDEX.with(|cell| cell.set(value));
}

fn loop_lane_resolver(_ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    let decision = loop_decision_index() == 0;
    set_loop_decision_index(loop_decision_index() + 1);
    Ok(DynamicResolution::Loop { decision })
}

fn register_loop_lane_resolvers<const MAX_RV: usize>(
    cluster: &SessionKit<'_, TestTransport, DefaultLabelUniverse, CounterClock, MAX_RV>,
    rv_id: RendezvousId,
) {
    cluster
        .set_resolver::<LOOP_POLICY_ID, 0, _>(
            rv_id,
            &CONTROLLER_PROGRAM,
            hibana::substrate::policy::ResolverRef::from_fn(loop_lane_resolver),
        )
        .expect("register loop resolver");
}

const LOOP_BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<7, u32>, 0>();
const LOOP_EXIT: g::Program<StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>> =
    g::send::<Role<1>, Role<0>, Msg<8, i32>, 0>();

// Self-send for canonical control: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<LoopContinueArmSteps> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<LOOP_POLICY_ID>(),
    LOOP_BODY,
);
const LOOP_BREAK_ARM: g::Program<LoopBreakArmSteps> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>(),
    LOOP_EXIT,
);

// Route is local to Controller (0 → 0, self-send)
const LOOP_SEGMENT: g::Program<LoopSegmentSteps> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

const PROTOCOL: g::Program<LoopLaneProgramSteps> =
    g::seq(g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>(), LOOP_SEGMENT);

static CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&PROTOCOL);
static TARGET_PROGRAM: RoleProgram<'static, 1> = project(&PROTOCOL);

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn controller_send_handshake(controller: &mut ControllerEndpoint) {
    let _outcome = futures::executor::block_on(
        controller
            .flow::<Msg<10, ()>>()
            .expect("handshake flow")
            .send(&()),
    )
    .expect("handshake send");
}

fn target_recv_handshake(target: &mut TargetEndpoint) {
    let () = futures::executor::block_on(target.recv::<Msg<10, ()>>()).expect("handshake recv");
}

fn controller_send_continue(controller: &mut ControllerEndpoint) {
    let _outcome = futures::executor::block_on(
        controller
            .flow::<Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >>()
            .expect("continue flow")
            .send(()),
    )
    .expect("continue send");
}

fn controller_send_body(controller: &mut ControllerEndpoint) {
    let _outcome = futures::executor::block_on(
        controller
            .flow::<Msg<7, u32>>()
            .expect("loop body flow")
            .send(&1),
    )
    .expect("loop body send");
}

fn target_recv_body(target: &mut TargetEndpoint) {
    let branch = futures::executor::block_on(target.offer()).expect("target offer iteration 1");
    assert_eq!(
        branch.label(),
        7,
        "continue arm exposes BodyMsg recv to passive observer"
    );
    let first_body = futures::executor::block_on(branch.decode::<Msg<7, u32>>())
        .expect("decode body in continue arm");
    assert_eq!(first_body, 1);
}

fn controller_send_break(controller: &mut ControllerEndpoint) {
    let _outcome = futures::executor::block_on(
        controller
            .flow::<Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >>()
            .expect("break flow")
            .send(()),
    )
    .expect("break send");
}

fn target_send_exit(target: &mut TargetEndpoint) {
    let _outcome = futures::executor::block_on(
        target
            .flow::<Msg<8, i32>>()
            .expect("exit marker flow")
            .send(&0),
    )
    .expect("exit marker send");
}

fn controller_recv_exit(controller: &mut ControllerEndpoint) -> i32 {
    futures::executor::block_on(controller.recv::<Msg<8, i32>>()).expect("exit recv")
}

fn run_loop_lane_share(
    cluster: &'static TestKit,
    tap_buf: &'static mut [TapEvent; runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let config = Config::new(tap_buf, slab);
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");
    set_loop_decision_index(0);
    register_loop_lane_resolvers(cluster, rv_id);

    let sid = SessionId::new(9);
    with_tls_mut(
        &CONTROLLER_SLOT,
        |ptr| unsafe {
            write_value(
                ptr,
                cluster
                    .enter(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
                    .expect("controller attach"),
            );
        },
        |controller| {
            with_tls_mut(
                &TARGET_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .enter(rv_id, sid, &TARGET_PROGRAM, NoBinding)
                            .expect("target attach"),
                    );
                },
                |target| {
                    controller_send_handshake(controller);
                    target_recv_handshake(target);
                    controller_send_continue(controller);
                    controller_send_body(controller);
                    target_recv_body(target);
                    controller_send_break(controller);
                    target_send_exit(target);
                    let exit_value = controller_recv_exit(controller);
                    assert_eq!(exit_value, 0);

                    assert!(transport_queue_is_empty(transport));
                },
            );
        },
    );
}

/// Test that loop control operates via flow().send() pattern (Pattern A).
///
/// Per AGENTS.md Branch Patterns:
/// - Controller uses flow().send() to explicitly decide Continue/Break
/// - Target (passive observer) uses offer() to observe the selected arm
#[test]
fn loop_and_control_plane_tokens_share_lane() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| run_loop_lane_share(cluster, tap_buf, slab, &transport),
        );
    });
}
