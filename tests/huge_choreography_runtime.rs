#![cfg(feature = "std")]

mod common;
#[path = "../internal/pico_smoke/src/fanout_program.rs"]
mod fanout_program;
#[path = "../internal/pico_smoke/src/huge_program.rs"]
mod huge_program;
#[path = "../internal/pico_smoke/src/linear_program.rs"]
mod linear_program;
#[path = "../internal/pico_smoke/src/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "../internal/pico_smoke/src/scenario.rs"]
mod scenario;

use common::TestTransport;
use hibana::{
    Endpoint, g,
    g::advanced::{CanonicalControl, project},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::advanced::{ControlMint, MintConfig},
        cap::{ControlResourceKind, GenericCapToken, ResourceKind},
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use scenario::ScenarioHarness;

type HugeKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type ControllerEndpoint<'a> = Endpoint<'a, 0, HugeKit, MintConfig>;
type WorkerEndpoint<'a> = Endpoint<'a, 1, HugeKit, MintConfig>;

fn controller_send_u8<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>, value: u8) {
    let flow = controller
        .flow::<g::Msg<LABEL, u8>>()
        .expect("controller flow<u8>");
    futures::executor::block_on(flow.send(&value)).expect("controller send<u8>");
}

fn controller_send_u32<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>, value: u32) {
    let flow = controller
        .flow::<g::Msg<LABEL, u32>>()
        .expect("controller flow<u32>");
    futures::executor::block_on(flow.send(&value)).expect("controller send<u32>");
}

fn worker_send_u8<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>, value: u8) {
    let flow = worker.flow::<g::Msg<LABEL, u8>>().expect("worker flow<u8>");
    futures::executor::block_on(flow.send(&value)).expect("worker send<u8>");
}

fn worker_recv_u8<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u8 {
    futures::executor::block_on(worker.recv::<g::Msg<LABEL, u8>>()).expect("worker recv<u8>")
}

fn controller_recv_u8<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>) -> u8 {
    futures::executor::block_on(controller.recv::<g::Msg<LABEL, u8>>())
        .expect("controller recv<u8>")
}

fn controller_select<'a, const LABEL: u8, K>(controller: &mut ControllerEndpoint<'_>)
where
    K: ResourceKind + ControlResourceKind + ControlMint + 'a + 'static,
{
    let outcome = futures::executor::block_on(
        controller
            .flow::<g::Msg<LABEL, GenericCapToken<K>, CanonicalControl<K>>>()
            .expect("controller control flow")
            .send(()),
    )
    .expect("controller control send");
    assert!(outcome.is_canonical());
}

fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = futures::executor::block_on(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    futures::executor::block_on(branch.decode::<g::Msg<LABEL, u32>>()).expect("worker decode<u32>")
}

struct RuntimeHarness;

impl ScenarioHarness for RuntimeHarness {
    type ControllerEndpoint<'a> = ControllerEndpoint<'a>;
    type WorkerEndpoint<'a> = WorkerEndpoint<'a>;

    fn controller_send_u8<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u8,
    ) {
        controller_send_u8::<LABEL>(controller, value);
    }

    fn controller_send_u32<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u32,
    ) {
        controller_send_u32::<LABEL>(controller, value);
    }

    fn worker_send_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>, value: u8) {
        worker_send_u8::<LABEL>(worker, value);
    }

    fn worker_recv_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u8 {
        worker_recv_u8::<LABEL>(worker)
    }

    fn controller_recv_u8<const LABEL: u8>(controller: &mut Self::ControllerEndpoint<'_>) -> u8 {
        controller_recv_u8::<LABEL>(controller)
    }

    fn controller_select<'a, const LABEL: u8, K>(controller: &mut Self::ControllerEndpoint<'a>)
    where
        K: ResourceKind + ControlResourceKind + ControlMint + 'a + 'static,
    {
        controller_select::<LABEL, K>(controller);
    }

    fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u32 {
        worker_offer_decode_u32::<LABEL>(worker)
    }
}

static ROUTE_HEAVY_PROGRAM: g::Program<huge_program::ProgramSteps> = huge_program::PROGRAM;
static LINEAR_HEAVY_PROGRAM: g::Program<linear_program::ProgramSteps> = linear_program::PROGRAM;
static FANOUT_HEAVY_PROGRAM: g::Program<fanout_program::ProgramSteps> = fanout_program::PROGRAM;
static ROUTE_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    0,
    huge_program::ProgramSteps,
    MintConfig,
> = project(&ROUTE_HEAVY_PROGRAM);
static ROUTE_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    1,
    huge_program::ProgramSteps,
    MintConfig,
> = project(&ROUTE_HEAVY_PROGRAM);
static LINEAR_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    0,
    linear_program::ProgramSteps,
    MintConfig,
> = project(&LINEAR_HEAVY_PROGRAM);
static LINEAR_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    1,
    linear_program::ProgramSteps,
    MintConfig,
> = project(&LINEAR_HEAVY_PROGRAM);
static FANOUT_HEAVY_CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    0,
    fanout_program::ProgramSteps,
    MintConfig,
> = project(&FANOUT_HEAVY_PROGRAM);
static FANOUT_HEAVY_WORKER_PROGRAM: hibana::g::advanced::RoleProgram<
    'static,
    1,
    fanout_program::ProgramSteps,
    MintConfig,
> = project(&FANOUT_HEAVY_PROGRAM);

#[inline(never)]
fn run_attached_sample<Steps>(
    controller_program: &'static hibana::g::advanced::RoleProgram<'static, 0, Steps, MintConfig>,
    worker_program: &'static hibana::g::advanced::RoleProgram<'static, 1, Steps, MintConfig>,
    route_scope_count: usize,
    expected_branch_labels: &'static [u8],
    expected_acks: &'static [u8],
    run: fn(
        &mut <RuntimeHarness as ScenarioHarness>::ControllerEndpoint<'_>,
        &mut <RuntimeHarness as ScenarioHarness>::WorkerEndpoint<'_>,
    ),
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
            huge_program::run::<RuntimeHarness>,
        );
    });

    run_on_small_stack("huge-choreography-linear-heavy", || {
        run_attached_sample(
            &LINEAR_HEAVY_CONTROLLER_PROGRAM,
            &LINEAR_HEAVY_WORKER_PROGRAM,
            linear_program::ROUTE_SCOPE_COUNT,
            &linear_program::EXPECTED_WORKER_BRANCH_LABELS,
            &linear_program::ACK_LABELS,
            linear_program::run::<RuntimeHarness>,
        );
    });

    run_on_small_stack("huge-choreography-fanout-heavy", || {
        run_attached_sample(
            &FANOUT_HEAVY_CONTROLLER_PROGRAM,
            &FANOUT_HEAVY_WORKER_PROGRAM,
            fanout_program::ROUTE_SCOPE_COUNT,
            &fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
            &fanout_program::ACK_LABELS,
            fanout_program::run::<RuntimeHarness>,
        );
    });
}
