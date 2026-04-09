use hibana::g::advanced::CanonicalControl;
use hibana::g::advanced::steps::{SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;

use super::{route_control_kinds, scenario::ScenarioHarness};

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

pub const CONTROLLER_LEAD_BLOCK: g::ProgramSource<ControllerLeadBlock> = {
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

pub const WORKER_LEAD_BLOCK: g::ProgramSource<WorkerLeadBlock> = {
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
pub type Route = <RouteLeftArm as StepConcat<RouteRightArm>>::Output;

pub const ROUTE_LEFT: g::ProgramSource<RouteLeftArm> = {
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

pub const ROUTE_RIGHT: g::ProgramSource<RouteRightArm> = {
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

pub const ROUTE: g::ProgramSource<Route> = g::route(ROUTE_LEFT, ROUTE_RIGHT);
pub type RouteSegment = SeqSteps<Route, W2C>;
pub const ROUTE_SEGMENT: g::ProgramSource<RouteSegment> = g::seq(
    ROUTE,
    g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
);

type Suffix01 = SeqSteps<C2W, W2C>;
type Suffix02 = SeqSteps<Suffix01, C2W>;
pub type SuffixBlock = SeqSteps<Suffix02, W2C>;
pub const SUFFIX_BLOCK: g::ProgramSource<SuffixBlock> = {
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

pub const PROGRAM: g::ProgramSource<ProgramSteps> = g::seq(
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

pub fn run<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    run_prefix::<H>(controller, worker);
    run_routes::<H>(controller, worker);
    run_suffix::<H>(controller, worker);
}

fn run_prefix<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 1);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 1);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 2);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 2);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 3);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 3);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 4);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 4);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 5);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 5);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 6);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 6);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 7);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 7);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 8);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 8);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 9);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 9);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 10);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 10);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 11);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 11);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 12);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 12);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 13);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 13);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 14);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 14);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 15);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 15);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 16);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 16);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 17);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 17);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 18);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 18);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 19);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 19);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 20);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 20);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 21);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 21);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 22);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 22);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 23);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 23);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 24);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 24);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 25);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 25);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 26);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 26);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 27);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 27);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 28);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 28);
}

fn run_routes<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_select::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind>(controller);
    H::controller_send_u32::<{ LABEL_ROUTE_LEFT_U32 }>(controller, 0);
    assert_eq!(
        H::worker_offer_decode_u32::<{ LABEL_ROUTE_LEFT_U32 }>(worker),
        0
    );
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 92);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 92);

    H::controller_select::<{ LABEL_ROUTE_RIGHT_CTRL }, RouteRightKind>(controller);
    H::controller_send_u32::<{ LABEL_ROUTE_RIGHT_U32 }>(controller, 0);
    assert_eq!(
        H::worker_offer_decode_u32::<{ LABEL_ROUTE_RIGHT_U32 }>(worker),
        0
    );
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 93);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 93);

    H::controller_select::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind>(controller);
    H::controller_send_u32::<{ LABEL_ROUTE_LEFT_U32 }>(controller, 0);
    assert_eq!(
        H::worker_offer_decode_u32::<{ LABEL_ROUTE_LEFT_U32 }>(worker),
        0
    );
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 94);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 94);

    H::controller_select::<{ LABEL_ROUTE_RIGHT_CTRL }, RouteRightKind>(controller);
    H::controller_send_u32::<{ LABEL_ROUTE_RIGHT_U32 }>(controller, 0);
    assert_eq!(
        H::worker_offer_decode_u32::<{ LABEL_ROUTE_RIGHT_U32 }>(worker),
        0
    );
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 95);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 95);
}

fn run_suffix<H: ScenarioHarness>(
    controller: &mut H::ControllerEndpoint<'_>,
    worker: &mut H::WorkerEndpoint<'_>,
) {
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 96);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 96);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 97);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 97);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 98);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 98);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 99);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 99);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 100);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 100);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 101);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 101);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 102);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 102);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 103);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 103);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 104);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 104);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 105);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 105);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 106);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 106);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 107);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 107);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 108);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 108);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 109);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 109);
    H::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, 110);
    assert_eq!(H::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker), 110);
    H::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 111);
    assert_eq!(H::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 111);
}
