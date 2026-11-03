use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

pub const ROUTE_SCOPE_COUNT: usize = 8;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [81, 84, 85, 88, 89, 92, 93, 96];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [97, 98, 99, 100, 101, 102, 103, 104];

pub fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let prefix_a = || {
        let program = g::send::<0, 1, Msg<1, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<2, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<3, u8>>());
        g::seq(program, g::send::<1, 0, Msg<4, u8>>())
    };

    let prefix_b = || {
        let program = g::send::<0, 1, Msg<5, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<6, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<7, u8>>());
        g::seq(program, g::send::<1, 0, Msg<8, u8>>())
    };

    let route1_segment = || {
        let left = g::send::<0, 1, Msg<81, u32>>();
        let right = g::send::<0, 1, Msg<82, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<97, u8>>())
    };

    let route2_segment = || {
        let left = g::send::<0, 1, Msg<83, u32>>();
        let right = g::send::<0, 1, Msg<84, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<98, u8>>())
    };

    let route3_segment = || {
        let left = g::send::<0, 1, Msg<85, u32>>();
        let right = g::send::<0, 1, Msg<86, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<99, u8>>())
    };

    let route4_segment = || {
        let left = g::send::<0, 1, Msg<87, u32>>();
        let right = g::send::<0, 1, Msg<88, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<100, u8>>())
    };

    let route5_segment = || {
        let left = g::send::<0, 1, Msg<89, u32>>();
        let right = g::send::<0, 1, Msg<90, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<101, u8>>())
    };

    let route6_segment = || {
        let left = g::send::<0, 1, Msg<91, u32>>();
        let right = g::send::<0, 1, Msg<92, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<102, u8>>())
    };

    let route7_segment = || {
        let left = g::send::<0, 1, Msg<93, u32>>();
        let right = g::send::<0, 1, Msg<94, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<103, u8>>())
    };

    let route8_segment = || {
        let left = g::send::<0, 1, Msg<95, u32>>();
        let right = g::send::<0, 1, Msg<96, u32>>();
        g::seq(g::route(left, right), g::send::<1, 0, Msg<104, u8>>())
    };

    let suffix_a = || {
        let program = g::send::<0, 1, Msg<85, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<86, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<87, u8>>());
        g::seq(program, g::send::<1, 0, Msg<88, u8>>())
    };

    let suffix_b = || {
        let program = g::send::<0, 1, Msg<89, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<90, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<91, u8>>());
        g::seq(program, g::send::<1, 0, Msg<92, u8>>())
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

    let projected: RoleProgram<ROLE> = project(&program);
    projected
}
