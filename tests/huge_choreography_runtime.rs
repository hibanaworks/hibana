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

const ROUTE_HEAVY_PROGRAM: g::Program<huge_program::ProgramSteps> =
    g::freeze(&huge_program::PROGRAM);
const LINEAR_HEAVY_PROGRAM: g::Program<linear_program::ProgramSteps> =
    g::freeze(&linear_program::PROGRAM);
const FANOUT_HEAVY_PROGRAM: g::Program<fanout_program::ProgramSteps> =
    g::freeze(&fanout_program::PROGRAM);

struct RuntimeHarness;

impl ScenarioHarness for RuntimeHarness {
    type ControllerEndpoint<'a> = Endpoint<'a, 0, HugeKit>;
    type WorkerEndpoint<'a> = Endpoint<'a, 1, HugeKit>;

    fn controller_send_u8<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u8,
    ) {
        let flow = controller
            .flow::<g::Msg<LABEL, u8>>()
            .expect("controller flow<u8>");
        let _ = futures::executor::block_on(flow.send(&value)).expect("controller send");
    }

    fn controller_send_u32<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u32,
    ) {
        let flow = controller
            .flow::<g::Msg<LABEL, u32>>()
            .expect("controller flow<u32>");
        let _ = futures::executor::block_on(flow.send(&value)).expect("controller send");
    }

    fn worker_send_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>, value: u8) {
        let flow = worker.flow::<g::Msg<LABEL, u8>>().expect("worker flow<u8>");
        let _ = futures::executor::block_on(flow.send(&value)).expect("worker send");
    }

    fn worker_recv_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u8 {
        futures::executor::block_on(worker.recv::<g::Msg<LABEL, u8>>()).expect("worker recv")
    }

    fn controller_recv_u8<const LABEL: u8>(controller: &mut Self::ControllerEndpoint<'_>) -> u8 {
        futures::executor::block_on(controller.recv::<g::Msg<LABEL, u8>>())
            .expect("controller recv")
    }

    fn controller_select<'a, const LABEL: u8, K>(controller: &mut Self::ControllerEndpoint<'a>)
    where
        K: ResourceKind + ControlResourceKind + ControlMint + 'a,
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

    fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u32 {
        let branch = futures::executor::block_on(worker.offer()).expect("worker offer");
        assert_eq!(branch.label(), LABEL);
        futures::executor::block_on(branch.decode::<g::Msg<LABEL, u32>>()).expect("worker decode")
    }
}

#[inline(never)]
fn run_attached_sample<Steps: 'static>(
    program: &'static g::Program<Steps>,
    route_scope_count: usize,
    expected_branch_labels: &'static [u8],
    expected_acks: &'static [u8],
    run: fn(
        &mut <RuntimeHarness as ScenarioHarness>::ControllerEndpoint<'_>,
        &mut <RuntimeHarness as ScenarioHarness>::WorkerEndpoint<'_>,
    ),
) where
    Steps: hibana::g::advanced::steps::ProjectRole<g::Role<0>>
        + hibana::g::advanced::steps::ProjectRole<g::Role<1>>,
{
    assert_eq!(route_scope_count, expected_branch_labels.len());
    assert_eq!(route_scope_count, expected_acks.len());

    runtime_support::with_fixture(|clock, tap_buf, slab| {
        eprintln!("before transport");
        let transport = TestTransport::default();
        eprintln!("before project controller");
        let controller_program: hibana::g::advanced::RoleProgram<'_, 0, Steps, MintConfig> =
            project(program);
        eprintln!("before project worker");
        let worker_program: hibana::g::advanced::RoleProgram<'_, 1, Steps, MintConfig> =
            project(program);
        eprintln!("before kit new");
        let kit = HugeKit::new(clock);
        eprintln!("before add rendezvous");
        let rv_id = kit
            .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
            .expect("register rendezvous");
        eprintln!("before enter controller");
        let sid = SessionId::new(0x6000);
        let mut controller = kit
            .enter(rv_id, sid, &controller_program, NoBinding)
            .expect("enter controller");
        eprintln!("before enter worker");
        let mut worker = kit
            .enter(rv_id, sid, &worker_program, NoBinding)
            .expect("enter worker");

        eprintln!("before run");
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
            &ROUTE_HEAVY_PROGRAM,
            huge_program::ROUTE_SCOPE_COUNT,
            &huge_program::EXPECTED_WORKER_BRANCH_LABELS,
            &huge_program::ACK_LABELS,
            huge_program::run::<RuntimeHarness>,
        );
    });

    run_on_small_stack("huge-choreography-linear-heavy", || {
        run_attached_sample(
            &LINEAR_HEAVY_PROGRAM,
            linear_program::ROUTE_SCOPE_COUNT,
            &linear_program::EXPECTED_WORKER_BRANCH_LABELS,
            &linear_program::ACK_LABELS,
            linear_program::run::<RuntimeHarness>,
        );
    });

    run_on_small_stack("huge-choreography-fanout-heavy", || {
        run_attached_sample(
            &FANOUT_HEAVY_PROGRAM,
            fanout_program::ROUTE_SCOPE_COUNT,
            &fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
            &fanout_program::ACK_LABELS,
            fanout_program::run::<RuntimeHarness>,
        );
    });
}
