#![cfg(feature = "std")]

//! Test that loop control (self-send) operates via flow().send() pattern.
//!
//! Per AGENTS.md Branch Patterns:
//! - Pattern A (Explicit Decision): Controller uses flow().send() for loop Continue/Break
//! - The self-send records the decision in RouteTable
//! - Target (passive observer) uses offer() to observe the arm via cross-role messages

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::TestTransport;
use hibana::{
    g::advanced::steps::SeqSteps,
    g::advanced::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil},
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
        SessionCluster, SessionId,
        binding::NoBinding,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;
const LOOP_POLICY_ID: u16 = 99;
static LOOP_DECISION_INDEX: AtomicUsize = AtomicUsize::new(0);

fn loop_lane_resolver(
    _cluster: &SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ResolverError> {
    let idx = LOOP_DECISION_INDEX.fetch_add(1, Ordering::Relaxed);
    let decision = idx == 0;
    Ok(DynamicResolution::Loop { decision })
}

fn register_loop_lane_resolvers(
    cluster: &SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    rv_id: RendezvousId,
) {
    cluster
        .set_resolver(
            rv_id,
            &CONTROLLER_PROGRAM,
            hibana::substrate::policy::PolicyId::new(LOOP_POLICY_ID),
            loop_lane_resolver,
        )
        .expect("register loop resolver");
}

const LOOP_BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<7, u32>, 0>();
const LOOP_EXIT: g::Program<StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>> =
    g::send::<Role<1>, Role<0>, Msg<8, i32>, 0>();

// Self-send for canonical control: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<
    SeqSteps<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
    >,
> = g::seq(
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
const LOOP_BREAK_ARM: g::Program<
    SeqSteps<
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
        StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>,
    >,
> = g::seq(
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
const LOOP_SEGMENT: g::Program<
    <SeqSteps<
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
        StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
    > as StepConcat<
        SeqSteps<
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
            StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>,
        >,
    >>::Output,
> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

const PROTOCOL: g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>,
        <SeqSteps<
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
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
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
                StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>,
            >,
        >>::Output,
    >,
> = g::seq(g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>(), LOOP_SEGMENT);

static CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <SeqSteps<
        StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>,
        <SeqSteps<
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
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
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
                StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>,
            >,
        >>::Output,
    > as ProjectRole<Role<0>>>::Output,
> = project(&PROTOCOL);
static TARGET_PROGRAM: RoleProgram<
    'static,
    1,
    <SeqSteps<
        StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>,
        <SeqSteps<
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
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
        > as StepConcat<
            SeqSteps<
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
                StepCons<SendStep<Role<1>, Role<0>, Msg<8, i32>>, StepNil>,
            >,
        >>::Output,
    > as ProjectRole<Role<1>>>::Output,
> = project(&PROTOCOL);

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport
        .state
        .lock()
        .expect("state lock")
        .queues
        .values()
        .all(|queue| queue.is_empty())
}

/// Test that loop control operates via flow().send() pattern (Pattern A).
///
/// Per AGENTS.md Branch Patterns:
/// - Controller uses flow().send() to explicitly decide Continue/Break
/// - Target (passive observer) uses offer() to observe the selected arm
#[tokio::test]
async fn loop_and_control_plane_tokens_share_lane() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(4096);
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
        .expect("register rendezvous");
    LOOP_DECISION_INDEX.store(0, Ordering::Relaxed);
    register_loop_lane_resolvers(&*cluster, rv_id);

    let sid = SessionId::new(9);

    let controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
        .expect("controller attach");
    let target = cluster
        .enter::<1, _, _, _>(rv_id, sid, &TARGET_PROGRAM, NoBinding)
        .expect("target attach");

    // Handshake: Controller → Target
    let (next_controller, _outcome) = controller
        .flow::<Msg<10, ()>>()
        .unwrap()
        .send(&())
        .await
        .expect("handshake send");
    let controller = next_controller;
    let (next_target, ()) = target.recv::<Msg<10, ()>>().await.expect("handshake recv");
    let target = next_target;

    // Loop iteration 1: Controller explicitly decides Continue via flow().send()
    // Per AGENTS.md Pattern A: flow().send() for explicit loop decisions
    // CanonicalControl auto-mints token, so pass () as payload
    let (next_controller, _outcome) = controller
        .flow::<Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >>()
        .unwrap()
        .send(())
        .await
        .expect("continue send");
    let controller = next_controller;

    // Controller: send BodyMsg to Target (inside continue arm)
    let (next_controller, _outcome) = controller
        .flow::<Msg<7, u32>>()
        .unwrap()
        .send(&1)
        .await
        .expect("loop body send");
    let controller = next_controller;

    // Target (passive observer): use offer() to observe the selected arm
    // Target doesn't see ContinueMsg (self-send), only cross-role messages (BodyMsg)
    let target_branch = target.offer().await.expect("target offer iteration 1");
    assert_eq!(
        target_branch.label(),
        7,
        "continue arm exposes BodyMsg recv to passive observer"
    );
    let (next_target, first_body) = target_branch
        .decode::<Msg<7, u32>>()
        .await
        .expect("decode body in continue arm");
    assert_eq!(first_body, 1);
    let target = next_target;

    // Loop iteration 2: Controller explicitly decides Break via flow().send()
    // CanonicalControl auto-mints token, so pass () as payload
    let (next_controller, _outcome) =
        controller
            .flow::<Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >>()
            .unwrap()
            .send(())
            .await
            .expect("break send");
    let controller = next_controller;

    // Target (passive observer): use offer() to observe break arm
    // Break arm exposes ExitMsg send to Target (not BreakMsg which is self-send)
    let target_branch = target.offer().await.expect("target offer break");
    assert_eq!(
        target_branch.label(),
        8,
        "break arm exposes ExitMsg send to passive observer"
    );
    // For send operations in selected arm, use flow().send() after into_endpoint
    let (next_target, _outcome) = target_branch
        .into_endpoint()
        .flow::<Msg<8, i32>>()
        .unwrap()
        .send(&0)
        .await
        .expect("exit marker send");
    let _target = next_target;

    // Controller: recv ExitMsg (inside break arm)
    let (next_controller, exit_value) = controller.recv::<Msg<8, i32>>().await.expect("exit recv");
    assert_eq!(exit_value, 0);
    let _controller = next_controller;

    assert!(transport_queue_is_empty(&transport));
}
