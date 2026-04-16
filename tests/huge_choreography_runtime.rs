#![cfg(feature = "std")]

mod common;
#[path = "../internal/pico_smoke/src/fanout_program.rs"]
mod fanout_program;
#[path = "../internal/pico_smoke/src/huge_program.rs"]
mod huge_program;
#[path = "../internal/pico_smoke/src/linear_program.rs"]
mod linear_program;
#[path = "../internal/pico_smoke/src/localside.rs"]
mod localside;
#[path = "../internal/pico_smoke/src/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    Endpoint, g,
    g::advanced::{
        CanonicalControl, project,
        steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil},
    },
    g::{Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::GenericCapToken,
        cap::advanced::MintConfig,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};

type HugeKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type ControllerEndpoint<'a> = Endpoint<'a, 0, HugeKit, MintConfig>;
type WorkerEndpoint<'a> = Endpoint<'a, 1, HugeKit, MintConfig>;

fn drive<F: core::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

static ROUTE_HEAVY_PROGRAM: g::Program<huge_program::ProgramSteps> = huge_program::PROGRAM;
static LINEAR_HEAVY_PROGRAM: g::Program<linear_program::ProgramSteps> = linear_program::PROGRAM;
static FANOUT_HEAVY_PROGRAM: g::Program<fanout_program::ProgramSteps> = fanout_program::PROGRAM;
static ROUTE_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 0, MintConfig> =
    project(&ROUTE_HEAVY_PROGRAM);
static ROUTE_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 1, MintConfig> =
    project(&ROUTE_HEAVY_PROGRAM);
static LINEAR_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 0, MintConfig> =
    project(&LINEAR_HEAVY_PROGRAM);
static LINEAR_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 1, MintConfig> =
    project(&LINEAR_HEAVY_PROGRAM);
static FANOUT_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 0, MintConfig> =
    project(&FANOUT_HEAVY_PROGRAM);
static FANOUT_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 1, MintConfig> =
    project(&FANOUT_HEAVY_PROGRAM);

const HIGH_LANE_LEFT_CTRL: u8 = 122;
const HIGH_LANE_RIGHT_CTRL: u8 = 123;
const HIGH_LANE_LEFT_LABEL: u8 = 124;
const HIGH_LANE_RIGHT_LABEL: u8 = 125;
const HIGH_LANE_LEFT_REPLY_LABEL: u8 = 126;
const HIGH_LANE_RIGHT_REPLY_LABEL: u8 = 127;
const HIGH_LANE_LEFT: u8 = 33;
const HIGH_LANE_RIGHT: u8 = 34;

type HighLaneLeftKind = route_control_kinds::RouteControl<HIGH_LANE_LEFT_CTRL, 0>;
type HighLaneRightKind = route_control_kinds::RouteControl<HIGH_LANE_RIGHT_CTRL, 1>;
type HighLaneLeftHead = StepCons<
    SendStep<
        Role<0>,
        Role<0>,
        Msg<
            { HIGH_LANE_LEFT_CTRL },
            GenericCapToken<HighLaneLeftKind>,
            CanonicalControl<HighLaneLeftKind>,
        >,
    >,
    StepNil,
>;
type HighLaneRightHead = StepCons<
    SendStep<
        Role<0>,
        Role<0>,
        Msg<
            { HIGH_LANE_RIGHT_CTRL },
            GenericCapToken<HighLaneRightKind>,
            CanonicalControl<HighLaneRightKind>,
        >,
    >,
    StepNil,
>;
type HighLaneLeftSteps = SeqSteps<
    HighLaneLeftHead,
    SeqSteps<
        StepCons<
            SendStep<Role<0>, Role<1>, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>, HIGH_LANE_LEFT>,
            StepNil,
        >,
        StepCons<
            SendStep<Role<1>, Role<0>, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>, HIGH_LANE_LEFT>,
            StepNil,
        >,
    >,
>;
type HighLaneRightSteps = SeqSteps<
    HighLaneRightHead,
    SeqSteps<
        StepCons<
            SendStep<Role<0>, Role<1>, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>, HIGH_LANE_RIGHT>,
            StepNil,
        >,
        StepCons<
            SendStep<Role<1>, Role<0>, Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>, HIGH_LANE_RIGHT>,
            StepNil,
        >,
    >,
>;
type HighLaneRouteProgramSteps = RouteSteps<HighLaneLeftSteps, HighLaneRightSteps>;

const HIGH_LANE_LEFT_PROGRAM: g::Program<HighLaneLeftSteps> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { HIGH_LANE_LEFT_CTRL },
            GenericCapToken<HighLaneLeftKind>,
            CanonicalControl<HighLaneLeftKind>,
        >,
        0,
    >();
    g::seq(
        program,
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>, HIGH_LANE_LEFT>(),
            g::send::<Role<1>, Role<0>, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>, HIGH_LANE_LEFT>(),
        ),
    )
};

const HIGH_LANE_RIGHT_PROGRAM: g::Program<HighLaneRightSteps> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { HIGH_LANE_RIGHT_CTRL },
            GenericCapToken<HighLaneRightKind>,
            CanonicalControl<HighLaneRightKind>,
        >,
        0,
    >();
    g::seq(
        program,
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>, HIGH_LANE_RIGHT>(),
            g::send::<Role<1>, Role<0>, Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>, HIGH_LANE_RIGHT>(
            ),
        ),
    )
};

const HIGH_LANE_ROUTE_PROGRAM: g::Program<HighLaneRouteProgramSteps> =
    g::route(HIGH_LANE_LEFT_PROGRAM, HIGH_LANE_RIGHT_PROGRAM);
static HIGH_LANE_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 0, MintConfig> =
    project(&HIGH_LANE_ROUTE_PROGRAM);
static HIGH_LANE_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 1, MintConfig> =
    project(&HIGH_LANE_ROUTE_PROGRAM);

#[inline(never)]
fn run_attached_sample(
    controller_program: &'static hibana::g::advanced::RoleProgram<'static, 0, MintConfig>,
    worker_program: &'static hibana::g::advanced::RoleProgram<'static, 1, MintConfig>,
    route_scope_count: usize,
    expected_branch_labels: &'static [u8],
    expected_acks: &'static [u8],
    run: fn(&mut ControllerEndpoint<'_>, &mut WorkerEndpoint<'_>),
) {
    assert_eq!(route_scope_count, expected_branch_labels.len());
    assert_eq!(route_scope_count, expected_acks.len());

    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = kit
            .enter(rv_id, sid, controller_program, NoBinding)
            .expect("enter controller");
        let mut worker = kit
            .enter(rv_id, sid, worker_program, NoBinding)
            .expect("enter worker");

        run(&mut controller, &mut worker);
        assert!(
            transport.queue_is_empty(),
            "huge choreography runtime must drain every transport frame"
        );
    });
}

#[inline(never)]
fn run_on_small_stack(name: &'static str, f: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .name(name.into())
        .stack_size(32 * 1024)
        .spawn(f)
        .expect("spawn huge choreography runtime thread");
    handle.join().expect("small-stack huge choreography thread");
}

#[test]
fn huge_choreography_shape_matrix_runs_to_completion_on_small_stack() {
    run_on_small_stack("huge-choreography-route-heavy", || {
        run_attached_sample(
            &ROUTE_HEAVY_CONTROLLER_PROGRAM,
            &ROUTE_HEAVY_WORKER_PROGRAM,
            huge_program::ROUTE_SCOPE_COUNT,
            &huge_program::EXPECTED_WORKER_BRANCH_LABELS,
            &huge_program::ACK_LABELS,
            huge_program::run,
        );
    });

    run_on_small_stack("huge-choreography-linear-heavy", || {
        run_attached_sample(
            &LINEAR_HEAVY_CONTROLLER_PROGRAM,
            &LINEAR_HEAVY_WORKER_PROGRAM,
            linear_program::ROUTE_SCOPE_COUNT,
            &linear_program::EXPECTED_WORKER_BRANCH_LABELS,
            &linear_program::ACK_LABELS,
            linear_program::run,
        );
    });

    run_on_small_stack("huge-choreography-fanout-heavy", || {
        run_attached_sample(
            &FANOUT_HEAVY_CONTROLLER_PROGRAM,
            &FANOUT_HEAVY_WORKER_PROGRAM,
            fanout_program::ROUTE_SCOPE_COUNT,
            &fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
            &fanout_program::ACK_LABELS,
            fanout_program::run,
        );
    });
}

#[test]
fn high_lane_route_runs_to_completion_on_actual_localside() {
    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(
                Config::new(tap_buf, slab).with_lane_range(0..35),
                transport.clone(),
            )
            .expect("register rendezvous");

        let mut controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6100),
                &HIGH_LANE_CONTROLLER_PROGRAM,
                NoBinding,
            )
            .expect("enter controller-left");
        let mut worker = kit
            .enter(
                rv_id,
                SessionId::new(0x6100),
                &HIGH_LANE_WORKER_PROGRAM,
                NoBinding,
            )
            .expect("enter worker-left");
        localside::controller_select::<{ HIGH_LANE_LEFT_CTRL }, HighLaneLeftKind, _, _, _, 2>(
            &mut controller,
        );
        localside::controller_send_u8::<{ HIGH_LANE_LEFT_LABEL }, _, _, _, 2>(&mut controller, 7);
        assert_eq!(
            localside::worker_offer_decode_u8::<{ HIGH_LANE_LEFT_LABEL }, _, _, _, 2>(&mut worker,),
            7,
            "lane 33 route payload must survive exact lane-set runtime selection"
        );
        localside::worker_send_u8::<{ HIGH_LANE_LEFT_REPLY_LABEL }, _, _, _, 2>(&mut worker, 17);
        assert_eq!(
            localside::controller_recv_u8::<{ HIGH_LANE_LEFT_REPLY_LABEL }, _, _, _, 2>(
                &mut controller
            ),
            17,
            "lane 33 reply payload must roundtrip through SessionKit localside"
        );
        drop(worker);
        drop(controller);

        let mut controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6101),
                &HIGH_LANE_CONTROLLER_PROGRAM,
                NoBinding,
            )
            .expect("enter controller-right");
        let mut worker = kit
            .enter(
                rv_id,
                SessionId::new(0x6101),
                &HIGH_LANE_WORKER_PROGRAM,
                NoBinding,
            )
            .expect("enter worker-right");
        localside::controller_select::<{ HIGH_LANE_RIGHT_CTRL }, HighLaneRightKind, _, _, _, 2>(
            &mut controller,
        );
        localside::controller_send_u8::<{ HIGH_LANE_RIGHT_LABEL }, _, _, _, 2>(&mut controller, 9);
        assert_eq!(
            localside::worker_offer_decode_u8::<{ HIGH_LANE_RIGHT_LABEL }, _, _, _, 2>(&mut worker,),
            9,
            "lane 34 route payload must survive exact lane-set runtime selection"
        );
        localside::worker_send_u8::<{ HIGH_LANE_RIGHT_REPLY_LABEL }, _, _, _, 2>(&mut worker, 19);
        assert_eq!(
            localside::controller_recv_u8::<{ HIGH_LANE_RIGHT_REPLY_LABEL }, _, _, _, 2>(
                &mut controller
            ),
            19,
            "lane 34 reply payload must roundtrip through SessionKit localside"
        );

        assert!(
            transport.queue_is_empty(),
            "high-lane localside route test must drain every transport frame"
        );
    });
}
