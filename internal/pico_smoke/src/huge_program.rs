use hibana::g::advanced::CanonicalControl;
use hibana::g::advanced::steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    Transport,
    cap::GenericCapToken,
    runtime::{Clock, LabelUniverse},
};

use super::{localside, route_control_kinds};

type U8Send<const FROM: u8, const TO: u8, const LABEL: u8> =
    StepCons<SendStep<Role<FROM>, Role<TO>, Msg<LABEL, u8>>, StepNil>;
type U32Send<const FROM: u8, const TO: u8, const LABEL: u8> =
    StepCons<SendStep<Role<FROM>, Role<TO>, Msg<LABEL, u32>>, StepNil>;
type ControlSend<const LABEL: u8, K> = StepCons<
    SendStep<Role<0>, Role<0>, Msg<LABEL, GenericCapToken<K>, CanonicalControl<K>>>,
    StepNil,
>;

const LABEL_C2W_U8: u8 = 1;
const LABEL_W2C_U8: u8 = 2;
const LABEL_ROUTE_LEFT_CTRL: u8 = 120;
const LABEL_ROUTE_RIGHT_CTRL: u8 = 121;
const LABEL_ROUTE_LEFT_U32: u8 = 84;
const LABEL_ROUTE_RIGHT_U32: u8 = 85;

type C2W = U8Send<0, 1, LABEL_C2W_U8>;
type W2C = U8Send<1, 0, LABEL_W2C_U8>;
type RoutePayloadLeft = U32Send<0, 1, LABEL_ROUTE_LEFT_U32>;
type RoutePayloadRight = U32Send<0, 1, LABEL_ROUTE_RIGHT_U32>;
type RouteSelectLeft<K> = ControlSend<LABEL_ROUTE_LEFT_CTRL, K>;
type RouteSelectRight<K> = ControlSend<LABEL_ROUTE_RIGHT_CTRL, K>;

type RouteLeftKind = route_control_kinds::RouteControl<LABEL_ROUTE_LEFT_CTRL, 0>;
type RouteRightKind = route_control_kinds::RouteControl<LABEL_ROUTE_RIGHT_CTRL, 1>;

pub const ROUTE_SCOPE_COUNT: usize = 4;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [
    LABEL_ROUTE_LEFT_U32,
    LABEL_ROUTE_RIGHT_U32,
    LABEL_ROUTE_LEFT_U32,
    LABEL_ROUTE_RIGHT_U32,
];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [LABEL_W2C_U8; ROUTE_SCOPE_COUNT];

type ControllerLead01 = SeqSteps<C2W, W2C>;
type ControllerLead02 = SeqSteps<ControllerLead01, C2W>;
type ControllerLead03 = SeqSteps<ControllerLead02, W2C>;
type ControllerLead04 = SeqSteps<ControllerLead03, C2W>;
type ControllerLead05 = SeqSteps<ControllerLead04, W2C>;
pub type ControllerLeadBlock = SeqSteps<ControllerLead05, C2W>;

type WorkerLead01 = SeqSteps<W2C, C2W>;
type WorkerLead02 = SeqSteps<WorkerLead01, W2C>;
type WorkerLead03 = SeqSteps<WorkerLead02, C2W>;
type WorkerLead04 = SeqSteps<WorkerLead03, W2C>;
type WorkerLead05 = SeqSteps<WorkerLead04, C2W>;
pub type WorkerLeadBlock = SeqSteps<WorkerLead05, W2C>;

pub const CONTROLLER_LEAD_BLOCK: g::Program<ControllerLeadBlock> = {
    let program = g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>();
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    )
};

pub const WORKER_LEAD_BLOCK: g::Program<WorkerLeadBlock> = {
    let program = g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>();
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    )
};

pub type RouteLeftArm = SeqSteps<RouteSelectLeft<RouteLeftKind>, RoutePayloadLeft>;
pub type RouteRightArm = SeqSteps<RouteSelectRight<RouteRightKind>, RoutePayloadRight>;
pub type Route = RouteSteps<RouteLeftArm, RouteRightArm>;

pub const ROUTE_LEFT: g::Program<RouteLeftArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_ROUTE_LEFT_CTRL },
            GenericCapToken<RouteLeftKind>,
            CanonicalControl<RouteLeftKind>,
        >,
        0,
    >();
    g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_ROUTE_LEFT_U32 }, u32>, 0>(),
    )
};

pub const ROUTE_RIGHT: g::Program<RouteRightArm> = {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_ROUTE_RIGHT_CTRL },
            GenericCapToken<RouteRightKind>,
            CanonicalControl<RouteRightKind>,
        >,
        0,
    >();
    g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_ROUTE_RIGHT_U32 }, u32>, 0>(),
    )
};

pub const ROUTE: g::Program<Route> = g::route(ROUTE_LEFT, ROUTE_RIGHT);
pub type RouteSegment = SeqSteps<Route, W2C>;
pub const ROUTE_SEGMENT: g::Program<RouteSegment> = g::seq(
    ROUTE,
    g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
);

type Suffix01 = SeqSteps<C2W, W2C>;
type Suffix02 = SeqSteps<Suffix01, C2W>;
pub type SuffixBlock = SeqSteps<Suffix02, W2C>;
pub const SUFFIX_BLOCK: g::Program<SuffixBlock> = {
    let program = g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>();
    let program = g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    );
    let program = g::seq(
        program,
        g::send::<Role<0>, Role<1>, Msg<{ LABEL_C2W_U8 }, u8>, 0>(),
    );
    g::seq(
        program,
        g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
    )
};

type SuffixTail3 = SeqSteps<SuffixBlock, SuffixBlock>;
type SuffixTail2 = SeqSteps<SuffixBlock, SuffixTail3>;
type SuffixTail1 = SeqSteps<SuffixBlock, SuffixTail2>;
type RouteTail4 = SeqSteps<RouteSegment, SuffixTail1>;
type RouteTail3 = SeqSteps<RouteSegment, RouteTail4>;
type RouteTail2 = SeqSteps<RouteSegment, RouteTail3>;
type RouteTail1 = SeqSteps<RouteSegment, RouteTail2>;
type PrefixTail4 = SeqSteps<WorkerLeadBlock, RouteTail1>;
type PrefixTail3 = SeqSteps<ControllerLeadBlock, PrefixTail4>;
type PrefixTail2 = SeqSteps<WorkerLeadBlock, PrefixTail3>;
pub type ProgramSteps = SeqSteps<ControllerLeadBlock, PrefixTail2>;

pub const PROGRAM: g::Program<ProgramSteps> = g::seq(
    CONTROLLER_LEAD_BLOCK,
    g::seq(
        WORKER_LEAD_BLOCK,
        g::seq(
            CONTROLLER_LEAD_BLOCK,
            g::seq(
                WORKER_LEAD_BLOCK,
                g::seq(
                    ROUTE_SEGMENT,
                    g::seq(
                        ROUTE_SEGMENT,
                        g::seq(
                            ROUTE_SEGMENT,
                            g::seq(
                                ROUTE_SEGMENT,
                                g::seq(
                                    SUFFIX_BLOCK,
                                    g::seq(SUFFIX_BLOCK, g::seq(SUFFIX_BLOCK, SUFFIX_BLOCK)),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    ),
);

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

fn run_prefix<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 1);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        1
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 2);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        2
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 3);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        3
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 4);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        4
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 5);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        5
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 6);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        6
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 7);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        7
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 8);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        8
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 9);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        9
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 10);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        10
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 11);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        11
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 12);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        12
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 13);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        13
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 14);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        14
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 15);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        15
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 16);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        16
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 17);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        17
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 18);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        18
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 19);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        19
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 20);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        20
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 21);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        21
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 22);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        22
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 23);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        23
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 24);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        24
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 25);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        25
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 26);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        26
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 27);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        27
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 28);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        28
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
    localside::controller_select::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind, _, _, _, MAX_RV>(
        controller,
    );
    localside::controller_send_u32::<{ LABEL_ROUTE_LEFT_U32 }, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        localside::worker_offer_decode_u32::<{ LABEL_ROUTE_LEFT_U32 }, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 92);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        92
    );

    localside::controller_select::<{ LABEL_ROUTE_RIGHT_CTRL }, RouteRightKind, _, _, _, MAX_RV>(
        controller,
    );
    localside::controller_send_u32::<{ LABEL_ROUTE_RIGHT_U32 }, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        localside::worker_offer_decode_u32::<{ LABEL_ROUTE_RIGHT_U32 }, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 93);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        93
    );

    localside::controller_select::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind, _, _, _, MAX_RV>(
        controller,
    );
    localside::controller_send_u32::<{ LABEL_ROUTE_LEFT_U32 }, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        localside::worker_offer_decode_u32::<{ LABEL_ROUTE_LEFT_U32 }, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 94);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        94
    );

    localside::controller_select::<{ LABEL_ROUTE_RIGHT_CTRL }, RouteRightKind, _, _, _, MAX_RV>(
        controller,
    );
    localside::controller_send_u32::<{ LABEL_ROUTE_RIGHT_U32 }, _, _, _, MAX_RV>(controller, 0);
    assert_eq!(
        localside::worker_offer_decode_u32::<{ LABEL_ROUTE_RIGHT_U32 }, _, _, _, MAX_RV>(worker),
        0
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 95);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        95
    );
}

fn run_suffix<T, U, C, const MAX_RV: usize>(
    controller: &mut localside::ControllerEndpoint<'_, T, U, C, MAX_RV>,
    worker: &mut localside::WorkerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 96);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        96
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 97);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        97
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 98);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        98
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 99);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        99
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 100);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        100
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 101);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        101
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 102);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        102
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 103);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        103
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 104);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        104
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 105);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        105
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 106);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        106
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 107);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        107
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 108);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        108
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 109);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        109
    );
    localside::controller_send_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(controller, 110);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }, _, _, _, MAX_RV>(worker),
        110
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(worker, 111);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }, _, _, _, MAX_RV>(controller),
        111
    );
}
