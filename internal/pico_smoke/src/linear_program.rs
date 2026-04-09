use super::scenario::ScenarioHarness;
use hibana::g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::{self, Msg, Role};

type U8Send<const FROM: u8, const TO: u8, const LABEL: u8> =
    StepCons<SendStep<Role<FROM>, Role<TO>, Msg<LABEL, u8>>, StepNil>;

pub const ROUTE_SCOPE_COUNT: usize = 0;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];

type SegmentA01 = SeqSteps<U8Send<0, 1, 1>, U8Send<1, 0, 2>>;
type SegmentA02 = SeqSteps<SegmentA01, U8Send<0, 1, 3>>;
type SegmentA03 = SeqSteps<SegmentA02, U8Send<1, 0, 4>>;
type SegmentA04 = SeqSteps<SegmentA03, U8Send<0, 1, 5>>;
type SegmentA05 = SeqSteps<SegmentA04, U8Send<1, 0, 6>>;
type SegmentA06 = SeqSteps<SegmentA05, U8Send<0, 1, 7>>;
pub type SegmentA = SeqSteps<SegmentA06, U8Send<1, 0, 8>>;

type SegmentB01 = SeqSteps<U8Send<0, 1, 9>, U8Send<1, 0, 10>>;
type SegmentB02 = SeqSteps<SegmentB01, U8Send<0, 1, 11>>;
type SegmentB03 = SeqSteps<SegmentB02, U8Send<1, 0, 12>>;
type SegmentB04 = SeqSteps<SegmentB03, U8Send<0, 1, 13>>;
type SegmentB05 = SeqSteps<SegmentB04, U8Send<1, 0, 14>>;
type SegmentB06 = SeqSteps<SegmentB05, U8Send<0, 1, 15>>;
pub type SegmentB = SeqSteps<SegmentB06, U8Send<1, 0, 16>>;

type SegmentC01 = SeqSteps<U8Send<0, 1, 17>, U8Send<1, 0, 18>>;
type SegmentC02 = SeqSteps<SegmentC01, U8Send<0, 1, 19>>;
type SegmentC03 = SeqSteps<SegmentC02, U8Send<1, 0, 20>>;
type SegmentC04 = SeqSteps<SegmentC03, U8Send<0, 1, 21>>;
type SegmentC05 = SeqSteps<SegmentC04, U8Send<1, 0, 22>>;
type SegmentC06 = SeqSteps<SegmentC05, U8Send<0, 1, 23>>;
pub type SegmentC = SeqSteps<SegmentC06, U8Send<1, 0, 24>>;

type SegmentD01 = SeqSteps<U8Send<0, 1, 81>, U8Send<1, 0, 82>>;
type SegmentD02 = SeqSteps<SegmentD01, U8Send<0, 1, 83>>;
type SegmentD03 = SeqSteps<SegmentD02, U8Send<1, 0, 84>>;
type SegmentD04 = SeqSteps<SegmentD03, U8Send<0, 1, 85>>;
type SegmentD05 = SeqSteps<SegmentD04, U8Send<1, 0, 86>>;
type SegmentD06 = SeqSteps<SegmentD05, U8Send<0, 1, 87>>;
pub type SegmentD = SeqSteps<SegmentD06, U8Send<1, 0, 88>>;

type SegmentE01 = SeqSteps<U8Send<0, 1, 89>, U8Send<1, 0, 90>>;
type SegmentE02 = SeqSteps<SegmentE01, U8Send<0, 1, 91>>;
type SegmentE03 = SeqSteps<SegmentE02, U8Send<1, 0, 92>>;
type SegmentE04 = SeqSteps<SegmentE03, U8Send<0, 1, 93>>;
type SegmentE05 = SeqSteps<SegmentE04, U8Send<1, 0, 94>>;
type SegmentE06 = SeqSteps<SegmentE05, U8Send<0, 1, 95>>;
pub type SegmentE = SeqSteps<SegmentE06, U8Send<1, 0, 96>>;

type SegmentF01 = SeqSteps<U8Send<0, 1, 97>, U8Send<1, 0, 98>>;
type SegmentF02 = SeqSteps<SegmentF01, U8Send<0, 1, 99>>;
type SegmentF03 = SeqSteps<SegmentF02, U8Send<1, 0, 100>>;
type SegmentF04 = SeqSteps<SegmentF03, U8Send<0, 1, 101>>;
type SegmentF05 = SeqSteps<SegmentF04, U8Send<1, 0, 102>>;
type SegmentF06 = SeqSteps<SegmentF05, U8Send<0, 1, 103>>;
pub type SegmentF = SeqSteps<SegmentF06, U8Send<1, 0, 104>>;

type ProgramTailE = SeqSteps<SegmentE, SegmentF>;
type ProgramTailD = SeqSteps<SegmentD, ProgramTailE>;
type ProgramTailC = SeqSteps<SegmentC, ProgramTailD>;
type ProgramTailB = SeqSteps<SegmentB, ProgramTailC>;
pub type ProgramSteps = SeqSteps<SegmentA, ProgramTailB>;

pub const PROGRAM: g::ProgramSource<ProgramSteps> = g::seq(
    segment_a(),
    g::seq(
        segment_b(),
        g::seq(
            segment_c(),
            g::seq(segment_d(), g::seq(segment_e(), segment_f())),
        ),
    ),
);

const fn segment_a() -> g::ProgramSource<SegmentA> {
    let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
}

const fn segment_b() -> g::ProgramSource<SegmentB> {
    let program = g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<13, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<14, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<15, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<16, u8>, 0>())
}

const fn segment_c() -> g::ProgramSource<SegmentC> {
    let program = g::send::<Role<0>, Role<1>, Msg<17, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<18, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<19, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<20, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<21, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<22, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<23, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<24, u8>, 0>())
}

const fn segment_d() -> g::ProgramSource<SegmentD> {
    let program = g::send::<Role<0>, Role<1>, Msg<81, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<82, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<84, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<86, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<88, u8>, 0>())
}

const fn segment_e() -> g::ProgramSource<SegmentE> {
    let program = g::send::<Role<0>, Role<1>, Msg<89, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<90, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<92, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<94, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<96, u8>, 0>())
}

const fn segment_f() -> g::ProgramSource<SegmentF> {
    let program = g::send::<Role<0>, Role<1>, Msg<97, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<99, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>())
}

pub fn run<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    run_segment_a::<H>(controller, worker);
    run_segment_b::<H>(controller, worker);
    run_segment_c::<H>(controller, worker);
    run_segment_d::<H>(controller, worker);
    run_segment_e::<H>(controller, worker);
    run_segment_f::<H>(controller, worker);
}

fn run_segment_a<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<1>(controller, 1);
    assert_eq!(H::worker_recv_u8::<1>(worker), 1);
    H::worker_send_u8::<2>(worker, 2);
    assert_eq!(H::controller_recv_u8::<2>(controller), 2);
    H::controller_send_u8::<3>(controller, 3);
    assert_eq!(H::worker_recv_u8::<3>(worker), 3);
    H::worker_send_u8::<4>(worker, 4);
    assert_eq!(H::controller_recv_u8::<4>(controller), 4);
    H::controller_send_u8::<5>(controller, 5);
    assert_eq!(H::worker_recv_u8::<5>(worker), 5);
    H::worker_send_u8::<6>(worker, 6);
    assert_eq!(H::controller_recv_u8::<6>(controller), 6);
    H::controller_send_u8::<7>(controller, 7);
    assert_eq!(H::worker_recv_u8::<7>(worker), 7);
    H::worker_send_u8::<8>(worker, 8);
    assert_eq!(H::controller_recv_u8::<8>(controller), 8);
}

fn run_segment_b<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<9>(controller, 9);
    assert_eq!(H::worker_recv_u8::<9>(worker), 9);
    H::worker_send_u8::<10>(worker, 10);
    assert_eq!(H::controller_recv_u8::<10>(controller), 10);
    H::controller_send_u8::<11>(controller, 11);
    assert_eq!(H::worker_recv_u8::<11>(worker), 11);
    H::worker_send_u8::<12>(worker, 12);
    assert_eq!(H::controller_recv_u8::<12>(controller), 12);
    H::controller_send_u8::<13>(controller, 13);
    assert_eq!(H::worker_recv_u8::<13>(worker), 13);
    H::worker_send_u8::<14>(worker, 14);
    assert_eq!(H::controller_recv_u8::<14>(controller), 14);
    H::controller_send_u8::<15>(controller, 15);
    assert_eq!(H::worker_recv_u8::<15>(worker), 15);
    H::worker_send_u8::<16>(worker, 16);
    assert_eq!(H::controller_recv_u8::<16>(controller), 16);
}

fn run_segment_c<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<17>(controller, 17);
    assert_eq!(H::worker_recv_u8::<17>(worker), 17);
    H::worker_send_u8::<18>(worker, 18);
    assert_eq!(H::controller_recv_u8::<18>(controller), 18);
    H::controller_send_u8::<19>(controller, 19);
    assert_eq!(H::worker_recv_u8::<19>(worker), 19);
    H::worker_send_u8::<20>(worker, 20);
    assert_eq!(H::controller_recv_u8::<20>(controller), 20);
    H::controller_send_u8::<21>(controller, 21);
    assert_eq!(H::worker_recv_u8::<21>(worker), 21);
    H::worker_send_u8::<22>(worker, 22);
    assert_eq!(H::controller_recv_u8::<22>(controller), 22);
    H::controller_send_u8::<23>(controller, 23);
    assert_eq!(H::worker_recv_u8::<23>(worker), 23);
    H::worker_send_u8::<24>(worker, 24);
    assert_eq!(H::controller_recv_u8::<24>(controller), 24);
}

fn run_segment_d<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<81>(controller, 81);
    assert_eq!(H::worker_recv_u8::<81>(worker), 81);
    H::worker_send_u8::<82>(worker, 82);
    assert_eq!(H::controller_recv_u8::<82>(controller), 82);
    H::controller_send_u8::<83>(controller, 83);
    assert_eq!(H::worker_recv_u8::<83>(worker), 83);
    H::worker_send_u8::<84>(worker, 84);
    assert_eq!(H::controller_recv_u8::<84>(controller), 84);
    H::controller_send_u8::<85>(controller, 85);
    assert_eq!(H::worker_recv_u8::<85>(worker), 85);
    H::worker_send_u8::<86>(worker, 86);
    assert_eq!(H::controller_recv_u8::<86>(controller), 86);
    H::controller_send_u8::<87>(controller, 87);
    assert_eq!(H::worker_recv_u8::<87>(worker), 87);
    H::worker_send_u8::<88>(worker, 88);
    assert_eq!(H::controller_recv_u8::<88>(controller), 88);
}

fn run_segment_e<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<89>(controller, 89);
    assert_eq!(H::worker_recv_u8::<89>(worker), 89);
    H::worker_send_u8::<90>(worker, 90);
    assert_eq!(H::controller_recv_u8::<90>(controller), 90);
    H::controller_send_u8::<91>(controller, 91);
    assert_eq!(H::worker_recv_u8::<91>(worker), 91);
    H::worker_send_u8::<92>(worker, 92);
    assert_eq!(H::controller_recv_u8::<92>(controller), 92);
    H::controller_send_u8::<93>(controller, 93);
    assert_eq!(H::worker_recv_u8::<93>(worker), 93);
    H::worker_send_u8::<94>(worker, 94);
    assert_eq!(H::controller_recv_u8::<94>(controller), 94);
    H::controller_send_u8::<95>(controller, 95);
    assert_eq!(H::worker_recv_u8::<95>(worker), 95);
    H::worker_send_u8::<96>(worker, 96);
    assert_eq!(H::controller_recv_u8::<96>(controller), 96);
}

fn run_segment_f<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<97>(controller, 97);
    assert_eq!(H::worker_recv_u8::<97>(worker), 97);
    H::worker_send_u8::<98>(worker, 98);
    assert_eq!(H::controller_recv_u8::<98>(controller), 98);
    H::controller_send_u8::<99>(controller, 99);
    assert_eq!(H::worker_recv_u8::<99>(worker), 99);
    H::worker_send_u8::<100>(worker, 100);
    assert_eq!(H::controller_recv_u8::<100>(controller), 100);
    H::controller_send_u8::<101>(controller, 101);
    assert_eq!(H::worker_recv_u8::<101>(worker), 101);
    H::worker_send_u8::<102>(worker, 102);
    assert_eq!(H::controller_recv_u8::<102>(controller), 102);
    H::controller_send_u8::<103>(controller, 103);
    assert_eq!(H::worker_recv_u8::<103>(worker), 103);
    H::worker_send_u8::<104>(worker, 104);
    assert_eq!(H::controller_recv_u8::<104>(controller), 104);
}
