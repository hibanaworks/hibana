use hibana::g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    Transport,
    runtime::{Clock, LabelUniverse},
};

use super::localside;

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

pub const PROGRAM: g::Program<ProgramSteps> = g::seq(
    segment_a(),
    g::seq(
        segment_b(),
        g::seq(
            segment_c(),
            g::seq(segment_d(), g::seq(segment_e(), segment_f())),
        ),
    ),
);

const fn segment_a() -> g::Program<SegmentA> {
    let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
}

const fn segment_b() -> g::Program<SegmentB> {
    let program = g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<13, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<14, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<15, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<16, u8>, 0>())
}

const fn segment_c() -> g::Program<SegmentC> {
    let program = g::send::<Role<0>, Role<1>, Msg<17, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<18, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<19, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<20, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<21, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<22, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<23, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<24, u8>, 0>())
}

const fn segment_d() -> g::Program<SegmentD> {
    let program = g::send::<Role<0>, Role<1>, Msg<81, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<82, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<84, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<86, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<88, u8>, 0>())
}

const fn segment_e() -> g::Program<SegmentE> {
    let program = g::send::<Role<0>, Role<1>, Msg<89, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<90, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<92, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<94, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<96, u8>, 0>())
}

const fn segment_f() -> g::Program<SegmentF> {
    let program = g::send::<Role<0>, Role<1>, Msg<97, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<99, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>());
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>())
}

pub fn run<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    run_segment_a(controller, worker);
    run_segment_b(controller, worker);
    run_segment_c(controller, worker);
    run_segment_d(controller, worker);
    run_segment_e(controller, worker);
    run_segment_f(controller, worker);
}

fn run_segment_a<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<1, _, _, _, MAX_RV>(controller, 1);
    assert_eq!(localside::worker_recv_u8::<1, _, _, _, MAX_RV>(worker), 1);
    localside::worker_send_u8::<2, _, _, _, MAX_RV>(worker, 2);
    assert_eq!(
        localside::controller_recv_u8::<2, _, _, _, MAX_RV>(controller),
        2
    );
    localside::controller_send_u8::<3, _, _, _, MAX_RV>(controller, 3);
    assert_eq!(localside::worker_recv_u8::<3, _, _, _, MAX_RV>(worker), 3);
    localside::worker_send_u8::<4, _, _, _, MAX_RV>(worker, 4);
    assert_eq!(
        localside::controller_recv_u8::<4, _, _, _, MAX_RV>(controller),
        4
    );
    localside::controller_send_u8::<5, _, _, _, MAX_RV>(controller, 5);
    assert_eq!(localside::worker_recv_u8::<5, _, _, _, MAX_RV>(worker), 5);
    localside::worker_send_u8::<6, _, _, _, MAX_RV>(worker, 6);
    assert_eq!(
        localside::controller_recv_u8::<6, _, _, _, MAX_RV>(controller),
        6
    );
    localside::controller_send_u8::<7, _, _, _, MAX_RV>(controller, 7);
    assert_eq!(localside::worker_recv_u8::<7, _, _, _, MAX_RV>(worker), 7);
    localside::worker_send_u8::<8, _, _, _, MAX_RV>(worker, 8);
    assert_eq!(
        localside::controller_recv_u8::<8, _, _, _, MAX_RV>(controller),
        8
    );
}

fn run_segment_b<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<9, _, _, _, MAX_RV>(controller, 9);
    assert_eq!(localside::worker_recv_u8::<9, _, _, _, MAX_RV>(worker), 9);
    localside::worker_send_u8::<10, _, _, _, MAX_RV>(worker, 10);
    assert_eq!(
        localside::controller_recv_u8::<10, _, _, _, MAX_RV>(controller),
        10
    );
    localside::controller_send_u8::<11, _, _, _, MAX_RV>(controller, 11);
    assert_eq!(localside::worker_recv_u8::<11, _, _, _, MAX_RV>(worker), 11);
    localside::worker_send_u8::<12, _, _, _, MAX_RV>(worker, 12);
    assert_eq!(
        localside::controller_recv_u8::<12, _, _, _, MAX_RV>(controller),
        12
    );
    localside::controller_send_u8::<13, _, _, _, MAX_RV>(controller, 13);
    assert_eq!(localside::worker_recv_u8::<13, _, _, _, MAX_RV>(worker), 13);
    localside::worker_send_u8::<14, _, _, _, MAX_RV>(worker, 14);
    assert_eq!(
        localside::controller_recv_u8::<14, _, _, _, MAX_RV>(controller),
        14
    );
    localside::controller_send_u8::<15, _, _, _, MAX_RV>(controller, 15);
    assert_eq!(localside::worker_recv_u8::<15, _, _, _, MAX_RV>(worker), 15);
    localside::worker_send_u8::<16, _, _, _, MAX_RV>(worker, 16);
    assert_eq!(
        localside::controller_recv_u8::<16, _, _, _, MAX_RV>(controller),
        16
    );
}

fn run_segment_c<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<17, _, _, _, MAX_RV>(controller, 17);
    assert_eq!(localside::worker_recv_u8::<17, _, _, _, MAX_RV>(worker), 17);
    localside::worker_send_u8::<18, _, _, _, MAX_RV>(worker, 18);
    assert_eq!(
        localside::controller_recv_u8::<18, _, _, _, MAX_RV>(controller),
        18
    );
    localside::controller_send_u8::<19, _, _, _, MAX_RV>(controller, 19);
    assert_eq!(localside::worker_recv_u8::<19, _, _, _, MAX_RV>(worker), 19);
    localside::worker_send_u8::<20, _, _, _, MAX_RV>(worker, 20);
    assert_eq!(
        localside::controller_recv_u8::<20, _, _, _, MAX_RV>(controller),
        20
    );
    localside::controller_send_u8::<21, _, _, _, MAX_RV>(controller, 21);
    assert_eq!(localside::worker_recv_u8::<21, _, _, _, MAX_RV>(worker), 21);
    localside::worker_send_u8::<22, _, _, _, MAX_RV>(worker, 22);
    assert_eq!(
        localside::controller_recv_u8::<22, _, _, _, MAX_RV>(controller),
        22
    );
    localside::controller_send_u8::<23, _, _, _, MAX_RV>(controller, 23);
    assert_eq!(localside::worker_recv_u8::<23, _, _, _, MAX_RV>(worker), 23);
    localside::worker_send_u8::<24, _, _, _, MAX_RV>(worker, 24);
    assert_eq!(
        localside::controller_recv_u8::<24, _, _, _, MAX_RV>(controller),
        24
    );
}

fn run_segment_d<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<81, _, _, _, MAX_RV>(controller, 81);
    assert_eq!(localside::worker_recv_u8::<81, _, _, _, MAX_RV>(worker), 81);
    localside::worker_send_u8::<82, _, _, _, MAX_RV>(worker, 82);
    assert_eq!(
        localside::controller_recv_u8::<82, _, _, _, MAX_RV>(controller),
        82
    );
    localside::controller_send_u8::<83, _, _, _, MAX_RV>(controller, 83);
    assert_eq!(localside::worker_recv_u8::<83, _, _, _, MAX_RV>(worker), 83);
    localside::worker_send_u8::<84, _, _, _, MAX_RV>(worker, 84);
    assert_eq!(
        localside::controller_recv_u8::<84, _, _, _, MAX_RV>(controller),
        84
    );
    localside::controller_send_u8::<85, _, _, _, MAX_RV>(controller, 85);
    assert_eq!(localside::worker_recv_u8::<85, _, _, _, MAX_RV>(worker), 85);
    localside::worker_send_u8::<86, _, _, _, MAX_RV>(worker, 86);
    assert_eq!(
        localside::controller_recv_u8::<86, _, _, _, MAX_RV>(controller),
        86
    );
    localside::controller_send_u8::<87, _, _, _, MAX_RV>(controller, 87);
    assert_eq!(localside::worker_recv_u8::<87, _, _, _, MAX_RV>(worker), 87);
    localside::worker_send_u8::<88, _, _, _, MAX_RV>(worker, 88);
    assert_eq!(
        localside::controller_recv_u8::<88, _, _, _, MAX_RV>(controller),
        88
    );
}

fn run_segment_e<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<89, _, _, _, MAX_RV>(controller, 89);
    assert_eq!(localside::worker_recv_u8::<89, _, _, _, MAX_RV>(worker), 89);
    localside::worker_send_u8::<90, _, _, _, MAX_RV>(worker, 90);
    assert_eq!(
        localside::controller_recv_u8::<90, _, _, _, MAX_RV>(controller),
        90
    );
    localside::controller_send_u8::<91, _, _, _, MAX_RV>(controller, 91);
    assert_eq!(localside::worker_recv_u8::<91, _, _, _, MAX_RV>(worker), 91);
    localside::worker_send_u8::<92, _, _, _, MAX_RV>(worker, 92);
    assert_eq!(
        localside::controller_recv_u8::<92, _, _, _, MAX_RV>(controller),
        92
    );
    localside::controller_send_u8::<93, _, _, _, MAX_RV>(controller, 93);
    assert_eq!(localside::worker_recv_u8::<93, _, _, _, MAX_RV>(worker), 93);
    localside::worker_send_u8::<94, _, _, _, MAX_RV>(worker, 94);
    assert_eq!(
        localside::controller_recv_u8::<94, _, _, _, MAX_RV>(controller),
        94
    );
    localside::controller_send_u8::<95, _, _, _, MAX_RV>(controller, 95);
    assert_eq!(localside::worker_recv_u8::<95, _, _, _, MAX_RV>(worker), 95);
    localside::worker_send_u8::<96, _, _, _, MAX_RV>(worker, 96);
    assert_eq!(
        localside::controller_recv_u8::<96, _, _, _, MAX_RV>(controller),
        96
    );
}

fn run_segment_f<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<97, _, _, _, MAX_RV>(controller, 97);
    assert_eq!(localside::worker_recv_u8::<97, _, _, _, MAX_RV>(worker), 97);
    localside::worker_send_u8::<98, _, _, _, MAX_RV>(worker, 98);
    assert_eq!(
        localside::controller_recv_u8::<98, _, _, _, MAX_RV>(controller),
        98
    );
    localside::controller_send_u8::<99, _, _, _, MAX_RV>(controller, 99);
    assert_eq!(localside::worker_recv_u8::<99, _, _, _, MAX_RV>(worker), 99);
    localside::worker_send_u8::<100, _, _, _, MAX_RV>(worker, 100);
    assert_eq!(
        localside::controller_recv_u8::<100, _, _, _, MAX_RV>(controller),
        100
    );
    localside::controller_send_u8::<101, _, _, _, MAX_RV>(controller, 101);
    assert_eq!(
        localside::worker_recv_u8::<101, _, _, _, MAX_RV>(worker),
        101
    );
    localside::worker_send_u8::<102, _, _, _, MAX_RV>(worker, 102);
    assert_eq!(
        localside::controller_recv_u8::<102, _, _, _, MAX_RV>(controller),
        102
    );
    localside::controller_send_u8::<103, _, _, _, MAX_RV>(controller, 103);
    assert_eq!(
        localside::worker_recv_u8::<103, _, _, _, MAX_RV>(worker),
        103
    );
    localside::worker_send_u8::<104, _, _, _, MAX_RV>(worker, 104);
    assert_eq!(
        localside::controller_recv_u8::<104, _, _, _, MAX_RV>(controller),
        104
    );
}
