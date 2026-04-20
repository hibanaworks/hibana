use hibana::g::advanced::CanonicalControl;
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;

use super::{localside, route_control_kinds, route_localside};

const LABEL_C2W_U8: u8 = 1;
const LABEL_W2C_U8: u8 = 2;
const LABEL_ROUTE_LEFT_CTRL: u8 = 120;
const LABEL_ROUTE_RIGHT_CTRL: u8 = 121;
const LABEL_ROUTE_LEFT_U32: u8 = 84;
const LABEL_ROUTE_RIGHT_U32: u8 = 85;

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

pub fn controller_program() -> RoleProgram<0> {
    let controller_lead_block = || {
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

    let worker_lead_block = || {
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

    let route_segment = || {
        let left = {
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
        let right = {
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
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
        )
    };

    let suffix_block = || {
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

    let program = g::seq(
        controller_lead_block(),
        g::seq(
            worker_lead_block(),
            g::seq(
                controller_lead_block(),
                g::seq(
                    worker_lead_block(),
                    g::seq(
                        route_segment(),
                        g::seq(
                            route_segment(),
                            g::seq(
                                route_segment(),
                                g::seq(
                                    route_segment(),
                                    g::seq(
                                        suffix_block(),
                                        g::seq(
                                            suffix_block(),
                                            g::seq(suffix_block(), suffix_block()),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    );

    let projected: RoleProgram<0> = project(&program);
    projected
}

pub fn worker_program() -> RoleProgram<1> {
    let controller_lead_block = || {
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

    let worker_lead_block = || {
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

    let route_segment = || {
        let left = {
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
        let right = {
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
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<{ LABEL_W2C_U8 }, u8>, 0>(),
        )
    };

    let suffix_block = || {
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

    let program = g::seq(
        controller_lead_block(),
        g::seq(
            worker_lead_block(),
            g::seq(
                controller_lead_block(),
                g::seq(
                    worker_lead_block(),
                    g::seq(
                        route_segment(),
                        g::seq(
                            route_segment(),
                            g::seq(
                                route_segment(),
                                g::seq(
                                    route_segment(),
                                    g::seq(
                                        suffix_block(),
                                        g::seq(
                                            suffix_block(),
                                            g::seq(suffix_block(), suffix_block()),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    );

    let projected: RoleProgram<1> = project(&program);
    projected
}

pub fn run(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    run_prefix(controller, worker);
    run_routes(controller, worker);
    run_suffix(controller, worker);
}

#[inline(never)]
fn controller_worker_roundtrip_values(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
    controller_value: u8,
    worker_value: u8,
) {
    localside::controller_send_u8::<{ LABEL_C2W_U8 }>(controller, controller_value);
    assert_eq!(
        localside::worker_recv_u8::<{ LABEL_C2W_U8 }>(worker),
        controller_value
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, worker_value);
    assert_eq!(
        localside::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller),
        worker_value
    );
}

#[inline(never)]
fn controller_route_roundtrip_ack<const CTRL: u8, K, const PAYLOAD: u8>(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) where
    K: hibana::substrate::cap::ResourceKind
        + hibana::substrate::cap::ControlResourceKind
        + hibana::substrate::cap::advanced::ControlMint
        + 'static,
{
    route_localside::controller_select::<CTRL, K>(controller);
    route_localside::controller_send_u32::<PAYLOAD>(controller, 0);
    assert_eq!(
        route_localside::worker_offer_decode_u32::<PAYLOAD>(worker),
        0
    );
}

#[inline(never)]
fn run_prefix(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    controller_worker_roundtrip_values(controller, worker, 1, 2);
    controller_worker_roundtrip_values(controller, worker, 3, 4);
    controller_worker_roundtrip_values(controller, worker, 5, 6);
    controller_worker_roundtrip_values(controller, worker, 7, 8);
    controller_worker_roundtrip_values(controller, worker, 9, 10);
    controller_worker_roundtrip_values(controller, worker, 11, 12);
    controller_worker_roundtrip_values(controller, worker, 13, 14);
    controller_worker_roundtrip_values(controller, worker, 15, 16);
    controller_worker_roundtrip_values(controller, worker, 17, 18);
    controller_worker_roundtrip_values(controller, worker, 19, 20);
    controller_worker_roundtrip_values(controller, worker, 21, 22);
    controller_worker_roundtrip_values(controller, worker, 23, 24);
    controller_worker_roundtrip_values(controller, worker, 25, 26);
    controller_worker_roundtrip_values(controller, worker, 27, 28);
}

#[inline(never)]
fn run_routes(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    controller_route_roundtrip_ack::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind, { LABEL_ROUTE_LEFT_U32 }>(
        controller,
        worker,
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 92);
    assert_eq!(localside::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 92);
    controller_route_roundtrip_ack::<
        { LABEL_ROUTE_RIGHT_CTRL },
        RouteRightKind,
        { LABEL_ROUTE_RIGHT_U32 },
    >(controller, worker);
    localside::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 93);
    assert_eq!(localside::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 93);
    controller_route_roundtrip_ack::<{ LABEL_ROUTE_LEFT_CTRL }, RouteLeftKind, { LABEL_ROUTE_LEFT_U32 }>(
        controller,
        worker,
    );
    localside::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 94);
    assert_eq!(localside::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 94);
    controller_route_roundtrip_ack::<
        { LABEL_ROUTE_RIGHT_CTRL },
        RouteRightKind,
        { LABEL_ROUTE_RIGHT_U32 },
    >(controller, worker);
    localside::worker_send_u8::<{ LABEL_W2C_U8 }>(worker, 95);
    assert_eq!(localside::controller_recv_u8::<{ LABEL_W2C_U8 }>(controller), 95);
}

#[inline(never)]
fn run_suffix(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    controller_worker_roundtrip_values(controller, worker, 96, 97);
    controller_worker_roundtrip_values(controller, worker, 98, 99);
    controller_worker_roundtrip_values(controller, worker, 100, 101);
    controller_worker_roundtrip_values(controller, worker, 102, 103);
    controller_worker_roundtrip_values(controller, worker, 104, 105);
    controller_worker_roundtrip_values(controller, worker, 106, 107);
    controller_worker_roundtrip_values(controller, worker, 108, 109);
    controller_worker_roundtrip_values(controller, worker, 110, 111);
}
