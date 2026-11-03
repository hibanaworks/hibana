use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

const CTRL_TO_WORKER_U8_LOGICAL: u8 = 1;
const WORKER_TO_CTRL_U8_LOGICAL: u8 = 2;
const ROUTE_LEFT_PAYLOAD_LOGICAL: u8 = 84;
const ROUTE_RIGHT_PAYLOAD_LOGICAL: u8 = 85;

pub const ROUTE_SCOPE_COUNT: usize = 4;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [
    ROUTE_LEFT_PAYLOAD_LOGICAL,
    ROUTE_RIGHT_PAYLOAD_LOGICAL,
    ROUTE_LEFT_PAYLOAD_LOGICAL,
    ROUTE_RIGHT_PAYLOAD_LOGICAL,
];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [WORKER_TO_CTRL_U8_LOGICAL; ROUTE_SCOPE_COUNT];

pub fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let controller_lead_block = || {
        let program = g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>();
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        )
    };

    let worker_lead_block = || {
        let program = g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>();
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        )
    };

    let route_segment = || {
        let left = g::send::<0, 1, Msg<{ ROUTE_LEFT_PAYLOAD_LOGICAL }, u32>>();
        let right = g::send::<0, 1, Msg<{ ROUTE_RIGHT_PAYLOAD_LOGICAL }, u32>>();
        g::seq(
            g::route(left, right),
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        )
    };

    let suffix_block = || {
        let program = g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>();
        let program = g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
        );
        let program = g::seq(
            program,
            g::send::<0, 1, Msg<{ CTRL_TO_WORKER_U8_LOGICAL }, u8>>(),
        );
        g::seq(
            program,
            g::send::<1, 0, Msg<{ WORKER_TO_CTRL_U8_LOGICAL }, u8>>(),
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

    let projected: RoleProgram<ROLE> = project(&program);
    projected
}
