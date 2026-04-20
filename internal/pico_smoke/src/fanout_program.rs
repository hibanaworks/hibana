use hibana::g::advanced::CanonicalControl;
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;

use super::{localside, route_control_kinds, route_localside};

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

pub fn controller_program() -> RoleProgram<0> {
    let prefix_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>())
    };

    let prefix_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
    };

    let route1_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<120, GenericCapToken<Route1LeftKind>, CanonicalControl<Route1LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<81, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<121, GenericCapToken<Route1RightKind>, CanonicalControl<Route1RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<82, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<97, u8>, 0>(),
        )
    };

    let route2_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<122, GenericCapToken<Route2LeftKind>, CanonicalControl<Route2LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<123, GenericCapToken<Route2RightKind>, CanonicalControl<Route2RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<84, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>(),
        )
    };

    let route3_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<124, GenericCapToken<Route3LeftKind>, CanonicalControl<Route3LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<125, GenericCapToken<Route3RightKind>, CanonicalControl<Route3RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<86, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<99, u8>, 0>(),
        )
    };

    let route4_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<126, GenericCapToken<Route4LeftKind>, CanonicalControl<Route4LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<127, GenericCapToken<Route4RightKind>, CanonicalControl<Route4RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<88, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>(),
        )
    };

    let route5_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<120, GenericCapToken<Route5LeftKind>, CanonicalControl<Route5LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<89, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<121, GenericCapToken<Route5RightKind>, CanonicalControl<Route5RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<90, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<101, u8>, 0>(),
        )
    };

    let route6_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<122, GenericCapToken<Route6LeftKind>, CanonicalControl<Route6LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<123, GenericCapToken<Route6RightKind>, CanonicalControl<Route6RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<92, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>(),
        )
    };

    let route7_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<124, GenericCapToken<Route7LeftKind>, CanonicalControl<Route7LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<125, GenericCapToken<Route7RightKind>, CanonicalControl<Route7RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<94, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<103, u8>, 0>(),
        )
    };

    let route8_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<126, GenericCapToken<Route8LeftKind>, CanonicalControl<Route8LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<127, GenericCapToken<Route8RightKind>, CanonicalControl<Route8RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<96, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
        )
    };

    let suffix_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<105, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<106, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<107, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<108, u8>, 0>())
    };

    let suffix_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<109, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<110, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<111, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<112, u8>, 0>())
    };

    let program = g::seq(
        prefix_a(),
        g::seq(
            prefix_b(),
            g::seq(
                route1_segment(),
                g::seq(
                    route2_segment(),
                    g::seq(
                        route3_segment(),
                        g::seq(
                            route4_segment(),
                            g::seq(
                                route5_segment(),
                                g::seq(
                                    route6_segment(),
                                    g::seq(
                                        route7_segment(),
                                        g::seq(route8_segment(), g::seq(suffix_a(), suffix_b())),
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
    let prefix_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>())
    };

    let prefix_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
    };

    let route1_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<120, GenericCapToken<Route1LeftKind>, CanonicalControl<Route1LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<81, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<121, GenericCapToken<Route1RightKind>, CanonicalControl<Route1RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<82, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<97, u8>, 0>(),
        )
    };

    let route2_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<122, GenericCapToken<Route2LeftKind>, CanonicalControl<Route2LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<123, GenericCapToken<Route2RightKind>, CanonicalControl<Route2RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<84, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>(),
        )
    };

    let route3_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<124, GenericCapToken<Route3LeftKind>, CanonicalControl<Route3LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<125, GenericCapToken<Route3RightKind>, CanonicalControl<Route3RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<86, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<99, u8>, 0>(),
        )
    };

    let route4_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<126, GenericCapToken<Route4LeftKind>, CanonicalControl<Route4LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<127, GenericCapToken<Route4RightKind>, CanonicalControl<Route4RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<88, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>(),
        )
    };

    let route5_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<120, GenericCapToken<Route5LeftKind>, CanonicalControl<Route5LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<89, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<121, GenericCapToken<Route5RightKind>, CanonicalControl<Route5RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<90, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<101, u8>, 0>(),
        )
    };

    let route6_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<122, GenericCapToken<Route6LeftKind>, CanonicalControl<Route6LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<123, GenericCapToken<Route6RightKind>, CanonicalControl<Route6RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<92, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>(),
        )
    };

    let route7_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<124, GenericCapToken<Route7LeftKind>, CanonicalControl<Route7LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<125, GenericCapToken<Route7RightKind>, CanonicalControl<Route7RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<94, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<103, u8>, 0>(),
        )
    };

    let route8_segment = || {
        let left = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<126, GenericCapToken<Route8LeftKind>, CanonicalControl<Route8LeftKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u32>, 0>())
        };
        let right = {
            let program = g::send::<
                Role<0>,
                Role<0>,
                Msg<127, GenericCapToken<Route8RightKind>, CanonicalControl<Route8RightKind>>,
                0,
            >();
            g::seq(program, g::send::<Role<0>, Role<1>, Msg<96, u32>, 0>())
        };
        g::seq(
            g::route(left, right),
            g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
        )
    };

    let suffix_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<105, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<106, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<107, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<108, u8>, 0>())
    };

    let suffix_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<109, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<110, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<111, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<112, u8>, 0>())
    };

    let program = g::seq(
        prefix_a(),
        g::seq(
            prefix_b(),
            g::seq(
                route1_segment(),
                g::seq(
                    route2_segment(),
                    g::seq(
                        route3_segment(),
                        g::seq(
                            route4_segment(),
                            g::seq(
                                route5_segment(),
                                g::seq(
                                    route6_segment(),
                                    g::seq(
                                        route7_segment(),
                                        g::seq(route8_segment(), g::seq(suffix_a(), suffix_b())),
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
fn run_prefix(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<1>(controller, 1);
    assert_eq!(localside::worker_recv_u8::<1>(worker), 1);
    localside::worker_send_u8::<2>(worker, 2);
    assert_eq!(localside::controller_recv_u8::<2>(controller), 2);
    localside::controller_send_u8::<3>(controller, 3);
    assert_eq!(localside::worker_recv_u8::<3>(worker), 3);
    localside::worker_send_u8::<4>(worker, 4);
    assert_eq!(localside::controller_recv_u8::<4>(controller), 4);
    localside::controller_send_u8::<5>(controller, 5);
    assert_eq!(localside::worker_recv_u8::<5>(worker), 5);
    localside::worker_send_u8::<6>(worker, 6);
    assert_eq!(localside::controller_recv_u8::<6>(controller), 6);
    localside::controller_send_u8::<7>(controller, 7);
    assert_eq!(localside::worker_recv_u8::<7>(worker), 7);
    localside::worker_send_u8::<8>(worker, 8);
    assert_eq!(localside::controller_recv_u8::<8>(controller), 8);
}

fn run_routes(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    run_routes_block_1(controller, worker);
    run_routes_block_2(controller, worker);
    run_routes_block_3(controller, worker);
    run_routes_block_4(controller, worker);
}

fn run_routes_block_1(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    route_localside::controller_select::<120, Route1LeftKind>(controller);
    route_localside::controller_send_u32::<81>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<81>(worker), 0);
    localside::worker_send_u8::<97>(worker, 97);
    assert_eq!(localside::controller_recv_u8::<97>(controller), 97);

    route_localside::controller_select::<123, Route2RightKind>(controller);
    route_localside::controller_send_u32::<84>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<84>(worker), 0);
    localside::worker_send_u8::<98>(worker, 98);
    assert_eq!(localside::controller_recv_u8::<98>(controller), 98);
}

fn run_routes_block_2(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    route_localside::controller_select::<124, Route3LeftKind>(controller);
    route_localside::controller_send_u32::<85>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<85>(worker), 0);
    localside::worker_send_u8::<99>(worker, 99);
    assert_eq!(localside::controller_recv_u8::<99>(controller), 99);

    route_localside::controller_select::<127, Route4RightKind>(controller);
    route_localside::controller_send_u32::<88>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<88>(worker), 0);
    localside::worker_send_u8::<100>(worker, 100);
    assert_eq!(localside::controller_recv_u8::<100>(controller), 100);
}

fn run_routes_block_3(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    route_localside::controller_select::<120, Route5LeftKind>(controller);
    route_localside::controller_send_u32::<89>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<89>(worker), 0);
    localside::worker_send_u8::<101>(worker, 101);
    assert_eq!(localside::controller_recv_u8::<101>(controller), 101);

    route_localside::controller_select::<123, Route6RightKind>(controller);
    route_localside::controller_send_u32::<92>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<92>(worker), 0);
    localside::worker_send_u8::<102>(worker, 102);
    assert_eq!(localside::controller_recv_u8::<102>(controller), 102);
}

fn run_routes_block_4(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    route_localside::controller_select::<124, Route7LeftKind>(controller);
    route_localside::controller_send_u32::<93>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<93>(worker), 0);
    localside::worker_send_u8::<103>(worker, 103);
    assert_eq!(localside::controller_recv_u8::<103>(controller), 103);

    route_localside::controller_select::<127, Route8RightKind>(controller);
    route_localside::controller_send_u32::<96>(controller, 0);
    assert_eq!(route_localside::worker_offer_decode_u32::<96>(worker), 0);
    localside::worker_send_u8::<104>(worker, 104);
    assert_eq!(localside::controller_recv_u8::<104>(controller), 104);
}

#[inline(never)]
fn run_suffix(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<105>(controller, 105);
    assert_eq!(localside::worker_recv_u8::<105>(worker), 105);
    localside::worker_send_u8::<106>(worker, 106);
    assert_eq!(localside::controller_recv_u8::<106>(controller), 106);
    localside::controller_send_u8::<107>(controller, 107);
    assert_eq!(localside::worker_recv_u8::<107>(worker), 107);
    localside::worker_send_u8::<108>(worker, 108);
    assert_eq!(localside::controller_recv_u8::<108>(controller), 108);
    localside::controller_send_u8::<109>(controller, 109);
    assert_eq!(localside::worker_recv_u8::<109>(worker), 109);
    localside::worker_send_u8::<110>(worker, 110);
    assert_eq!(localside::controller_recv_u8::<110>(controller), 110);
    localside::controller_send_u8::<111>(controller, 111);
    assert_eq!(localside::worker_recv_u8::<111>(worker), 111);
    localside::worker_send_u8::<112>(worker, 112);
    assert_eq!(localside::controller_recv_u8::<112>(controller), 112);
}
