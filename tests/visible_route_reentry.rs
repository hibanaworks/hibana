mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Message, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const TOP_BODY_REQ: u8 = 151;
const TOP_BODY_ACK: u8 = 152;
const TOP_EXIT: u8 = 153;

const PAR_LEFT: u8 = 156;
const PAR_RIGHT: u8 = 157;
const PAR_EXIT: u8 = 158;

const OUTER_OPEN: u8 = 161;
const INNER_BODY: u8 = 162;
const INNER_EXIT: u8 = 163;
const OUTER_ACK: u8 = 164;
const OUTER_EXIT: u8 = 165;

const NESTED_BODY: u8 = 171;
const NESTED_OTHER: u8 = 172;
const NESTED_TAIL: u8 = 173;
const NESTED_EXIT: u8 = 174;

const NESTED_ROUTE_A_REQ: u8 = 181;
const NESTED_ROUTE_A_ACK: u8 = 182;
const NESTED_ROUTE_B_REQ: u8 = 183;
const NESTED_ROUTE_B_ACK: u8 = 184;
const NESTED_ROUTE_C_REQ: u8 = 185;
const NESTED_ROUTE_C_ACK: u8 = 186;
const DEEP_ROUTE_A_REQ: u8 = 187;
const DEEP_ROUTE_A_ACK: u8 = 188;
const DEEP_ROUTE_B_REQ: u8 = 189;
const DEEP_ROUTE_B_ACK: u8 = 190;
const DEEP_ROUTE_C_REQ: u8 = 191;
const DEEP_ROUTE_C_ACK: u8 = 192;
const DEEP_ROUTE_D_REQ: u8 = 193;
const DEEP_ROUTE_D_ACK: u8 = 194;

const SPINE_A_REQ: u8 = 201;
const SPINE_A_ACK: u8 = 202;
const SPINE_B_REQ: u8 = 203;
const SPINE_B_ACK: u8 = 204;
const SPINE_C_REQ: u8 = 205;
const SPINE_C_ACK: u8 = 206;
const SPINE_D_REQ: u8 = 207;
const SPINE_D_ACK: u8 = 208;
const SPINE_E_REQ: u8 = 209;
const SPINE_E_ACK: u8 = 210;
const SPINE_F_REQ: u8 = 211;
const SPINE_F_ACK: u8 = 212;
const SPINE_G_REQ: u8 = 213;
const SPINE_G_ACK: u8 = 214;
const SPINE_H_REQ: u8 = 215;
const SPINE_H_ACK: u8 = 216;

const SEQ_ARM_A_REQ: u8 = 221;
const SEQ_ARM_A_ACK: u8 = 222;
const SEQ_ARM_B_REQ: u8 = 223;
const SEQ_ARM_B_ACK: u8 = 224;
const SEQ_ARM_C_REQ: u8 = 225;
const SEQ_ARM_C_ACK: u8 = 226;
const SEQ_ARM_D_REQ: u8 = 227;
const SEQ_ARM_D_ACK: u8 = 228;
const ROUTE_SEQ_ROLL_BODY_REQ: u8 = 229;
const ROUTE_SEQ_ROLL_BODY_ACK: u8 = 230;
const ROUTE_SEQ_ROLL_TAIL_REQ: u8 = 231;
const ROUTE_SEQ_ROLL_TAIL_ACK: u8 = 232;
const ROUTE_SEQ_ROLL_OTHER_REQ: u8 = 233;
const ROUTE_SEQ_ROLL_OTHER_ACK: u8 = 234;
const ROUTE_PAR_ROLL_LEFT_REQ: u8 = 235;
const ROUTE_PAR_ROLL_RIGHT_REQ: u8 = 236;
const ROUTE_PAR_ROLL_OTHER_REQ: u8 = 237;
const ROUTE_PAR_ROLL_OTHER_ACK: u8 = 238;
const PAR_ROUTE_ROLL_A_REQ: u8 = 239;
const PAR_ROUTE_ROLL_A_ACK: u8 = 240;
const PAR_ROUTE_ROLL_B_REQ: u8 = 241;
const PAR_ROUTE_ROLL_B_ACK: u8 = 242;
const PAR_ROUTE_ROLL_SIBLING_REQ: u8 = 243;
const PAR_ROUTE_ROLL_SIBLING_ACK: u8 = 244;
const PAR_ROUTE_ROLL_EXIT_REQ: u8 = 245;
const PAR_ROUTE_ROLL_EXIT_ACK: u8 = 246;
const SEQ_ROUTE_ROLL_A_REQ: u8 = 247;
const SEQ_ROUTE_ROLL_A_ACK: u8 = 248;
const SEQ_ROUTE_ROLL_B_REQ: u8 = 249;
const SEQ_ROUTE_ROLL_B_ACK: u8 = 250;
const SEQ_ROUTE_ROLL_TAIL_REQ: u8 = 251;
const SEQ_ROUTE_ROLL_TAIL_ACK: u8 = 252;
const SEQ_ROUTE_ROLL_EXIT_REQ: u8 = 253;
const SEQ_ROUTE_ROLL_EXIT_ACK: u8 = 254;
const MIX_OUTER_LEFT_REQ: u8 = 101;
const MIX_OUTER_LEFT_ACK: u8 = 102;
const MIX_INNER_LEFT_REQ: u8 = 103;
const MIX_INNER_LEFT_ACK: u8 = 104;
const MIX_DEEP_A_REQ: u8 = 105;
const MIX_DEEP_A_ACK: u8 = 106;
const MIX_DEEP_B_REQ: u8 = 107;
const MIX_DEEP_B_ACK: u8 = 108;
const MIX_SEQ_TAIL_REQ: u8 = 109;
const MIX_SEQ_TAIL_ACK: u8 = 110;
const MIX_PAR_SIBLING_REQ: u8 = 111;
const MIX_PAR_SIBLING_ACK: u8 = 112;

const WASI_FD_WRITE_REQ: u8 = 85;
const WASI_FD_WRITE_ACK: u8 = 86;
const WASI_FD_READ_REQ: u8 = 87;
const WASI_FD_READ_ACK: u8 = 88;
const WASI_FD_FDSTAT_REQ: u8 = 89;
const WASI_FD_FDSTAT_ACK: u8 = 90;
const WASI_FD_CLOSE_REQ: u8 = 91;
const WASI_FD_CLOSE_ACK: u8 = 92;
const WASI_PATH_OPEN_REQ: u8 = 127;
const WASI_PATH_OPEN_ACK: u8 = 128;
const WASI_FD_WRITE_REFINED_REQ: u8 = 151;
const WASI_FD_WRITE_REFINED_ACK: u8 = 152;
const WASI_FD_PRESTAT_REQ: u8 = 153;
const WASI_FD_PRESTAT_ACK: u8 = 154;
const WASI_FD_PRESTAT_DIR_REQ: u8 = 155;
const WASI_FD_PRESTAT_DIR_ACK: u8 = 156;
const WASI_FD_FILESTAT_REQ: u8 = 157;
const WASI_FD_FILESTAT_ACK: u8 = 158;
const WASI_PATH_FILESTAT_REQ: u8 = 159;
const WASI_PATH_FILESTAT_ACK: u8 = 160;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

macro_rules! request_response_row {
    ($req:ident, $ack:ident) => {
        g::seq(
            g::send::<0, 1, Msg<$req, u8>>(),
            g::send::<1, 0, Msg<$ack, u8>>(),
        )
    };
}

macro_rules! repeated_read_flow {
    () => {
        g::seq(
            request_response_row!(WASI_FD_READ_REQ, WASI_FD_READ_ACK),
            g::seq(
                request_response_row!(WASI_FD_READ_REQ, WASI_FD_READ_ACK),
                request_response_row!(WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK),
            ),
        )
    };
}

fn visible_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let body = g::seq(
        g::send::<0, 1, Msg<TOP_BODY_REQ, u8>>(),
        g::send::<1, 0, Msg<TOP_BODY_ACK, u8>>(),
    )
    .roll();
    project(&g::seq(body, g::send::<0, 1, Msg<TOP_EXIT, u8>>()))
}

fn visible_parallel_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let body = g::par(
        g::send::<0, 1, Msg<PAR_LEFT, u8>>(),
        g::send::<0, 1, Msg<PAR_RIGHT, u8>>(),
    )
    .roll();
    project(&g::seq(body, g::send::<0, 1, Msg<PAR_EXIT, u8>>()))
}

fn nested_visible_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<INNER_BODY, u8>>(),
        g::send::<0, 1, Msg<INNER_EXIT, u8>>(),
    )
    .roll();
    let outer_body = g::seq(
        g::send::<0, 1, Msg<OUTER_OPEN, u8>>(),
        g::seq(inner, g::send::<1, 0, Msg<OUTER_ACK, u8>>()),
    );
    project(&g::route(outer_body, g::send::<0, 1, Msg<OUTER_EXIT, u8>>()).roll())
}

fn nested_seq_roll_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::seq(
        g::send::<0, 1, Msg<NESTED_BODY, u8>>(),
        g::send::<1, 0, Msg<NESTED_OTHER, u8>>(),
    )
    .roll();
    let outer = g::seq(inner, g::send::<0, 1, Msg<NESTED_TAIL, u8>>()).roll();
    project(&g::seq(outer, g::send::<0, 1, Msg<NESTED_EXIT, u8>>()))
}

fn rolled_nested_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<NESTED_ROUTE_A_REQ, u8>>(),
        g::send::<1, 0, Msg<NESTED_ROUTE_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<NESTED_ROUTE_B_REQ, u8>>(),
        g::send::<1, 0, Msg<NESTED_ROUTE_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<NESTED_ROUTE_C_REQ, u8>>(),
        g::send::<1, 0, Msg<NESTED_ROUTE_C_ACK, u8>>(),
    );
    project(&g::route(a, g::route(b, c)).roll())
}

fn rolled_deep_nested_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<DEEP_ROUTE_A_REQ, u8>>(),
        g::send::<1, 0, Msg<DEEP_ROUTE_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<DEEP_ROUTE_B_REQ, u8>>(),
        g::send::<1, 0, Msg<DEEP_ROUTE_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<DEEP_ROUTE_C_REQ, u8>>(),
        g::send::<1, 0, Msg<DEEP_ROUTE_C_ACK, u8>>(),
    );
    let d = g::seq(
        g::send::<0, 1, Msg<DEEP_ROUTE_D_REQ, u8>>(),
        g::send::<1, 0, Msg<DEEP_ROUTE_D_ACK, u8>>(),
    );
    project(&g::route(a, g::route(g::route(b, c), d)).roll())
}

fn rolled_right_spine_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<SPINE_A_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<SPINE_B_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<SPINE_C_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_C_ACK, u8>>(),
    );
    let d = g::seq(
        g::send::<0, 1, Msg<SPINE_D_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_D_ACK, u8>>(),
    );
    let e = g::seq(
        g::send::<0, 1, Msg<SPINE_E_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_E_ACK, u8>>(),
    );
    let f = g::seq(
        g::send::<0, 1, Msg<SPINE_F_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_F_ACK, u8>>(),
    );
    let g = g::seq(
        g::send::<0, 1, Msg<SPINE_G_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_G_ACK, u8>>(),
    );
    let h = g::seq(
        g::send::<0, 1, Msg<SPINE_H_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_H_ACK, u8>>(),
    );
    project(
        &g::route(
            a,
            g::route(
                b,
                g::route(c, g::route(d, g::route(e, g::route(f, g::route(g, h))))),
            ),
        )
        .roll(),
    )
}

fn rolled_left_spine_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<SPINE_A_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<SPINE_B_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<SPINE_C_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_C_ACK, u8>>(),
    );
    let d = g::seq(
        g::send::<0, 1, Msg<SPINE_D_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_D_ACK, u8>>(),
    );
    let e = g::seq(
        g::send::<0, 1, Msg<SPINE_E_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_E_ACK, u8>>(),
    );
    let f = g::seq(
        g::send::<0, 1, Msg<SPINE_F_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_F_ACK, u8>>(),
    );
    let g = g::seq(
        g::send::<0, 1, Msg<SPINE_G_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_G_ACK, u8>>(),
    );
    let h = g::seq(
        g::send::<0, 1, Msg<SPINE_H_REQ, u8>>(),
        g::send::<1, 0, Msg<SPINE_H_ACK, u8>>(),
    );
    project(
        &g::route(
            g::route(
                g::route(g::route(g::route(g::route(g::route(a, b), c), d), e), f),
                g,
            ),
            h,
        )
        .roll(),
    )
}

fn rolled_right_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_A_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_B_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_C_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_C_ACK, u8>>(),
    );
    project(&g::route(a, g::seq(b, c)).roll())
}

fn rolled_left_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_A_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_B_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_C_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_C_ACK, u8>>(),
    );
    project(&g::route(g::seq(a, b), c).roll())
}

fn rolled_nested_right_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let a = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_A_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_A_ACK, u8>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_B_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_B_ACK, u8>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_C_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_C_ACK, u8>>(),
    );
    let d = g::seq(
        g::send::<0, 1, Msg<SEQ_ARM_D_REQ, u8>>(),
        g::send::<1, 0, Msg<SEQ_ARM_D_ACK, u8>>(),
    );
    project(&g::route(a, g::route(b, g::seq(c, d))).roll())
}

fn rolled_route_left_seq_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = request_response_row!(ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK).roll();
    let rolled_left = g::seq(
        inner,
        request_response_row!(ROUTE_SEQ_ROLL_TAIL_REQ, ROUTE_SEQ_ROLL_TAIL_ACK),
    );
    project(
        &g::route(
            rolled_left,
            request_response_row!(ROUTE_SEQ_ROLL_OTHER_REQ, ROUTE_SEQ_ROLL_OTHER_ACK),
        )
        .roll(),
    )
}

fn rolled_route_right_seq_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = request_response_row!(ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK).roll();
    let rolled_right = g::seq(
        inner,
        request_response_row!(ROUTE_SEQ_ROLL_TAIL_REQ, ROUTE_SEQ_ROLL_TAIL_ACK),
    );
    project(
        &g::route(
            request_response_row!(ROUTE_SEQ_ROLL_OTHER_REQ, ROUTE_SEQ_ROLL_OTHER_ACK),
            rolled_right,
        )
        .roll(),
    )
}

fn rolled_route_left_par_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let rolled_left = g::par(
        g::send::<0, 1, Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(),
        g::send::<0, 1, Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(),
    )
    .roll();
    project(
        &g::route(
            rolled_left,
            request_response_row!(ROUTE_PAR_ROLL_OTHER_REQ, ROUTE_PAR_ROLL_OTHER_ACK),
        )
        .roll(),
    )
}

fn rolled_route_right_par_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let rolled_right = g::par(
        g::send::<0, 1, Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(),
        g::send::<0, 1, Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(),
    )
    .roll();
    project(
        &g::route(
            request_response_row!(ROUTE_PAR_ROLL_OTHER_REQ, ROUTE_PAR_ROLL_OTHER_ACK),
            rolled_right,
        )
        .roll(),
    )
}

fn rolled_par_left_route_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let route = g::route(
        request_response_row!(PAR_ROUTE_ROLL_A_REQ, PAR_ROUTE_ROLL_A_ACK),
        request_response_row!(PAR_ROUTE_ROLL_B_REQ, PAR_ROUTE_ROLL_B_ACK),
    )
    .roll();
    let sibling = request_response_row!(PAR_ROUTE_ROLL_SIBLING_REQ, PAR_ROUTE_ROLL_SIBLING_ACK);
    let body = g::par(route, sibling).roll();
    project(&g::seq(
        body,
        request_response_row!(PAR_ROUTE_ROLL_EXIT_REQ, PAR_ROUTE_ROLL_EXIT_ACK),
    ))
}

fn rolled_par_right_route_roll_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let route = g::route(
        request_response_row!(PAR_ROUTE_ROLL_A_REQ, PAR_ROUTE_ROLL_A_ACK),
        request_response_row!(PAR_ROUTE_ROLL_B_REQ, PAR_ROUTE_ROLL_B_ACK),
    )
    .roll();
    let sibling = request_response_row!(PAR_ROUTE_ROLL_SIBLING_REQ, PAR_ROUTE_ROLL_SIBLING_ACK);
    let body = g::par(sibling, route).roll();
    project(&g::seq(
        body,
        request_response_row!(PAR_ROUTE_ROLL_EXIT_REQ, PAR_ROUTE_ROLL_EXIT_ACK),
    ))
}

fn rolled_seq_route_roll_head_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let route = g::route(
        request_response_row!(SEQ_ROUTE_ROLL_A_REQ, SEQ_ROUTE_ROLL_A_ACK),
        request_response_row!(SEQ_ROUTE_ROLL_B_REQ, SEQ_ROUTE_ROLL_B_ACK),
    )
    .roll();
    let body = g::seq(
        route,
        request_response_row!(SEQ_ROUTE_ROLL_TAIL_REQ, SEQ_ROUTE_ROLL_TAIL_ACK),
    )
    .roll();
    project(&g::seq(
        body,
        request_response_row!(SEQ_ROUTE_ROLL_EXIT_REQ, SEQ_ROUTE_ROLL_EXIT_ACK),
    ))
}

fn rolled_route_route_par_seq_route_roll_mixed_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let deep_route = g::route(
        request_response_row!(MIX_DEEP_A_REQ, MIX_DEEP_A_ACK),
        request_response_row!(MIX_DEEP_B_REQ, MIX_DEEP_B_ACK),
    )
    .roll();
    let seq_roll = g::seq(
        deep_route,
        request_response_row!(MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK),
    )
    .roll();
    let par_roll = g::par(
        seq_roll,
        request_response_row!(MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK),
    )
    .roll();
    let inner_route = g::route(
        request_response_row!(MIX_INNER_LEFT_REQ, MIX_INNER_LEFT_ACK),
        par_roll,
    )
    .roll();
    project(
        &g::route(
            request_response_row!(MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK),
            inner_route,
        )
        .roll(),
    )
}

fn rolled_left_route_left_route_par_seq_route_roll_mixed_program<const ROLE: u8>()
-> RoleProgram<ROLE> {
    let deep_route = g::route(
        request_response_row!(MIX_DEEP_A_REQ, MIX_DEEP_A_ACK),
        request_response_row!(MIX_DEEP_B_REQ, MIX_DEEP_B_ACK),
    )
    .roll();
    let seq_roll = g::seq(
        deep_route,
        request_response_row!(MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK),
    )
    .roll();
    let par_roll = g::par(
        request_response_row!(MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK),
        seq_roll,
    )
    .roll();
    let inner_route = g::route(
        par_roll,
        request_response_row!(MIX_INNER_LEFT_REQ, MIX_INNER_LEFT_ACK),
    )
    .roll();
    project(
        &g::route(
            inner_route,
            request_response_row!(MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK),
        )
        .roll(),
    )
}

fn wasi_shape_rolled_route_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let open_selector_flow = g::seq(
        request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
        request_response_row!(WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK),
    );
    let evidence_read_flow = g::seq(
        request_response_row!(WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK),
        g::seq(
            request_response_row!(WASI_FD_PRESTAT_DIR_REQ, WASI_FD_PRESTAT_DIR_ACK),
            g::seq(
                request_response_row!(WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK),
                g::seq(
                    request_response_row!(WASI_PATH_FILESTAT_REQ, WASI_PATH_FILESTAT_ACK),
                    g::seq(
                        request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
                        g::seq(
                            request_response_row!(WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK),
                            g::seq(
                                request_response_row!(WASI_FD_FILESTAT_REQ, WASI_FD_FILESTAT_ACK),
                                g::seq(
                                    request_response_row!(WASI_FD_READ_REQ, WASI_FD_READ_ACK),
                                    g::seq(
                                        request_response_row!(WASI_FD_READ_REQ, WASI_FD_READ_ACK),
                                        request_response_row!(WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    );
    let device_write_flow = g::seq(
        request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
        g::seq(
            request_response_row!(WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK),
            g::seq(
                request_response_row!(WASI_FD_WRITE_REFINED_REQ, WASI_FD_WRITE_REFINED_ACK),
                g::seq(
                    request_response_row!(WASI_FD_WRITE_REFINED_REQ, WASI_FD_WRITE_REFINED_ACK),
                    g::seq(
                        request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
                        request_response_row!(WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK),
                    ),
                ),
            ),
        ),
    );
    let commit_flow = g::seq(evidence_read_flow, device_write_flow);

    project(
        &g::route(
            request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
            g::route(
                request_response_row!(WASI_FD_READ_REQ, WASI_FD_READ_ACK),
                g::route(open_selector_flow, commit_flow),
            ),
        )
        .roll(),
    )
}

fn rolled_left_repeated_label_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            repeated_read_flow!(),
            request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
        )
        .roll(),
    )
}

fn rolled_nested_right_repeated_label_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
            g::route(
                request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
                repeated_read_flow!(),
            ),
        )
        .roll(),
    )
}

fn rolled_nested_left_repeated_label_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::route(
                repeated_read_flow!(),
                request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
            ),
            request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
        )
        .roll(),
    )
}

fn rolled_balanced_repeated_label_seq_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::route(
                request_response_row!(WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK),
                repeated_read_flow!(),
            ),
            g::route(
                request_response_row!(WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK),
                request_response_row!(WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK),
            ),
        )
        .roll(),
    )
}

fn with_visible_reentry_workspace(
    sid: u32,
    controller_program: RoleProgram<0>,
    worker_program: RoleProgram<1>,
    run: impl FnOnce(&mut hibana::Endpoint<'static, 0>, &mut hibana::Endpoint<'static, 1>),
) {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(sid);
            let mut controller = rv
                .enter(sid, &controller_program)
                .expect("attach controller");
            let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
            run(&mut controller, &mut worker);
        });
    });
}

async fn send_from_controller<const MSG: u8>(
    controller: &mut hibana::Endpoint<'static, 0>,
    value: u8,
) {
    controller
        .send::<Msg<MSG, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MSG}: {err:?}"));
}

async fn send_from_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>, value: u8) {
    worker
        .send::<Msg<MSG, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MSG}: {err:?}"));
}

async fn offer_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>) -> u8 {
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MSG}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MSG, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MSG, u8>>().await.expect("worker recv")
}

async fn recv_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>) -> u8 {
    worker
        .recv::<Msg<MSG, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {MSG}: {err:?}"))
}

async fn recv_controller<const MSG: u8>(controller: &mut hibana::Endpoint<'static, 0>) -> u8 {
    controller
        .recv::<Msg<MSG, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MSG}: {err:?}"))
}

async fn direct_request_response<const REQ: u8, const ACK: u8>(
    controller: &mut hibana::Endpoint<'static, 0>,
    worker: &mut hibana::Endpoint<'static, 1>,
    value: u8,
) {
    send_from_controller::<REQ>(controller, value).await;
    assert_eq!(recv_worker::<REQ>(worker).await, value);
    send_from_worker::<ACK>(worker, value.wrapping_add(1)).await;
    assert_eq!(
        recv_controller::<ACK>(controller).await,
        value.wrapping_add(1)
    );
}

async fn offer_request_response<const REQ: u8, const ACK: u8>(
    controller: &mut hibana::Endpoint<'static, 0>,
    worker: &mut hibana::Endpoint<'static, 1>,
    value: u8,
) {
    send_from_controller::<REQ>(controller, value).await;
    assert_eq!(offer_worker::<REQ>(worker).await, value);
    send_from_worker::<ACK>(worker, value.wrapping_add(1)).await;
    assert_eq!(
        recv_controller::<ACK>(controller).await,
        value.wrapping_add(1)
    );
}

async fn assert_controller_send_blocked<const MSG: u8>(
    controller: &mut hibana::Endpoint<'static, 0>,
) {
    let err = match controller.send::<Msg<MSG, u8>>(&0).await {
        Ok(()) => panic!("controller send label {MSG} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "controller send label {MSG} must remain blocked by roll/par progress: {rendered}"
    );
}

async fn assert_worker_send_blocked<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>) {
    let err = match worker.send::<Msg<MSG, u8>>(&0).await {
        Ok(()) => panic!("worker send label {MSG} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "worker send label {MSG} must remain blocked by roll progress: {rendered}"
    );
}

#[test]
fn rolled_seq_reenters_by_repeated_head_without_loop_control() {
    with_visible_reentry_workspace(
        960,
        visible_reentry_program::<0>(),
        visible_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<TOP_BODY_REQ>(controller, 10).await;
                assert_eq!(recv_worker::<TOP_BODY_REQ>(worker).await, 10);
                send_from_worker::<TOP_BODY_ACK>(worker, 11).await;
                assert_eq!(
                    controller
                        .recv::<Msg<TOP_BODY_ACK, u8>>()
                        .await
                        .expect("controller recv first ack"),
                    11
                );

                send_from_controller::<TOP_BODY_REQ>(controller, 20).await;
                assert_eq!(recv_worker::<TOP_BODY_REQ>(worker).await, 20);
                send_from_worker::<TOP_BODY_ACK>(worker, 21).await;
                assert_eq!(
                    controller
                        .recv::<Msg<TOP_BODY_ACK, u8>>()
                        .await
                        .expect("controller recv second ack"),
                    21
                );

                send_from_controller::<TOP_EXIT>(controller, 99).await;
                assert_eq!(recv_worker::<TOP_EXIT>(worker).await, 99);
            });
        },
    );
}

#[test]
fn rolled_par_reenters_only_after_both_lanes_settle() {
    with_visible_reentry_workspace(
        962,
        visible_parallel_reentry_program::<0>(),
        visible_parallel_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<PAR_LEFT>(controller, 10).await;
                assert_controller_send_blocked::<PAR_LEFT>(controller).await;
                assert_controller_send_blocked::<PAR_EXIT>(controller).await;

                send_from_controller::<PAR_RIGHT>(controller, 11).await;
                send_from_controller::<PAR_LEFT>(controller, 20).await;
                assert_controller_send_blocked::<PAR_EXIT>(controller).await;

                send_from_controller::<PAR_RIGHT>(controller, 21).await;
                send_from_controller::<PAR_EXIT>(controller, 99).await;

                assert_eq!(recv_worker::<PAR_LEFT>(worker).await, 10);
                assert_eq!(recv_worker::<PAR_RIGHT>(worker).await, 11);
                assert_eq!(recv_worker::<PAR_LEFT>(worker).await, 20);
                assert_eq!(recv_worker::<PAR_RIGHT>(worker).await, 21);
                assert_eq!(recv_worker::<PAR_EXIT>(worker).await, 99);
            });
        },
    );
}

#[test]
fn nested_rolled_route_reenters_before_outer_body_continues() {
    with_visible_reentry_workspace(
        961,
        nested_visible_reentry_program::<0>(),
        nested_visible_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<OUTER_OPEN>(controller, 1).await;
                assert_eq!(offer_worker::<OUTER_OPEN>(worker).await, 1);

                send_from_controller::<INNER_BODY>(controller, 2).await;
                assert_eq!(offer_worker::<INNER_BODY>(worker).await, 2);

                send_from_controller::<INNER_BODY>(controller, 3).await;
                assert_eq!(offer_worker::<INNER_BODY>(worker).await, 3);

                send_from_controller::<INNER_EXIT>(controller, 4).await;
                assert_eq!(offer_worker::<INNER_EXIT>(worker).await, 4);

                send_from_worker::<OUTER_ACK>(worker, 5).await;
                assert_eq!(
                    controller
                        .recv::<Msg<OUTER_ACK, u8>>()
                        .await
                        .expect("controller recv outer ack"),
                    5
                );

                send_from_controller::<OUTER_EXIT>(controller, 6).await;
                assert_eq!(offer_worker::<OUTER_EXIT>(worker).await, 6);
            });
        },
    );
}

#[test]
fn nested_roll_scopes_reenter_inner_until_outer_scope_completes() {
    with_visible_reentry_workspace(
        963,
        nested_seq_roll_program::<0>(),
        nested_seq_roll_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<NESTED_BODY>(controller, 10).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 10);
                send_from_worker::<NESTED_OTHER>(worker, 11).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 11);

                send_from_controller::<NESTED_BODY>(controller, 20).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 20);
                send_from_worker::<NESTED_OTHER>(worker, 21).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 21);

                send_from_controller::<NESTED_TAIL>(controller, 30).await;
                assert_eq!(recv_worker::<NESTED_TAIL>(worker).await, 30);

                send_from_controller::<NESTED_BODY>(controller, 40).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 40);
                send_from_worker::<NESTED_OTHER>(worker, 41).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 41);
                assert_controller_send_blocked::<NESTED_EXIT>(controller).await;

                send_from_controller::<NESTED_TAIL>(controller, 50).await;
                assert_eq!(recv_worker::<NESTED_TAIL>(worker).await, 50);
                send_from_controller::<NESTED_EXIT>(controller, 60).await;
                assert_eq!(recv_worker::<NESTED_EXIT>(worker).await, 60);
            });
        },
    );
}

#[test]
fn rolled_nested_route_reenters_to_sibling_nested_arm() {
    with_visible_reentry_workspace(
        964,
        rolled_nested_route_program::<0>(),
        rolled_nested_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<NESTED_ROUTE_B_REQ>(controller, 10).await;
                assert_eq!(offer_worker::<NESTED_ROUTE_B_REQ>(worker).await, 10);
                send_from_worker::<NESTED_ROUTE_B_ACK>(worker, 11).await;
                assert_eq!(recv_controller::<NESTED_ROUTE_B_ACK>(controller).await, 11);

                send_from_controller::<NESTED_ROUTE_C_REQ>(controller, 20).await;
                assert_eq!(offer_worker::<NESTED_ROUTE_C_REQ>(worker).await, 20);
                send_from_worker::<NESTED_ROUTE_C_ACK>(worker, 21).await;
                assert_eq!(recv_controller::<NESTED_ROUTE_C_ACK>(controller).await, 21);
            });
        },
    );
}

#[test]
fn rolled_route_reenters_from_left_arm_to_nested_right_arm() {
    with_visible_reentry_workspace(
        965,
        rolled_nested_route_program::<0>(),
        rolled_nested_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<NESTED_ROUTE_A_REQ>(controller, 10).await;
                assert_eq!(offer_worker::<NESTED_ROUTE_A_REQ>(worker).await, 10);
                send_from_worker::<NESTED_ROUTE_A_ACK>(worker, 11).await;
                assert_eq!(recv_controller::<NESTED_ROUTE_A_ACK>(controller).await, 11);

                send_from_controller::<NESTED_ROUTE_B_REQ>(controller, 20).await;
                assert_eq!(offer_worker::<NESTED_ROUTE_B_REQ>(worker).await, 20);
                send_from_worker::<NESTED_ROUTE_B_ACK>(worker, 21).await;
                assert_eq!(recv_controller::<NESTED_ROUTE_B_ACK>(controller).await, 21);
            });
        },
    );
}

#[test]
fn rolled_route_reenters_from_completed_outer_arm_to_deep_nested_arm() {
    with_visible_reentry_workspace(
        966,
        rolled_deep_nested_route_program::<0>(),
        rolled_deep_nested_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<DEEP_ROUTE_A_REQ>(controller, 10).await;
                assert_eq!(offer_worker::<DEEP_ROUTE_A_REQ>(worker).await, 10);
                send_from_worker::<DEEP_ROUTE_A_ACK>(worker, 11).await;
                assert_eq!(recv_controller::<DEEP_ROUTE_A_ACK>(controller).await, 11);

                send_from_controller::<DEEP_ROUTE_B_REQ>(controller, 20).await;
                assert_eq!(offer_worker::<DEEP_ROUTE_B_REQ>(worker).await, 20);
                send_from_worker::<DEEP_ROUTE_B_ACK>(worker, 21).await;
                assert_eq!(recv_controller::<DEEP_ROUTE_B_ACK>(controller).await, 21);
            });
        },
    );
}

#[test]
fn rolled_right_spine_reenters_repeated_deep_rightmost_arm() {
    with_visible_reentry_workspace(
        967,
        rolled_right_spine_route_program::<0>(),
        rolled_right_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                direct_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 10).await;
                direct_request_response::<SPINE_B_REQ, SPINE_B_ACK>(controller, worker, 20).await;
                direct_request_response::<SPINE_C_REQ, SPINE_C_ACK>(controller, worker, 30).await;
                direct_request_response::<SPINE_D_REQ, SPINE_D_ACK>(controller, worker, 40).await;
                direct_request_response::<SPINE_E_REQ, SPINE_E_ACK>(controller, worker, 50).await;
                direct_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 60).await;
                direct_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 70).await;
            });
        },
    );
}

#[test]
fn rolled_right_spine_rejects_deep_response_without_reentered_request() {
    with_visible_reentry_workspace(
        970,
        rolled_right_spine_route_program::<0>(),
        rolled_right_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                direct_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 10).await;
                direct_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 20).await;
                assert_worker_send_blocked::<SPINE_H_ACK>(worker).await;
            });
        },
    );
}

#[test]
fn rolled_right_spine_offer_reenters_through_left_chain() {
    with_visible_reentry_workspace(
        968,
        rolled_right_spine_route_program::<0>(),
        rolled_right_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 10).await;
                offer_request_response::<SPINE_B_REQ, SPINE_B_ACK>(controller, worker, 20).await;
                offer_request_response::<SPINE_C_REQ, SPINE_C_ACK>(controller, worker, 30).await;
                offer_request_response::<SPINE_D_REQ, SPINE_D_ACK>(controller, worker, 40).await;
                offer_request_response::<SPINE_E_REQ, SPINE_E_ACK>(controller, worker, 50).await;
            });
        },
    );
}

#[test]
fn rolled_right_spine_offer_reenters_repeated_deep_rightmost_arm() {
    with_visible_reentry_workspace(
        971,
        rolled_right_spine_route_program::<0>(),
        rolled_right_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 10).await;
                offer_request_response::<SPINE_B_REQ, SPINE_B_ACK>(controller, worker, 20).await;
                offer_request_response::<SPINE_C_REQ, SPINE_C_ACK>(controller, worker, 30).await;
                offer_request_response::<SPINE_D_REQ, SPINE_D_ACK>(controller, worker, 40).await;
                offer_request_response::<SPINE_E_REQ, SPINE_E_ACK>(controller, worker, 50).await;
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 60).await;
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 70).await;
            });
        },
    );
}

#[test]
fn rolled_left_spine_reenters_repeated_deep_leftmost_arm() {
    with_visible_reentry_workspace(
        969,
        rolled_left_spine_route_program::<0>(),
        rolled_left_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                direct_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 10).await;
                direct_request_response::<SPINE_G_REQ, SPINE_G_ACK>(controller, worker, 20).await;
                direct_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 30).await;
                direct_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 40).await;
            });
        },
    );
}

#[test]
fn rolled_left_spine_offer_enters_rightmost_arm() {
    with_visible_reentry_workspace(
        973,
        rolled_left_spine_route_program::<0>(),
        rolled_left_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 10).await;
            });
        },
    );
}

#[test]
fn rolled_left_spine_offer_reenters_inner_right_arm() {
    with_visible_reentry_workspace(
        974,
        rolled_left_spine_route_program::<0>(),
        rolled_left_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 10).await;
                offer_request_response::<SPINE_G_REQ, SPINE_G_ACK>(controller, worker, 20).await;
            });
        },
    );
}

#[test]
fn rolled_left_spine_offer_reenters_deep_left_arm_once() {
    with_visible_reentry_workspace(
        975,
        rolled_left_spine_route_program::<0>(),
        rolled_left_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 10).await;
                offer_request_response::<SPINE_G_REQ, SPINE_G_ACK>(controller, worker, 20).await;
                offer_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 30).await;
            });
        },
    );
}

#[test]
fn rolled_left_spine_offer_reenters_repeated_deep_leftmost_arm() {
    with_visible_reentry_workspace(
        972,
        rolled_left_spine_route_program::<0>(),
        rolled_left_spine_route_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SPINE_H_REQ, SPINE_H_ACK>(controller, worker, 10).await;
                offer_request_response::<SPINE_G_REQ, SPINE_G_ACK>(controller, worker, 20).await;
                offer_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 30).await;
                offer_request_response::<SPINE_A_REQ, SPINE_A_ACK>(controller, worker, 40).await;
            });
        },
    );
}

#[test]
fn rolled_route_right_seq_arm_keeps_send_authority_for_continuation() {
    with_visible_reentry_workspace(
        976,
        rolled_right_seq_arm_program::<0>(),
        rolled_right_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SEQ_ARM_B_REQ, SEQ_ARM_B_ACK>(controller, worker, 10)
                    .await;
                direct_request_response::<SEQ_ARM_C_REQ, SEQ_ARM_C_ACK>(controller, worker, 20)
                    .await;
            });
        },
    );
}

#[test]
fn rolled_route_left_seq_arm_keeps_send_authority_for_continuation() {
    with_visible_reentry_workspace(
        977,
        rolled_left_seq_arm_program::<0>(),
        rolled_left_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SEQ_ARM_A_REQ, SEQ_ARM_A_ACK>(controller, worker, 10)
                    .await;
                direct_request_response::<SEQ_ARM_B_REQ, SEQ_ARM_B_ACK>(controller, worker, 20)
                    .await;
            });
        },
    );
}

#[test]
fn rolled_nested_right_route_seq_arm_keeps_send_authority_for_continuation() {
    with_visible_reentry_workspace(
        978,
        rolled_nested_right_seq_arm_program::<0>(),
        rolled_nested_right_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SEQ_ARM_C_REQ, SEQ_ARM_C_ACK>(controller, worker, 10)
                    .await;
                direct_request_response::<SEQ_ARM_D_REQ, SEQ_ARM_D_ACK>(controller, worker, 20)
                    .await;
            });
        },
    );
}

#[test]
fn rolled_route_left_seq_roll_arm_reenters_inner_before_tail() {
    with_visible_reentry_workspace(
        986,
        rolled_route_left_seq_roll_arm_program::<0>(),
        rolled_route_left_seq_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<ROUTE_SEQ_ROLL_TAIL_REQ, ROUTE_SEQ_ROLL_TAIL_ACK>(
                    controller, worker, 30,
                )
                .await;
                offer_request_response::<ROUTE_SEQ_ROLL_OTHER_REQ, ROUTE_SEQ_ROLL_OTHER_ACK>(
                    controller, worker, 40,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_right_seq_roll_arm_reenters_inner_before_tail() {
    with_visible_reentry_workspace(
        987,
        rolled_route_right_seq_roll_arm_program::<0>(),
        rolled_route_right_seq_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<ROUTE_SEQ_ROLL_OTHER_REQ, ROUTE_SEQ_ROLL_OTHER_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK>(
                    controller, worker, 20,
                )
                .await;
                offer_request_response::<ROUTE_SEQ_ROLL_BODY_REQ, ROUTE_SEQ_ROLL_BODY_ACK>(
                    controller, worker, 30,
                )
                .await;
                direct_request_response::<ROUTE_SEQ_ROLL_TAIL_REQ, ROUTE_SEQ_ROLL_TAIL_ACK>(
                    controller, worker, 40,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_left_par_roll_arm_reenters_only_after_parallel_settles() {
    with_visible_reentry_workspace(
        988,
        rolled_route_left_par_roll_arm_program::<0>(),
        rolled_route_left_par_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<ROUTE_PAR_ROLL_LEFT_REQ>(controller, 10).await;
                assert_eq!(offer_worker::<ROUTE_PAR_ROLL_LEFT_REQ>(worker).await, 10);
                assert_controller_send_blocked::<ROUTE_PAR_ROLL_OTHER_REQ>(controller).await;

                send_from_controller::<ROUTE_PAR_ROLL_RIGHT_REQ>(controller, 20).await;
                assert_eq!(recv_worker::<ROUTE_PAR_ROLL_RIGHT_REQ>(worker).await, 20);

                send_from_controller::<ROUTE_PAR_ROLL_LEFT_REQ>(controller, 30).await;
                assert_eq!(offer_worker::<ROUTE_PAR_ROLL_LEFT_REQ>(worker).await, 30);
                send_from_controller::<ROUTE_PAR_ROLL_RIGHT_REQ>(controller, 40).await;
                assert_eq!(recv_worker::<ROUTE_PAR_ROLL_RIGHT_REQ>(worker).await, 40);

                offer_request_response::<ROUTE_PAR_ROLL_OTHER_REQ, ROUTE_PAR_ROLL_OTHER_ACK>(
                    controller, worker, 50,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_left_par_roll_arm_exits_after_parallel_settles_without_probe() {
    with_visible_reentry_workspace(
        993,
        rolled_route_left_par_roll_arm_program::<0>(),
        rolled_route_left_par_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<ROUTE_PAR_ROLL_LEFT_REQ>(controller, 10).await;
                assert_eq!(offer_worker::<ROUTE_PAR_ROLL_LEFT_REQ>(worker).await, 10);
                send_from_controller::<ROUTE_PAR_ROLL_RIGHT_REQ>(controller, 20).await;
                assert_eq!(recv_worker::<ROUTE_PAR_ROLL_RIGHT_REQ>(worker).await, 20);

                offer_request_response::<ROUTE_PAR_ROLL_OTHER_REQ, ROUTE_PAR_ROLL_OTHER_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_right_par_roll_arm_reenters_only_after_parallel_settles() {
    with_visible_reentry_workspace(
        989,
        rolled_route_right_par_roll_arm_program::<0>(),
        rolled_route_right_par_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<ROUTE_PAR_ROLL_OTHER_REQ, ROUTE_PAR_ROLL_OTHER_ACK>(
                    controller, worker, 10,
                )
                .await;

                send_from_controller::<ROUTE_PAR_ROLL_RIGHT_REQ>(controller, 20).await;
                assert_eq!(offer_worker::<ROUTE_PAR_ROLL_RIGHT_REQ>(worker).await, 20);
                assert_controller_send_blocked::<ROUTE_PAR_ROLL_OTHER_REQ>(controller).await;

                send_from_controller::<ROUTE_PAR_ROLL_LEFT_REQ>(controller, 30).await;
                assert_eq!(recv_worker::<ROUTE_PAR_ROLL_LEFT_REQ>(worker).await, 30);
                send_from_controller::<ROUTE_PAR_ROLL_RIGHT_REQ>(controller, 40).await;
                assert_eq!(offer_worker::<ROUTE_PAR_ROLL_RIGHT_REQ>(worker).await, 40);
                send_from_controller::<ROUTE_PAR_ROLL_LEFT_REQ>(controller, 50).await;
                assert_eq!(recv_worker::<ROUTE_PAR_ROLL_LEFT_REQ>(worker).await, 50);
            });
        },
    );
}

#[test]
fn rolled_par_left_route_roll_arm_reenters_before_parallel_sibling_settles() {
    with_visible_reentry_workspace(
        990,
        rolled_par_left_route_roll_arm_program::<0>(),
        rolled_par_left_route_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<PAR_ROUTE_ROLL_A_REQ, PAR_ROUTE_ROLL_A_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<PAR_ROUTE_ROLL_B_REQ, PAR_ROUTE_ROLL_B_ACK>(
                    controller, worker, 20,
                )
                .await;
                assert_controller_send_blocked::<PAR_ROUTE_ROLL_EXIT_REQ>(controller).await;

                direct_request_response::<PAR_ROUTE_ROLL_SIBLING_REQ, PAR_ROUTE_ROLL_SIBLING_ACK>(
                    controller, worker, 30,
                )
                .await;
                direct_request_response::<PAR_ROUTE_ROLL_EXIT_REQ, PAR_ROUTE_ROLL_EXIT_ACK>(
                    controller, worker, 40,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_par_right_route_roll_arm_reenters_before_parallel_sibling_settles() {
    with_visible_reentry_workspace(
        991,
        rolled_par_right_route_roll_arm_program::<0>(),
        rolled_par_right_route_roll_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                direct_request_response::<PAR_ROUTE_ROLL_SIBLING_REQ, PAR_ROUTE_ROLL_SIBLING_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<PAR_ROUTE_ROLL_B_REQ, PAR_ROUTE_ROLL_B_ACK>(
                    controller, worker, 20,
                )
                .await;
                offer_request_response::<PAR_ROUTE_ROLL_A_REQ, PAR_ROUTE_ROLL_A_ACK>(
                    controller, worker, 30,
                )
                .await;
                direct_request_response::<PAR_ROUTE_ROLL_EXIT_REQ, PAR_ROUTE_ROLL_EXIT_ACK>(
                    controller, worker, 40,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_seq_route_roll_head_reenters_before_seq_tail_and_outer_exit() {
    with_visible_reentry_workspace(
        992,
        rolled_seq_route_roll_head_program::<0>(),
        rolled_seq_route_roll_head_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<SEQ_ROUTE_ROLL_A_REQ, SEQ_ROUTE_ROLL_A_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<SEQ_ROUTE_ROLL_B_REQ, SEQ_ROUTE_ROLL_B_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<SEQ_ROUTE_ROLL_TAIL_REQ, SEQ_ROUTE_ROLL_TAIL_ACK>(
                    controller, worker, 30,
                )
                .await;

                offer_request_response::<SEQ_ROUTE_ROLL_A_REQ, SEQ_ROUTE_ROLL_A_ACK>(
                    controller, worker, 40,
                )
                .await;
                direct_request_response::<SEQ_ROUTE_ROLL_TAIL_REQ, SEQ_ROUTE_ROLL_TAIL_ACK>(
                    controller, worker, 50,
                )
                .await;
                direct_request_response::<SEQ_ROUTE_ROLL_EXIT_REQ, SEQ_ROUTE_ROLL_EXIT_ACK>(
                    controller, worker, 60,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_route_par_seq_route_roll_mixed_reentry_keeps_single_authority() {
    with_visible_reentry_workspace(
        994,
        rolled_route_route_par_seq_route_roll_mixed_program::<0>(),
        rolled_route_route_par_seq_route_roll_mixed_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<MIX_INNER_LEFT_REQ, MIX_INNER_LEFT_ACK>(
                    controller, worker, 20,
                )
                .await;

                offer_request_response::<MIX_DEEP_A_REQ, MIX_DEEP_A_ACK>(controller, worker, 30)
                    .await;
                offer_request_response::<MIX_DEEP_B_REQ, MIX_DEEP_B_ACK>(controller, worker, 40)
                    .await;
                assert_controller_send_blocked::<MIX_OUTER_LEFT_REQ>(controller).await;

                direct_request_response::<MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK>(
                    controller, worker, 50,
                )
                .await;
                assert_controller_send_blocked::<MIX_OUTER_LEFT_REQ>(controller).await;

                direct_request_response::<MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK>(
                    controller, worker, 60,
                )
                .await;
                offer_request_response::<MIX_DEEP_B_REQ, MIX_DEEP_B_ACK>(controller, worker, 70)
                    .await;
                direct_request_response::<MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK>(
                    controller, worker, 80,
                )
                .await;
                direct_request_response::<MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK>(
                    controller, worker, 90,
                )
                .await;

                offer_request_response::<MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK>(
                    controller, worker, 100,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_left_route_left_route_par_seq_route_roll_mixed_reentry_keeps_single_authority() {
    with_visible_reentry_workspace(
        995,
        rolled_left_route_left_route_par_seq_route_roll_mixed_program::<0>(),
        rolled_left_route_left_route_par_seq_route_roll_mixed_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK>(
                    controller, worker, 10,
                )
                .await;
                offer_request_response::<MIX_INNER_LEFT_REQ, MIX_INNER_LEFT_ACK>(
                    controller, worker, 20,
                )
                .await;

                direct_request_response::<MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK>(
                    controller, worker, 30,
                )
                .await;
                offer_request_response::<MIX_DEEP_A_REQ, MIX_DEEP_A_ACK>(controller, worker, 40)
                    .await;
                offer_request_response::<MIX_DEEP_B_REQ, MIX_DEEP_B_ACK>(controller, worker, 50)
                    .await;
                assert_controller_send_blocked::<MIX_OUTER_LEFT_REQ>(controller).await;

                direct_request_response::<MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK>(
                    controller, worker, 60,
                )
                .await;
                offer_request_response::<MIX_DEEP_B_REQ, MIX_DEEP_B_ACK>(controller, worker, 70)
                    .await;
                direct_request_response::<MIX_SEQ_TAIL_REQ, MIX_SEQ_TAIL_ACK>(
                    controller, worker, 80,
                )
                .await;
                direct_request_response::<MIX_PAR_SIBLING_REQ, MIX_PAR_SIBLING_ACK>(
                    controller, worker, 90,
                )
                .await;

                offer_request_response::<MIX_OUTER_LEFT_REQ, MIX_OUTER_LEFT_ACK>(
                    controller, worker, 100,
                )
                .await;
            });
        },
    );
}

#[test]
fn wasi_shape_rolled_route_seq_arm_keeps_send_authority_for_continuation() {
    with_visible_reentry_workspace(
        979,
        wasi_shape_rolled_route_seq_arm_program::<0>(),
        wasi_shape_rolled_route_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_DIR_REQ, WASI_FD_PRESTAT_DIR_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 30,
                )
                .await;
                direct_request_response::<WASI_PATH_FILESTAT_REQ, WASI_PATH_FILESTAT_ACK>(
                    controller, worker, 40,
                )
                .await;
                direct_request_response::<WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK>(
                    controller, worker, 50,
                )
                .await;
                direct_request_response::<WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK>(
                    controller, worker, 60,
                )
                .await;
                direct_request_response::<WASI_FD_FILESTAT_REQ, WASI_FD_FILESTAT_ACK>(
                    controller, worker, 70,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 80,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 90,
                )
                .await;
            });
        },
    );
}

#[test]
fn wasi_shape_rolled_route_seq_arm_survives_prior_sibling_arms() {
    with_visible_reentry_workspace(
        980,
        wasi_shape_rolled_route_seq_arm_program::<0>(),
        wasi_shape_rolled_route_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK>(
                    controller, worker, 1,
                )
                .await;
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(controller, worker, 2)
                    .await;
                offer_request_response::<WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK>(
                    controller, worker, 3,
                )
                .await;
                direct_request_response::<WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK>(
                    controller, worker, 4,
                )
                .await;

                offer_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_DIR_REQ, WASI_FD_PRESTAT_DIR_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn wasi_shape_rolled_route_seq_arm_survives_prompt_read_after_open_selector() {
    with_visible_reentry_workspace(
        981,
        wasi_shape_rolled_route_seq_arm_program::<0>(),
        wasi_shape_rolled_route_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK>(
                    controller, worker, 1,
                )
                .await;
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(controller, worker, 2)
                    .await;
                offer_request_response::<WASI_FD_FDSTAT_REQ, WASI_FD_FDSTAT_ACK>(
                    controller, worker, 3,
                )
                .await;
                direct_request_response::<WASI_PATH_OPEN_REQ, WASI_PATH_OPEN_ACK>(
                    controller, worker, 4,
                )
                .await;
                offer_request_response::<WASI_FD_WRITE_REQ, WASI_FD_WRITE_ACK>(
                    controller, worker, 5,
                )
                .await;
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(controller, worker, 6)
                    .await;

                offer_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_DIR_REQ, WASI_FD_PRESTAT_DIR_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_PRESTAT_REQ, WASI_FD_PRESTAT_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_route_left_repeated_label_seq_arm_keeps_continuation() {
    with_visible_reentry_workspace(
        982,
        rolled_left_repeated_label_seq_arm_program::<0>(),
        rolled_left_repeated_label_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_nested_right_route_repeated_label_seq_arm_keeps_continuation() {
    with_visible_reentry_workspace(
        983,
        rolled_nested_right_repeated_label_seq_arm_program::<0>(),
        rolled_nested_right_repeated_label_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_nested_left_route_repeated_label_seq_arm_keeps_continuation() {
    with_visible_reentry_workspace(
        984,
        rolled_nested_left_repeated_label_seq_arm_program::<0>(),
        rolled_nested_left_repeated_label_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}

#[test]
fn rolled_balanced_route_repeated_label_seq_arm_keeps_continuation() {
    with_visible_reentry_workspace(
        985,
        rolled_balanced_repeated_label_seq_arm_program::<0>(),
        rolled_balanced_repeated_label_seq_arm_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                offer_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 10,
                )
                .await;
                direct_request_response::<WASI_FD_READ_REQ, WASI_FD_READ_ACK>(
                    controller, worker, 20,
                )
                .await;
                direct_request_response::<WASI_FD_CLOSE_REQ, WASI_FD_CLOSE_ACK>(
                    controller, worker, 30,
                )
                .await;
            });
        },
    );
}
