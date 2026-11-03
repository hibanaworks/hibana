use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

pub const ROUTE_SCOPE_COUNT: usize = 0;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];

pub fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let segment_a = || {
        let program = g::send::<0, 1, Msg<1, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<2, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<3, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<4, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<5, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<6, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<7, u8>>());
        g::seq(program, g::send::<1, 0, Msg<8, u8>>())
    };

    let segment_b = || {
        let program = g::send::<0, 1, Msg<9, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<10, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<11, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<12, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<13, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<14, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<15, u8>>());
        g::seq(program, g::send::<1, 0, Msg<16, u8>>())
    };

    let segment_c = || {
        let program = g::send::<0, 1, Msg<17, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<18, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<19, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<20, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<21, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<22, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<23, u8>>());
        g::seq(program, g::send::<1, 0, Msg<24, u8>>())
    };

    let segment_d = || {
        let program = g::send::<0, 1, Msg<81, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<82, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<83, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<84, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<85, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<86, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<87, u8>>());
        g::seq(program, g::send::<1, 0, Msg<88, u8>>())
    };

    let segment_e = || {
        let program = g::send::<0, 1, Msg<89, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<90, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<91, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<92, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<93, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<94, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<95, u8>>());
        g::seq(program, g::send::<1, 0, Msg<96, u8>>())
    };

    let segment_f = || {
        let program = g::send::<0, 1, Msg<97, u8>>();
        let program = g::seq(program, g::send::<1, 0, Msg<98, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<99, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<100, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<101, u8>>());
        let program = g::seq(program, g::send::<1, 0, Msg<102, u8>>());
        let program = g::seq(program, g::send::<0, 1, Msg<103, u8>>());
        g::seq(program, g::send::<1, 0, Msg<104, u8>>())
    };

    let program = g::seq(
        segment_a(),
        g::seq(
            segment_b(),
            g::seq(
                segment_c(),
                g::seq(segment_d(), g::seq(segment_e(), segment_f())),
            ),
        ),
    );

    let projected: RoleProgram<ROLE> = project(&program);
    projected
}
