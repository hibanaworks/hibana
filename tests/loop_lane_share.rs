#![cfg(feature = "std")]

//! Test that loop control (self-send) operates via flow().send() pattern.
//!
//! Per AGENTS.md Branch Patterns:
//! - Pattern A (Explicit Decision): Controller uses flow().send() for loop Continue/Break
//! - The self-send records the decision in RouteTable
//! - Target (passive observer) uses offer() to observe the arm via cross-role messages

mod common;
mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::TestTransport;
use hibana::{
    NoBinding,
    control::{
        cap::{GenericCapToken, resource_kinds::{LoopBreakKind, LoopContinueKind}},
        cluster::{DynamicResolution, ResolverContext},
        types::RendezvousId,
    },
    g::{
        self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, Msg, Role,
        steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil},
    },
    global::const_dsl::{DynamicMeta, HandlePlan},
    rendezvous::{Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE},
    },
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

type Controller = Role<0>;
type Target = Role<1>;

const LOOP_POLICY_ID: u16 = 99;
const LOOP_PLAN_META: DynamicMeta = DynamicMeta::new();
static LOOP_DECISION_INDEX: AtomicUsize = AtomicUsize::new(0);

type Handshake = Msg<10, ()>;
type BodyMsg = Msg<7, u32>;
type ExitMsg = Msg<8, i32>;
// Self-send control messages for loop decisions (Controller → Controller)
type ContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    hibana::g::CanonicalControl<LoopContinueKind>,
>;
type BreakMsg = Msg<
    { LABEL_LOOP_BREAK },
    GenericCapToken<LoopBreakKind>,
    hibana::g::CanonicalControl<LoopBreakKind>,
>;

type HandshakeSteps = StepCons<SendStep<Controller, Target, Handshake>, StepNil>;
type BodySteps = StepCons<SendStep<Controller, Target, BodyMsg>, StepNil>;
type ExitSteps = StepCons<SendStep<Target, Controller, ExitMsg>, StepNil>;
// LoopContinue/BreakMsg are self-send (Controller → Controller, no Target param)
type LoopSeq = LoopDecisionSteps<Controller, ContinueMsg, BreakMsg, ExitSteps, BodySteps>;
type ProtocolSteps = <HandshakeSteps as StepConcat<LoopSeq>>::Output;

type ControllerLocal = <ProtocolSteps as ProjectRole<Controller>>::Output;
type TargetLocal = <ProtocolSteps as ProjectRole<Target>>::Output;

fn loop_lane_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    let idx = LOOP_DECISION_INDEX.fetch_add(1, Ordering::Relaxed);
    let decision = idx == 0;
    Ok(DynamicResolution::Loop { decision })
}

fn register_loop_lane_resolvers(cluster: &Cluster, rv_id: RendezvousId) {
    for info in CONTROLLER_PROGRAM.control_plans() {
        if info.plan.is_dynamic() {
            cluster
                .register_control_plan_resolver(rv_id, &info, loop_lane_resolver)
                .expect("register loop resolver");
        }
    }
}

const LOOP_BODY: g::Program<BodySteps> = g::send::<Controller, Target, BodyMsg, 0>();
const LOOP_EXIT: g::Program<ExitSteps> = g::send::<Target, Controller, ExitMsg, 0>();

// Self-send for canonical control: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<LoopContinueSteps<Controller, ContinueMsg, BodySteps>> =
    g::with_control_plan(
        g::send::<Controller, Controller, ContinueMsg, 0>(),
        HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
    )
    .then(LOOP_BODY);
const LOOP_BREAK_ARM: g::Program<LoopBreakSteps<Controller, BreakMsg, ExitSteps>> =
    g::with_control_plan(
        g::send::<Controller, Controller, BreakMsg, 0>(),
        HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
    )
    .then(LOOP_EXIT);

// Route is local to Controller (0 → 0, self-send)
const LOOP_SEGMENT: g::Program<LoopSeq> = g::route::<0, _>(
    g::route_chain::<0, LoopContinueSteps<Controller, ContinueMsg, BodySteps>>(LOOP_CONTINUE_ARM)
        .and::<LoopBreakSteps<Controller, BreakMsg, ExitSteps>>(LOOP_BREAK_ARM),
);

const PROTOCOL: g::Program<ProtocolSteps> =
    g::seq(g::send::<Controller, Target, Handshake, 0>(), LOOP_SEGMENT);

static CONTROLLER_PROGRAM: g::RoleProgram<'static, 0, ControllerLocal> =
    g::project::<0, ProtocolSteps, _>(&PROTOCOL);
static TARGET_PROGRAM: g::RoleProgram<'static, 1, TargetLocal> =
    g::project::<1, ProtocolSteps, _>(&PROTOCOL);

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
    let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport.clone());

    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");
    LOOP_DECISION_INDEX.store(0, Ordering::Relaxed);
    register_loop_lane_resolvers(&*cluster, rv_id);

    let sid = SessionId::new(9);

    let controller = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
        .expect("controller attach");
    let target = cluster
        .attach_cursor::<1, _, _, _>(rv_id, sid, &TARGET_PROGRAM, NoBinding)
        .expect("target attach");

    // Handshake: Controller → Target
    let (next_controller, _outcome) = controller
        .flow::<Handshake>()
        .unwrap()
        .send(&())
        .await
        .expect("handshake send");
    let controller = next_controller;
    let (next_target, ()) = target.recv::<Handshake>().await.expect("handshake recv");
    let target = next_target;

    // Loop iteration 1: Controller explicitly decides Continue via flow().send()
    // Per AGENTS.md Pattern A: flow().send() for explicit loop decisions
    // CanonicalControl auto-mints token, so pass () as payload
    let (next_controller, _outcome) = controller
        .flow::<ContinueMsg>()
        .unwrap()
        .send(())
        .await
        .expect("continue send");
    let controller = next_controller;

    // Controller: send BodyMsg to Target (inside continue arm)
    let (next_controller, _outcome) = controller
        .flow::<BodyMsg>()
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
        .decode::<BodyMsg>()
        .await
        .expect("decode body in continue arm");
    assert_eq!(first_body, 1);
    let target = next_target;

    // Loop iteration 2: Controller explicitly decides Break via flow().send()
    // CanonicalControl auto-mints token, so pass () as payload
    let (next_controller, _outcome) = controller
        .flow::<BreakMsg>()
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
        .flow::<ExitMsg>()
        .unwrap()
        .send(&0)
        .await
        .expect("exit marker send");
    let _target = next_target;

    // Controller: recv ExitMsg (inside break arm)
    let (next_controller, exit_value) = controller.recv::<ExitMsg>().await.expect("exit recv");
    assert_eq!(exit_value, 0);
    let _controller = next_controller;

    #[cfg(feature = "test-utils")]
    controller.phase_cursor().assert_terminal();
    #[cfg(feature = "test-utils")]
    target.phase_cursor().assert_terminal();
    assert!(transport.queue_is_empty());
}
