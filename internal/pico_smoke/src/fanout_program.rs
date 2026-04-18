use hibana::g::advanced::CanonicalControl;
use hibana::g::advanced::steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    Transport,
    cap::GenericCapToken,
    runtime::{Clock, LabelUniverse},
};

use super::{localside, route_control_kinds, route_localside};

type U8Send<const FROM: u8, const TO: u8, const LABEL: u8> =
    StepCons<SendStep<Role<FROM>, Role<TO>, Msg<LABEL, u8>>, StepNil>;
type U32Send<const FROM: u8, const TO: u8, const LABEL: u8> =
    StepCons<SendStep<Role<FROM>, Role<TO>, Msg<LABEL, u32>>, StepNil>;
type ControlSend<const LABEL: u8, K> = StepCons<
    SendStep<Role<0>, Role<0>, Msg<LABEL, GenericCapToken<K>, CanonicalControl<K>>>,
    StepNil,
>;

type Route1LeftKind = route_control_kinds::RouteControl<120, 0>;
type Route1RightKind = route_control_kinds::RouteControl<121, 1>;
type Route2LeftKind = route_control_kinds::RouteControl<122, 0>;
type Route2RightKind = route_control_kinds::RouteControl<123, 1>;
type Route3LeftKind = route_control_kinds::RouteControl<124, 0>;
type Route3RightKind = route_control_kinds::RouteControl<125, 1>;
type Route4LeftKind = route_control_kinds::RouteControl<126, 0>;
type Route4RightKind = route_control_kinds::RouteControl<127, 1>;
type Route5LeftKind = route_control_kinds::RouteControl<120, 0>;
type Route5RightKind = route_control_kinds::RouteControl<121, 1>;
type Route6LeftKind = route_control_kinds::RouteControl<122, 0>;
type Route6RightKind = route_control_kinds::RouteControl<123, 1>;
type Route7LeftKind = route_control_kinds::RouteControl<124, 0>;
type Route7RightKind = route_control_kinds::RouteControl<125, 1>;
type Route8LeftKind = route_control_kinds::RouteControl<126, 0>;
type Route8RightKind = route_control_kinds::RouteControl<127, 1>;

pub const ROUTE_SCOPE_COUNT: usize = 8;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [81, 84, 85, 88, 89, 92, 93, 96];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [97, 98, 99, 100, 101, 102, 103, 104];

type PrefixA01 = SeqSteps<U8Send<0, 1, 1>, U8Send<1, 0, 2>>;
type PrefixA02 = SeqSteps<PrefixA01, U8Send<0, 1, 3>>;
pub type PrefixA = SeqSteps<PrefixA02, U8Send<1, 0, 4>>;

type PrefixB01 = SeqSteps<U8Send<0, 1, 5>, U8Send<1, 0, 6>>;
type PrefixB02 = SeqSteps<PrefixB01, U8Send<0, 1, 7>>;
pub type PrefixB = SeqSteps<PrefixB02, U8Send<1, 0, 8>>;

pub type Route1LeftArm = SeqSteps<ControlSend<120, Route1LeftKind>, U32Send<0, 1, 81>>;
pub type Route1RightArm = SeqSteps<ControlSend<121, Route1RightKind>, U32Send<0, 1, 82>>;
pub type Route1 = RouteSteps<Route1LeftArm, Route1RightArm>;
pub const ROUTE1_LEFT: g::Program<Route1LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<120, GenericCapToken<Route1LeftKind>, CanonicalControl<Route1LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<81, u32>, 0>())
};
pub const ROUTE1_RIGHT: g::Program<Route1RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<121, GenericCapToken<Route1RightKind>, CanonicalControl<Route1RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<82, u32>, 0>())
};
pub const ROUTE1: g::Program<Route1> = g::route(ROUTE1_LEFT, ROUTE1_RIGHT);
pub type Route1Segment = SeqSteps<Route1, U8Send<1, 0, 97>>;

pub type Route2LeftArm = SeqSteps<ControlSend<122, Route2LeftKind>, U32Send<0, 1, 83>>;
pub type Route2RightArm = SeqSteps<ControlSend<123, Route2RightKind>, U32Send<0, 1, 84>>;
pub type Route2 = RouteSteps<Route2LeftArm, Route2RightArm>;
pub const ROUTE2_LEFT: g::Program<Route2LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<122, GenericCapToken<Route2LeftKind>, CanonicalControl<Route2LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u32>, 0>())
};
pub const ROUTE2_RIGHT: g::Program<Route2RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<123, GenericCapToken<Route2RightKind>, CanonicalControl<Route2RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<84, u32>, 0>())
};
pub const ROUTE2: g::Program<Route2> = g::route(ROUTE2_LEFT, ROUTE2_RIGHT);
pub type Route2Segment = SeqSteps<Route2, U8Send<1, 0, 98>>;

pub type Route3LeftArm = SeqSteps<ControlSend<124, Route3LeftKind>, U32Send<0, 1, 85>>;
pub type Route3RightArm = SeqSteps<ControlSend<125, Route3RightKind>, U32Send<0, 1, 86>>;
pub type Route3 = RouteSteps<Route3LeftArm, Route3RightArm>;
pub const ROUTE3_LEFT: g::Program<Route3LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<124, GenericCapToken<Route3LeftKind>, CanonicalControl<Route3LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u32>, 0>())
};
pub const ROUTE3_RIGHT: g::Program<Route3RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<125, GenericCapToken<Route3RightKind>, CanonicalControl<Route3RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<86, u32>, 0>())
};
pub const ROUTE3: g::Program<Route3> = g::route(ROUTE3_LEFT, ROUTE3_RIGHT);
pub type Route3Segment = SeqSteps<Route3, U8Send<1, 0, 99>>;

pub type Route4LeftArm = SeqSteps<ControlSend<126, Route4LeftKind>, U32Send<0, 1, 87>>;
pub type Route4RightArm = SeqSteps<ControlSend<127, Route4RightKind>, U32Send<0, 1, 88>>;
pub type Route4 = RouteSteps<Route4LeftArm, Route4RightArm>;
pub const ROUTE4_LEFT: g::Program<Route4LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<126, GenericCapToken<Route4LeftKind>, CanonicalControl<Route4LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u32>, 0>())
};
pub const ROUTE4_RIGHT: g::Program<Route4RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<127, GenericCapToken<Route4RightKind>, CanonicalControl<Route4RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<88, u32>, 0>())
};
pub const ROUTE4: g::Program<Route4> = g::route(ROUTE4_LEFT, ROUTE4_RIGHT);
pub type Route4Segment = SeqSteps<Route4, U8Send<1, 0, 100>>;

pub type Route5LeftArm = SeqSteps<ControlSend<120, Route5LeftKind>, U32Send<0, 1, 89>>;
pub type Route5RightArm = SeqSteps<ControlSend<121, Route5RightKind>, U32Send<0, 1, 90>>;
pub type Route5 = RouteSteps<Route5LeftArm, Route5RightArm>;
pub const ROUTE5_LEFT: g::Program<Route5LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<120, GenericCapToken<Route5LeftKind>, CanonicalControl<Route5LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<89, u32>, 0>())
};
pub const ROUTE5_RIGHT: g::Program<Route5RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<121, GenericCapToken<Route5RightKind>, CanonicalControl<Route5RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<90, u32>, 0>())
};
pub const ROUTE5: g::Program<Route5> = g::route(ROUTE5_LEFT, ROUTE5_RIGHT);
pub type Route5Segment = SeqSteps<Route5, U8Send<1, 0, 101>>;

pub type Route6LeftArm = SeqSteps<ControlSend<122, Route6LeftKind>, U32Send<0, 1, 91>>;
pub type Route6RightArm = SeqSteps<ControlSend<123, Route6RightKind>, U32Send<0, 1, 92>>;
pub type Route6 = RouteSteps<Route6LeftArm, Route6RightArm>;
pub const ROUTE6_LEFT: g::Program<Route6LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<122, GenericCapToken<Route6LeftKind>, CanonicalControl<Route6LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u32>, 0>())
};
pub const ROUTE6_RIGHT: g::Program<Route6RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<123, GenericCapToken<Route6RightKind>, CanonicalControl<Route6RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<92, u32>, 0>())
};
pub const ROUTE6: g::Program<Route6> = g::route(ROUTE6_LEFT, ROUTE6_RIGHT);
pub type Route6Segment = SeqSteps<Route6, U8Send<1, 0, 102>>;

pub type Route7LeftArm = SeqSteps<ControlSend<124, Route7LeftKind>, U32Send<0, 1, 93>>;
pub type Route7RightArm = SeqSteps<ControlSend<125, Route7RightKind>, U32Send<0, 1, 94>>;
pub type Route7 = RouteSteps<Route7LeftArm, Route7RightArm>;
pub const ROUTE7_LEFT: g::Program<Route7LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<124, GenericCapToken<Route7LeftKind>, CanonicalControl<Route7LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u32>, 0>())
};
pub const ROUTE7_RIGHT: g::Program<Route7RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<125, GenericCapToken<Route7RightKind>, CanonicalControl<Route7RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<94, u32>, 0>())
};
pub const ROUTE7: g::Program<Route7> = g::route(ROUTE7_LEFT, ROUTE7_RIGHT);
pub type Route7Segment = SeqSteps<Route7, U8Send<1, 0, 103>>;

pub type Route8LeftArm = SeqSteps<ControlSend<126, Route8LeftKind>, U32Send<0, 1, 95>>;
pub type Route8RightArm = SeqSteps<ControlSend<127, Route8RightKind>, U32Send<0, 1, 96>>;
pub type Route8 = RouteSteps<Route8LeftArm, Route8RightArm>;
pub const ROUTE8_LEFT: g::Program<Route8LeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<126, GenericCapToken<Route8LeftKind>, CanonicalControl<Route8LeftKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u32>, 0>())
};
pub const ROUTE8_RIGHT: g::Program<Route8RightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<127, GenericCapToken<Route8RightKind>, CanonicalControl<Route8RightKind>>,
        0,
    >();
    g::seq(program, g::send::<Role<0>, Role<1>, Msg<96, u32>, 0>())
};
pub const ROUTE8: g::Program<Route8> = g::route(ROUTE8_LEFT, ROUTE8_RIGHT);
pub type Route8Segment = SeqSteps<Route8, U8Send<1, 0, 104>>;

type SuffixA01 = SeqSteps<U8Send<0, 1, 105>, U8Send<1, 0, 106>>;
type SuffixA02 = SeqSteps<SuffixA01, U8Send<0, 1, 107>>;
pub type SuffixA = SeqSteps<SuffixA02, U8Send<1, 0, 108>>;

type SuffixB01 = SeqSteps<U8Send<0, 1, 109>, U8Send<1, 0, 110>>;
type SuffixB02 = SeqSteps<SuffixB01, U8Send<0, 1, 111>>;
pub type SuffixB = SeqSteps<SuffixB02, U8Send<1, 0, 112>>;

type RouteTail7 = SeqSteps<Route7Segment, Route8Segment>;
type RouteTail6 = SeqSteps<Route6Segment, RouteTail7>;
type RouteTail5 = SeqSteps<Route5Segment, RouteTail6>;
type RouteTail4 = SeqSteps<Route4Segment, RouteTail5>;
type RouteTail3 = SeqSteps<Route3Segment, RouteTail4>;
type RouteTail2 = SeqSteps<Route2Segment, RouteTail3>;
type RouteTail1 = SeqSteps<Route1Segment, RouteTail2>;
type SuffixTail = SeqSteps<SuffixA, SuffixB>;
type ProgramTailB = SeqSteps<PrefixB, SeqSteps<RouteTail1, SuffixTail>>;
pub type ProgramSteps = SeqSteps<PrefixA, ProgramTailB>;

const ROUTE1_SEGMENT_SOURCE: g::Program<Route1Segment> =
    g::seq(ROUTE1, g::send::<Role<1>, Role<0>, Msg<97, u8>, 0>());
const ROUTE2_SEGMENT_SOURCE: g::Program<Route2Segment> =
    g::seq(ROUTE2, g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>());
const ROUTE3_SEGMENT_SOURCE: g::Program<Route3Segment> =
    g::seq(ROUTE3, g::send::<Role<1>, Role<0>, Msg<99, u8>, 0>());
const ROUTE4_SEGMENT_SOURCE: g::Program<Route4Segment> =
    g::seq(ROUTE4, g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>());
const ROUTE5_SEGMENT_SOURCE: g::Program<Route5Segment> =
    g::seq(ROUTE5, g::send::<Role<1>, Role<0>, Msg<101, u8>, 0>());
const ROUTE6_SEGMENT_SOURCE: g::Program<Route6Segment> =
    g::seq(ROUTE6, g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>());
const ROUTE7_SEGMENT_SOURCE: g::Program<Route7Segment> =
    g::seq(ROUTE7, g::send::<Role<1>, Role<0>, Msg<103, u8>, 0>());
const ROUTE8_SEGMENT_SOURCE: g::Program<Route8Segment> =
    g::seq(ROUTE8, g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>());

const ROUTE_TAIL7_SOURCE: g::Program<RouteTail7> =
    g::seq(ROUTE7_SEGMENT_SOURCE, ROUTE8_SEGMENT_SOURCE);
const ROUTE_TAIL6_SOURCE: g::Program<RouteTail6> =
    g::seq(ROUTE6_SEGMENT_SOURCE, ROUTE_TAIL7_SOURCE);
const ROUTE_TAIL5_SOURCE: g::Program<RouteTail5> =
    g::seq(ROUTE5_SEGMENT_SOURCE, ROUTE_TAIL6_SOURCE);
const ROUTE_TAIL4_SOURCE: g::Program<RouteTail4> =
    g::seq(ROUTE4_SEGMENT_SOURCE, ROUTE_TAIL5_SOURCE);
const ROUTE_TAIL3_SOURCE: g::Program<RouteTail3> =
    g::seq(ROUTE3_SEGMENT_SOURCE, ROUTE_TAIL4_SOURCE);
const ROUTE_TAIL2_SOURCE: g::Program<RouteTail2> =
    g::seq(ROUTE2_SEGMENT_SOURCE, ROUTE_TAIL3_SOURCE);
const ROUTE_TAIL1_SOURCE: g::Program<RouteTail1> =
    g::seq(ROUTE1_SEGMENT_SOURCE, ROUTE_TAIL2_SOURCE);

pub const PROGRAM: g::Program<ProgramSteps> = g::seq(
    prefix_a(),
    g::seq(
        prefix_b(),
        g::seq(ROUTE_TAIL1_SOURCE, g::seq(suffix_a(), suffix_b())),
    ),
);

const fn prefix_a() -> g::Program<PrefixA> {
    let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>())
}

const fn prefix_b() -> g::Program<PrefixB> {
    let program = g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
}

const fn suffix_a() -> g::Program<SuffixA> {
    let program = g::send::<Role<0>, Role<1>, Msg<105, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<106, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<107, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<108, u8>, 0>())
}

const fn suffix_b() -> g::Program<SuffixB> {
    let program = g::send::<Role<0>, Role<1>, Msg<109, u8>, 0>();
    let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<110, u8>, 0>());
    let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<111, u8>, 0>());
    g::seq(program, g::send::<Role<1>, Role<0>, Msg<112, u8>, 0>())
}

pub fn run<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    run_prefix(controller, worker);
    run_routes(controller, worker);
    run_suffix(controller, worker);
}

#[inline(never)]
fn run_prefix<T, U, C, const MAX_RV: usize>(
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

fn run_routes<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    run_routes_block_1(controller, worker);
    run_routes_block_2(controller, worker);
    run_routes_block_3(controller, worker);
    run_routes_block_4(controller, worker);
}

fn run_routes_block_1<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    route_localside::controller_select::<120, Route1LeftKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<81, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<81, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<97, _, _, _, MAX_RV>(worker, 97);
    assert_eq!(
        localside::controller_recv_u8::<97, _, _, _, MAX_RV>(controller),
        97
    );

    route_localside::controller_select::<123, Route2RightKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<84, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<84, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<98, _, _, _, MAX_RV>(worker, 98);
    assert_eq!(
        localside::controller_recv_u8::<98, _, _, _, MAX_RV>(controller),
        98
    );
}

fn run_routes_block_2<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    route_localside::controller_select::<124, Route3LeftKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<85, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<85, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<99, _, _, _, MAX_RV>(worker, 99);
    assert_eq!(
        localside::controller_recv_u8::<99, _, _, _, MAX_RV>(controller),
        99
    );

    route_localside::controller_select::<127, Route4RightKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<88, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<88, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<100, _, _, _, MAX_RV>(worker, 100);
    assert_eq!(
        localside::controller_recv_u8::<100, _, _, _, MAX_RV>(controller),
        100
    );
}

fn run_routes_block_3<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    route_localside::controller_select::<120, Route5LeftKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<89, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<89, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<101, _, _, _, MAX_RV>(worker, 101);
    assert_eq!(
        localside::controller_recv_u8::<101, _, _, _, MAX_RV>(controller),
        101
    );

    route_localside::controller_select::<123, Route6RightKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<92, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<92, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<102, _, _, _, MAX_RV>(worker, 102);
    assert_eq!(
        localside::controller_recv_u8::<102, _, _, _, MAX_RV>(controller),
        102
    );
}

fn run_routes_block_4<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    route_localside::controller_select::<124, Route7LeftKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<93, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<93, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<103, _, _, _, MAX_RV>(worker, 103);
    assert_eq!(
        localside::controller_recv_u8::<103, _, _, _, MAX_RV>(controller),
        103
    );

    route_localside::controller_select::<127, Route8RightKind, _, _, _, MAX_RV>(controller);
    route_localside::controller_send_u32::<96, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<96, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<104, _, _, _, MAX_RV>(worker, 104);
    assert_eq!(
        localside::controller_recv_u8::<104, _, _, _, MAX_RV>(controller),
        104
    );
}

#[inline(never)]
fn run_suffix<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<105, _, _, _, MAX_RV>(controller, 105);
    assert_eq!(
        localside::worker_recv_u8::<105, _, _, _, MAX_RV>(worker),
        105
    );
    localside::worker_send_u8::<106, _, _, _, MAX_RV>(worker, 106);
    assert_eq!(
        localside::controller_recv_u8::<106, _, _, _, MAX_RV>(controller),
        106
    );
    localside::controller_send_u8::<107, _, _, _, MAX_RV>(controller, 107);
    assert_eq!(
        localside::worker_recv_u8::<107, _, _, _, MAX_RV>(worker),
        107
    );
    localside::worker_send_u8::<108, _, _, _, MAX_RV>(worker, 108);
    assert_eq!(
        localside::controller_recv_u8::<108, _, _, _, MAX_RV>(controller),
        108
    );
    localside::controller_send_u8::<109, _, _, _, MAX_RV>(controller, 109);
    assert_eq!(
        localside::worker_recv_u8::<109, _, _, _, MAX_RV>(worker),
        109
    );
    localside::worker_send_u8::<110, _, _, _, MAX_RV>(worker, 110);
    assert_eq!(
        localside::controller_recv_u8::<110, _, _, _, MAX_RV>(controller),
        110
    );
    localside::controller_send_u8::<111, _, _, _, MAX_RV>(controller, 111);
    assert_eq!(
        localside::worker_recv_u8::<111, _, _, _, MAX_RV>(worker),
        111
    );
    localside::worker_send_u8::<112, _, _, _, MAX_RV>(worker, 112);
    assert_eq!(
        localside::controller_recv_u8::<112, _, _, _, MAX_RV>(controller),
        112
    );
}
