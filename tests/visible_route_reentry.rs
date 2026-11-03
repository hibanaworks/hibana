mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::{self, Message, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};

fn nested_input_roll_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::seq(
                g::send::<0, 1, Msg<128, ()>>(),
                g::seq(
                    g::send::<1, 0, Msg<129, ()>>(),
                    g::route(
                        g::seq(
                            g::send::<0, 1, Msg<110, [u8; 7]>>(),
                            g::send::<1, 0, Msg<111, [u8; 7]>>(),
                        ),
                        g::seq(
                            g::send::<0, 1, Msg<130, u16>>(),
                            g::send::<1, 0, Msg<131, u16>>(),
                        ),
                    )
                    .roll(),
                ),
            ),
            g::seq(
                g::send::<0, 1, Msg<134, u16>>(),
                g::send::<1, 0, Msg<135, u16>>(),
            ),
        )
        .roll(),
    )
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_input_roll_accepts_failure_ack_after_retained_samples() {
    for samples in [0, 1, 3] {
        {
            let controller_program = nested_input_roll_program::<0>();
            let worker_program = nested_input_roll_program::<1>();
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .expect("register rendezvous");
            let sid = SessionId::new(1210 + samples);
            let mut source = rv
                .enter(sid, &controller_program)
                .expect("attach controller");
            let mut receiver = rv.enter(sid, &worker_program).expect("attach worker");
            (async {
                source.send::<Msg<128, ()>>(&()).await.unwrap();
                receiver
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<128, ()>>()
                    .await
                    .unwrap();
                receiver.send::<Msg<129, ()>>(&()).await.unwrap();
                source.recv::<Msg<129, ()>>().await.unwrap();
                for index in 0..samples {
                    let frame = [index as u8; 7];
                    source.send::<Msg<110, [u8; 7]>>(&frame).await.unwrap();
                    assert_eq!(
                        receiver
                            .offer()
                            .await
                            .unwrap()
                            .recv::<Msg<110, [u8; 7]>>()
                            .await
                            .unwrap(),
                        frame
                    );
                    receiver.send::<Msg<111, [u8; 7]>>(&frame).await.unwrap();
                    assert_eq!(source.recv::<Msg<111, [u8; 7]>>().await.unwrap(), frame);
                }
                source.send::<Msg<130, u16>>(&1).await.unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<130, u16>>()
                        .await
                        .unwrap(),
                    1
                );
                receiver.send::<Msg<131, u16>>(&1).await.unwrap();
                assert_eq!(
                    source.recv::<Msg<131, u16>>().await.unwrap(),
                    1,
                    "{samples} actual samples before failure"
                );
                // Reenter the enclosing visit after its actual completion.
                // Neither the old connection nor the old failure may grant
                // authority to this new visit's acknowledgement.
                source.send::<Msg<134, u16>>(&2).await.unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<134, u16>>()
                        .await
                        .unwrap(),
                    2
                );
                receiver.send::<Msg<135, u16>>(&2).await.unwrap();
                assert_eq!(source.recv::<Msg<135, u16>>().await.unwrap(), 2);
                source.send::<Msg<128, ()>>(&()).await.unwrap();
                receiver
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<128, ()>>()
                    .await
                    .unwrap();
                receiver.send::<Msg<129, ()>>(&()).await.unwrap();
                source.recv::<Msg<129, ()>>().await.unwrap();
                source.send::<Msg<130, u16>>(&3).await.unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<130, u16>>()
                        .await
                        .unwrap(),
                    3
                );
                receiver.send::<Msg<131, u16>>(&3).await.unwrap();
                assert_eq!(source.recv::<Msg<131, u16>>().await.unwrap(), 3);
            })
            .await
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_input_roll_rejects_switch_before_retention() {
    {
        let controller_program = nested_input_roll_program::<0>();
        let worker_program = nested_input_roll_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1218);
        let mut source = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut receiver = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            source.send::<Msg<128, ()>>(&()).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<128, ()>>()
                .await
                .unwrap();
            receiver.send::<Msg<129, ()>>(&()).await.unwrap();
            source.recv::<Msg<129, ()>>().await.unwrap();
            source.send::<Msg<110, [u8; 7]>>(&[1; 7]).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<110, [u8; 7]>>()
                .await
                .unwrap();
            assert!(source.send::<Msg<130, u16>>(&1).await.is_err());
        })
        .await
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_input_roll_rejects_duplicate_failure_ack() {
    {
        let controller_program = nested_input_roll_program::<0>();
        let worker_program = nested_input_roll_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1219);
        let mut source = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut receiver = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            source.send::<Msg<128, ()>>(&()).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<128, ()>>()
                .await
                .unwrap();
            receiver.send::<Msg<129, ()>>(&()).await.unwrap();
            source.recv::<Msg<129, ()>>().await.unwrap();
            source.send::<Msg<130, u16>>(&1).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<130, u16>>()
                .await
                .unwrap();
            receiver.send::<Msg<131, u16>>(&1).await.unwrap();
            source.recv::<Msg<131, u16>>().await.unwrap();
            assert!(receiver.send::<Msg<131, u16>>(&1).await.is_err());
        })
        .await
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_input_roll_preserves_prefix_when_parallel_right_lane_reenters_first() {
    fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
        project(
            &g::route(
                g::seq(
                    g::send::<0, 1, Msg<128, ()>>(),
                    g::seq(
                        g::send::<1, 0, Msg<129, ()>>(),
                        g::route(
                            g::par(
                                g::seq(
                                    g::send::<0, 1, Msg<110, u16>>(),
                                    g::send::<1, 0, Msg<111, u16>>(),
                                ),
                                g::seq(
                                    g::send::<0, 1, Msg<112, u16>>(),
                                    g::send::<1, 0, Msg<113, u16>>(),
                                ),
                            ),
                            g::seq(
                                g::send::<0, 1, Msg<130, u16>>(),
                                g::send::<1, 0, Msg<131, u16>>(),
                            ),
                        )
                        .roll(),
                    ),
                ),
                g::seq(
                    g::send::<0, 1, Msg<134, u16>>(),
                    g::send::<1, 0, Msg<135, u16>>(),
                ),
            )
            .roll(),
        )
    }
    {
        let controller_program = program::<0>();
        let worker_program = program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1220);
        let mut source = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut receiver = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            source.send::<Msg<128, ()>>(&()).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<128, ()>>()
                .await
                .unwrap();
            receiver.send::<Msg<129, ()>>(&()).await.unwrap();
            source.recv::<Msg<129, ()>>().await.unwrap();
            source.send::<Msg<110, u16>>(&1).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<110, u16>>()
                .await
                .unwrap();
            receiver.send::<Msg<111, u16>>(&1).await.unwrap();
            source.recv::<Msg<111, u16>>().await.unwrap();
            source.send::<Msg<112, u16>>(&1).await.unwrap();
            receiver.recv::<Msg<112, u16>>().await.unwrap();
            receiver.send::<Msg<113, u16>>(&1).await.unwrap();
            source.recv::<Msg<113, u16>>().await.unwrap();
            source.send::<Msg<112, u16>>(&2).await.unwrap();
            receiver.recv::<Msg<112, u16>>().await.unwrap();
            receiver.send::<Msg<113, u16>>(&2).await.unwrap();
            source.recv::<Msg<113, u16>>().await.unwrap();
            source.send::<Msg<110, u16>>(&2).await.unwrap();
            receiver.recv::<Msg<110, u16>>().await.unwrap();
            receiver.send::<Msg<111, u16>>(&2).await.unwrap();
            source.recv::<Msg<111, u16>>().await.unwrap();
            source.send::<Msg<130, u16>>(&3).await.unwrap();
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<130, u16>>()
                .await
                .unwrap();
            receiver.send::<Msg<131, u16>>(&3).await.unwrap();
            source.recv::<Msg<131, u16>>().await.unwrap();
        })
        .await
    };
}

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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_seq_reenters_by_repeated_head_without_loop_control() {
    {
        let controller_program = visible_reentry_program::<0>();
        let worker_program = visible_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(960);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<TOP_BODY_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {TOP_BODY_REQ}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<TOP_BODY_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {TOP_BODY_REQ}: {err:?}"))
                },
                10
            );
            {
                let value: u8 = 11;
                worker
                    .send::<Msg<TOP_BODY_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {TOP_BODY_ACK}: {err:?}"));
            };
            assert_eq!(
                controller
                    .recv::<Msg<TOP_BODY_ACK, u8>>()
                    .await
                    .expect("controller recv first ack"),
                11
            );

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<TOP_BODY_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {TOP_BODY_REQ}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<TOP_BODY_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {TOP_BODY_REQ}: {err:?}"))
                },
                20
            );
            {
                let value: u8 = 21;
                worker
                    .send::<Msg<TOP_BODY_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {TOP_BODY_ACK}: {err:?}"));
            };
            assert_eq!(
                controller
                    .recv::<Msg<TOP_BODY_ACK, u8>>()
                    .await
                    .expect("controller recv second ack"),
                21
            );

            {
                let value: u8 = 99;
                controller
                    .send::<Msg<TOP_EXIT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {TOP_EXIT}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<TOP_EXIT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {TOP_EXIT}: {err:?}"))
                },
                99
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_par_reenters_only_after_both_lanes_settle() {
    {
        let controller_program = visible_parallel_reentry_program::<0>();
        let worker_program = visible_parallel_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1962);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {let value:u8=10;
    controller
        .send::<Msg<PAR_LEFT, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<PAR_LEFT, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
}, 10);
                {
    let err = match controller.send::<Msg<PAR_LEFT, u8>>(&0).await {
        Ok(()) => panic!("controller send label {PAR_LEFT} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {PAR_LEFT} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = visible_parallel_reentry_program::<0>();
        let worker_program = visible_parallel_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(2962);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {let value:u8=10;
    controller
        .send::<Msg<PAR_LEFT, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<PAR_LEFT, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
}, 10);
                {
    let err = match controller.send::<Msg<PAR_EXIT, u8>>(&0).await {
        Ok(()) => panic!("controller send label {PAR_EXIT} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {PAR_EXIT} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = visible_parallel_reentry_program::<0>();
        let worker_program = visible_parallel_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(3962);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {let value:u8=10;
    controller
        .send::<Msg<PAR_LEFT, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
};
                {let value:u8=11;
    controller
        .send::<Msg<PAR_RIGHT, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_RIGHT}: {err:?}"));
};
                {let value:u8=20;
    controller
        .send::<Msg<PAR_LEFT, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<PAR_LEFT, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
}, 10);
                assert_eq!({
    worker
        .recv::<Msg<PAR_RIGHT, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {PAR_RIGHT}: {err:?}"))
}, 11);
                assert_eq!({
    worker
        .recv::<Msg<PAR_LEFT, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
}, 20);
                {
    let err = match controller.send::<Msg<PAR_EXIT, u8>>(&0).await {
        Ok(()) => panic!("controller send label {PAR_EXIT} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {PAR_EXIT} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = visible_parallel_reentry_program::<0>();
        let worker_program = visible_parallel_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(962);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<PAR_LEFT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
            };
            {
                let value: u8 = 11;
                controller
                    .send::<Msg<PAR_RIGHT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {PAR_RIGHT}: {err:?}"));
            };
            {
                let value: u8 = 20;
                controller
                    .send::<Msg<PAR_LEFT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {PAR_LEFT}: {err:?}"));
            };
            {
                let value: u8 = 21;
                controller
                    .send::<Msg<PAR_RIGHT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {PAR_RIGHT}: {err:?}"));
            };
            {
                let value: u8 = 99;
                controller
                    .send::<Msg<PAR_EXIT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {PAR_EXIT}: {err:?}"));
            };

            assert_eq!(
                {
                    worker
                        .recv::<Msg<PAR_LEFT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
                },
                10
            );
            assert_eq!(
                {
                    worker
                        .recv::<Msg<PAR_RIGHT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {PAR_RIGHT}: {err:?}"))
                },
                11
            );
            assert_eq!(
                {
                    worker
                        .recv::<Msg<PAR_LEFT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {PAR_LEFT}: {err:?}"))
                },
                20
            );
            assert_eq!(
                {
                    worker
                        .recv::<Msg<PAR_RIGHT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {PAR_RIGHT}: {err:?}"))
                },
                21
            );
            assert_eq!(
                {
                    worker
                        .recv::<Msg<PAR_EXIT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {PAR_EXIT}: {err:?}"))
                },
                99
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_rolled_route_reenters_before_outer_body_continues() {
    {
        let controller_program = nested_visible_reentry_program::<0>();
        let worker_program = nested_visible_reentry_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(961);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 1;
                controller
                    .send::<Msg<OUTER_OPEN, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {OUTER_OPEN}: {err:?}"));
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {OUTER_OPEN}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<OUTER_OPEN, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<OUTER_OPEN, u8>>()
                        .await
                        .expect("worker recv")
                },
                1
            );

            {
                let value: u8 = 2;
                controller
                    .send::<Msg<INNER_BODY, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {INNER_BODY}: {err:?}"));
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {INNER_BODY}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<INNER_BODY, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<INNER_BODY, u8>>()
                        .await
                        .expect("worker recv")
                },
                2
            );

            {
                let value: u8 = 3;
                controller
                    .send::<Msg<INNER_BODY, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {INNER_BODY}: {err:?}"));
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {INNER_BODY}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<INNER_BODY, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<INNER_BODY, u8>>()
                        .await
                        .expect("worker recv")
                },
                3
            );

            {
                let value: u8 = 4;
                controller
                    .send::<Msg<INNER_EXIT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {INNER_EXIT}: {err:?}"));
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {INNER_EXIT}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<INNER_EXIT, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<INNER_EXIT, u8>>()
                        .await
                        .expect("worker recv")
                },
                4
            );

            {
                let value: u8 = 5;
                worker
                    .send::<Msg<OUTER_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {OUTER_ACK}: {err:?}"));
            };
            assert_eq!(
                controller
                    .recv::<Msg<OUTER_ACK, u8>>()
                    .await
                    .expect("controller recv outer ack"),
                5
            );

            {
                let value: u8 = 6;
                controller
                    .send::<Msg<OUTER_EXIT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {OUTER_EXIT}: {err:?}"));
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {OUTER_EXIT}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<OUTER_EXIT, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<OUTER_EXIT, u8>>()
                        .await
                        .expect("worker recv")
                },
                6
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_roll_scopes_reenter_inner_until_outer_scope_completes() {
    {
        let controller_program = nested_seq_roll_program::<0>();
        let worker_program = nested_seq_roll_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1963);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {let value:u8=10;
    controller
        .send::<Msg<NESTED_BODY, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<NESTED_BODY, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
}, 10);
                {let value:u8=11;
    worker
        .send::<Msg<NESTED_OTHER, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
};
                assert_eq!({
    controller
        .recv::<Msg<NESTED_OTHER, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {NESTED_OTHER}: {err:?}"))
}, 11);
                {let value:u8=20;
    controller
        .send::<Msg<NESTED_BODY, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<NESTED_BODY, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
}, 20);
                {let value:u8=21;
    worker
        .send::<Msg<NESTED_OTHER, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
};
                assert_eq!({
    controller
        .recv::<Msg<NESTED_OTHER, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {NESTED_OTHER}: {err:?}"))
}, 21);
                {let value:u8=30;
    controller
        .send::<Msg<NESTED_TAIL, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {NESTED_TAIL}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<NESTED_TAIL, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {NESTED_TAIL}: {err:?}"))
}, 30);
                {let value:u8=40;
    controller
        .send::<Msg<NESTED_BODY, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
};
                assert_eq!({
    worker
        .recv::<Msg<NESTED_BODY, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
}, 40);
                {let value:u8=41;
    worker
        .send::<Msg<NESTED_OTHER, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
};
                assert_eq!({
    controller
        .recv::<Msg<NESTED_OTHER, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {NESTED_OTHER}: {err:?}"))
}, 41);
                {
    let err = match controller.send::<Msg<NESTED_EXIT, u8>>(&0).await {
        Ok(()) => panic!("controller send label {NESTED_EXIT} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {NESTED_EXIT} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = nested_seq_roll_program::<0>();
        let worker_program = nested_seq_roll_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(963);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<NESTED_BODY, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_BODY, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
                },
                10
            );
            {
                let value: u8 = 11;
                worker
                    .send::<Msg<NESTED_OTHER, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_OTHER, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_OTHER}: {err:?}")
                        })
                },
                11
            );

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<NESTED_BODY, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_BODY, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
                },
                20
            );
            {
                let value: u8 = 21;
                worker
                    .send::<Msg<NESTED_OTHER, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_OTHER, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_OTHER}: {err:?}")
                        })
                },
                21
            );

            {
                let value: u8 = 30;
                controller
                    .send::<Msg<NESTED_TAIL, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_TAIL}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_TAIL, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_TAIL}: {err:?}"))
                },
                30
            );

            {
                let value: u8 = 40;
                controller
                    .send::<Msg<NESTED_BODY, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_BODY}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_BODY, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_BODY}: {err:?}"))
                },
                40
            );
            {
                let value: u8 = 41;
                worker
                    .send::<Msg<NESTED_OTHER, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {NESTED_OTHER}: {err:?}"));
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_OTHER, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_OTHER}: {err:?}")
                        })
                },
                41
            );
            {
                let value: u8 = 50;
                controller
                    .send::<Msg<NESTED_TAIL, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_TAIL}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_TAIL, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_TAIL}: {err:?}"))
                },
                50
            );
            {
                let value: u8 = 60;
                controller
                    .send::<Msg<NESTED_EXIT, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("controller send label {NESTED_EXIT}: {err:?}"));
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<NESTED_EXIT, u8>>()
                        .await
                        .unwrap_or_else(|err| panic!("worker recv label {NESTED_EXIT}: {err:?}"))
                },
                60
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_route_reenters_to_sibling_nested_arm() {
    {
        let controller_program = rolled_nested_route_program::<0>();
        let worker_program = rolled_nested_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(964);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<NESTED_ROUTE_B_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {NESTED_ROUTE_B_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {NESTED_ROUTE_B_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<NESTED_ROUTE_B_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<NESTED_ROUTE_B_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                10
            );
            {
                let value: u8 = 11;
                worker
                    .send::<Msg<NESTED_ROUTE_B_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("worker send label {NESTED_ROUTE_B_ACK}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_ROUTE_B_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_ROUTE_B_ACK}: {err:?}")
                        })
                },
                11
            );

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<NESTED_ROUTE_C_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {NESTED_ROUTE_C_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {NESTED_ROUTE_C_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<NESTED_ROUTE_C_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<NESTED_ROUTE_C_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                20
            );
            {
                let value: u8 = 21;
                worker
                    .send::<Msg<NESTED_ROUTE_C_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("worker send label {NESTED_ROUTE_C_ACK}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_ROUTE_C_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_ROUTE_C_ACK}: {err:?}")
                        })
                },
                21
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_reenters_from_left_arm_to_nested_right_arm() {
    {
        let controller_program = rolled_nested_route_program::<0>();
        let worker_program = rolled_nested_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(965);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<NESTED_ROUTE_A_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {NESTED_ROUTE_A_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {NESTED_ROUTE_A_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<NESTED_ROUTE_A_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<NESTED_ROUTE_A_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                10
            );
            {
                let value: u8 = 11;
                worker
                    .send::<Msg<NESTED_ROUTE_A_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("worker send label {NESTED_ROUTE_A_ACK}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_ROUTE_A_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_ROUTE_A_ACK}: {err:?}")
                        })
                },
                11
            );

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<NESTED_ROUTE_B_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {NESTED_ROUTE_B_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {NESTED_ROUTE_B_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<NESTED_ROUTE_B_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<NESTED_ROUTE_B_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                20
            );
            {
                let value: u8 = 21;
                worker
                    .send::<Msg<NESTED_ROUTE_B_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("worker send label {NESTED_ROUTE_B_ACK}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<NESTED_ROUTE_B_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {NESTED_ROUTE_B_ACK}: {err:?}")
                        })
                },
                21
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_reenters_from_completed_outer_arm_to_deep_nested_arm() {
    {
        let controller_program = rolled_deep_nested_route_program::<0>();
        let worker_program = rolled_deep_nested_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(966);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<DEEP_ROUTE_A_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {DEEP_ROUTE_A_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {DEEP_ROUTE_A_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_ROUTE_A_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<DEEP_ROUTE_A_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                10
            );
            {
                let value: u8 = 11;
                worker
                    .send::<Msg<DEEP_ROUTE_A_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {DEEP_ROUTE_A_ACK}: {err:?}"));
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<DEEP_ROUTE_A_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {DEEP_ROUTE_A_ACK}: {err:?}")
                        })
                },
                11
            );

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<DEEP_ROUTE_B_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {DEEP_ROUTE_B_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {DEEP_ROUTE_B_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_ROUTE_B_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<DEEP_ROUTE_B_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                20
            );
            {
                let value: u8 = 21;
                worker
                    .send::<Msg<DEEP_ROUTE_B_ACK, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| panic!("worker send label {DEEP_ROUTE_B_ACK}: {err:?}"));
            };
            assert_eq!(
                {
                    controller
                        .recv::<Msg<DEEP_ROUTE_B_ACK, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller recv label {DEEP_ROUTE_B_ACK}: {err:?}")
                        })
                },
                21
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_right_spine_reenters_repeated_deep_rightmost_arm() {
    {
        let controller_program = rolled_right_spine_route_program::<0>();
        let worker_program = rolled_right_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(967);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_A_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_B_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_B_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_B_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_C_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_C_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_C_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_C_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_C_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_C_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_C_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_C_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_D_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_D_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_D_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_D_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_D_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_D_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_D_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_D_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_E_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_E_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_E_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_E_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_E_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_E_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_E_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_E_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_H_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 70;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_H_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_right_spine_rejects_deep_response_without_reentered_request() {
    {
        let controller_program = rolled_right_spine_route_program::<0>();
        let worker_program = rolled_right_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(970);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<SPINE_A_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {SPINE_A_REQ}: {err:?}"));
};
    assert_eq!({
    worker
        .recv::<Msg<SPINE_A_REQ, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {SPINE_A_REQ}: {err:?}"))
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<SPINE_A_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<SPINE_A_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {SPINE_A_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                { let value:u8=20;
    {let value:u8=value;
    controller
        .send::<Msg<SPINE_H_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {SPINE_H_REQ}: {err:?}"));
};
    assert_eq!({
    worker
        .recv::<Msg<SPINE_H_REQ, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {SPINE_H_REQ}: {err:?}"))
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<SPINE_H_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<SPINE_H_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {SPINE_H_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                {
    let err = match worker.send::<Msg<SPINE_H_ACK, u8>>(&0).await {
        Ok(()) => panic!("worker send label {SPINE_H_ACK} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "worker send label {SPINE_H_ACK} must remain blocked by roll progress: {rendered}"
    );
};
            }).await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_right_spine_offer_reenters_through_left_chain() {
    {
        let controller_program = rolled_right_spine_route_program::<0>();
        let worker_program = rolled_right_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(968);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_B_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_C_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_C_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_C_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_C_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_C_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_C_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_C_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_C_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_C_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_D_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_D_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_D_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_D_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_D_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_D_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_D_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_D_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_D_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_E_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_E_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_E_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_E_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_E_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_E_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_E_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_E_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_E_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_right_spine_offer_reenters_repeated_deep_rightmost_arm() {
    {
        let controller_program = rolled_right_spine_route_program::<0>();
        let worker_program = rolled_right_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(971);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_B_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_C_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_C_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_C_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_C_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_C_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_C_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_C_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_C_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_C_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_D_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_D_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_D_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_D_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_D_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_D_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_D_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_D_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_D_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_E_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_E_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_E_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_E_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_E_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_E_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_E_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_E_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_E_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 70;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_spine_reenters_repeated_deep_leftmost_arm() {
    {
        let controller_program = rolled_left_spine_route_program::<0>();
        let worker_program = rolled_left_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(969);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_H_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_G_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_G_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_G_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_G_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_G_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_G_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_G_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_G_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_A_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SPINE_A_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_spine_offer_enters_rightmost_arm() {
    {
        let controller_program = rolled_left_spine_route_program::<0>();
        let worker_program = rolled_left_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(973);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_spine_offer_reenters_inner_right_arm() {
    {
        let controller_program = rolled_left_spine_route_program::<0>();
        let worker_program = rolled_left_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(974);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_G_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_G_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_G_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_G_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_G_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_G_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_G_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_G_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_G_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_spine_offer_reenters_deep_left_arm_once() {
    {
        let controller_program = rolled_left_spine_route_program::<0>();
        let worker_program = rolled_left_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(975);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_G_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_G_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_G_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_G_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_G_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_G_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_G_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_G_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_G_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_spine_offer_reenters_repeated_deep_leftmost_arm() {
    {
        let controller_program = rolled_left_spine_route_program::<0>();
        let worker_program = rolled_left_spine_route_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(972);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_H_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_H_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_H_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_H_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_H_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_H_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_H_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_H_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_H_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_G_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_G_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_G_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_G_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_G_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_G_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_G_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_G_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_G_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SPINE_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SPINE_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SPINE_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SPINE_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SPINE_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SPINE_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SPINE_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SPINE_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SPINE_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_right_seq_arm_keeps_send_authority_for_continuation() {
    {
        let controller_program = rolled_right_seq_arm_program::<0>();
        let worker_program = rolled_right_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(976);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ARM_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ARM_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ARM_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_B_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_C_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_C_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ARM_C_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ARM_C_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_C_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_C_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_C_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_C_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_left_seq_arm_keeps_send_authority_for_continuation() {
    {
        let controller_program = rolled_left_seq_arm_program::<0>();
        let worker_program = rolled_left_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(977);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ARM_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ARM_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ARM_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_A_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ARM_B_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ARM_B_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_B_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_right_route_seq_arm_keeps_send_authority_for_continuation() {
    {
        let controller_program = rolled_nested_right_seq_arm_program::<0>();
        let worker_program = rolled_nested_right_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(978);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_C_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_C_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ARM_C_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ARM_C_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ARM_C_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_C_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_C_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_C_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_C_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ARM_D_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ARM_D_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ARM_D_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ARM_D_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ARM_D_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| panic!("worker send label {SEQ_ARM_D_ACK}: {err:?}"));
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ARM_D_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ARM_D_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_left_seq_roll_arm_reenters_inner_before_tail() {
    {
        let controller_program = rolled_route_left_seq_roll_arm_program::<0>();
        let worker_program = rolled_route_left_seq_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(986);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<ROUTE_SEQ_ROLL_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {ROUTE_SEQ_ROLL_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_OTHER_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_OTHER_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_OTHER_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_OTHER_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_OTHER_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_OTHER_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_right_seq_roll_arm_reenters_inner_before_tail() {
    {
        let controller_program = rolled_route_right_seq_roll_arm_program::<0>();
        let worker_program = rolled_route_right_seq_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(987);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_OTHER_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_OTHER_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_OTHER_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_OTHER_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_OTHER_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_OTHER_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_OTHER_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_SEQ_ROLL_BODY_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_BODY_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_BODY_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_SEQ_ROLL_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_SEQ_ROLL_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<ROUTE_SEQ_ROLL_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {ROUTE_SEQ_ROLL_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_SEQ_ROLL_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_SEQ_ROLL_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_SEQ_ROLL_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_SEQ_ROLL_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_left_par_roll_arm_reenters_only_after_parallel_settles() {
    {
        let controller_program = rolled_route_left_par_roll_arm_program::<0>();
        let worker_program = rolled_route_left_par_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1988);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {let value:u8=10;
    controller
        .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}"));
};
                assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>().await.expect("worker recv")
}, 10);
                {
    let err = match controller.send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {ROUTE_PAR_ROLL_OTHER_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = rolled_route_left_par_roll_arm_program::<0>();
        let worker_program = rolled_route_left_par_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(988);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                10
            );
            {
                let value: u8 = 20;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker recv label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                        })
                },
                20
            );

            {
                let value: u8 = 30;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                30
            );
            {
                let value: u8 = 40;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker recv label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                        })
                },
                40
            );

            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_left_par_roll_arm_exits_after_parallel_settles_without_probe() {
    {
        let controller_program = rolled_route_left_par_roll_arm_program::<0>();
        let worker_program = rolled_route_left_par_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(993);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                10
            );
            {
                let value: u8 = 20;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker recv label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                        })
                },
                20
            );

            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_right_par_roll_arm_reenters_only_after_parallel_settles() {
    {
        let controller_program = rolled_route_right_par_roll_arm_program::<0>();
        let worker_program = rolled_route_right_par_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1989);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                {let value:u8=20;
    controller
        .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}"));
};
                assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>().await.expect("worker recv")
}, 20);
                {
    let err = match controller.send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {ROUTE_PAR_ROLL_OTHER_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = rolled_route_right_par_roll_arm_program::<0>();
        let worker_program = rolled_route_right_par_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(989);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {ROUTE_PAR_ROLL_OTHER_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<ROUTE_PAR_ROLL_OTHER_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {ROUTE_PAR_ROLL_OTHER_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 20;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                20
            );
            {
                let value: u8 = 30;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker recv label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                        })
                },
                30
            );
            {
                let value: u8 = 40;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    let branch = worker.offer().await.unwrap_or_else(|err| {
                        panic!("worker offer for label {ROUTE_PAR_ROLL_RIGHT_REQ}: {err:?}")
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8> as Message>::LOGICAL_LABEL
                    );
                    branch
                        .recv::<Msg<ROUTE_PAR_ROLL_RIGHT_REQ, u8>>()
                        .await
                        .expect("worker recv")
                },
                40
            );
            {
                let value: u8 = 50;
                controller
                    .send::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>(&value)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("controller send label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                    });
            };
            assert_eq!(
                {
                    worker
                        .recv::<Msg<ROUTE_PAR_ROLL_LEFT_REQ, u8>>()
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker recv label {ROUTE_PAR_ROLL_LEFT_REQ}: {err:?}")
                        })
                },
                50
            );
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_par_left_route_roll_arm_reenters_before_parallel_sibling_settles() {
    {
        let controller_program = rolled_par_left_route_roll_arm_program::<0>();
        let worker_program = rolled_par_left_route_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1990);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_ROUTE_ROLL_A_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {PAR_ROUTE_ROLL_A_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<PAR_ROUTE_ROLL_A_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {PAR_ROUTE_ROLL_A_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {PAR_ROUTE_ROLL_A_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                { let value:u8=20;
    {let value:u8=value;
    controller
        .send::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {PAR_ROUTE_ROLL_B_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {PAR_ROUTE_ROLL_B_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<PAR_ROUTE_ROLL_B_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {PAR_ROUTE_ROLL_B_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {PAR_ROUTE_ROLL_B_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                {
    let err = match controller.send::<Msg<PAR_ROUTE_ROLL_EXIT_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {PAR_ROUTE_ROLL_EXIT_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {PAR_ROUTE_ROLL_EXIT_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = rolled_par_left_route_roll_arm_program::<0>();
        let worker_program = rolled_par_left_route_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(990);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {PAR_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<PAR_ROUTE_ROLL_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_A_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {PAR_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<PAR_ROUTE_ROLL_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_B_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_SIBLING_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_SIBLING_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<PAR_ROUTE_ROLL_SIBLING_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {PAR_ROUTE_ROLL_SIBLING_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_SIBLING_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_SIBLING_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_SIBLING_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!(
                                    "controller recv label {PAR_ROUTE_ROLL_SIBLING_ACK}: {err:?}"
                                )
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_EXIT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<PAR_ROUTE_ROLL_EXIT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {PAR_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_EXIT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_EXIT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_par_right_route_roll_arm_reenters_before_parallel_sibling_settles() {
    {
        let controller_program = rolled_par_right_route_roll_arm_program::<0>();
        let worker_program = rolled_par_right_route_roll_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(991);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_SIBLING_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_SIBLING_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<PAR_ROUTE_ROLL_SIBLING_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {PAR_ROUTE_ROLL_SIBLING_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_SIBLING_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_SIBLING_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_SIBLING_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!(
                                    "controller recv label {PAR_ROUTE_ROLL_SIBLING_ACK}: {err:?}"
                                )
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {PAR_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<PAR_ROUTE_ROLL_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<PAR_ROUTE_ROLL_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_B_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {PAR_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<PAR_ROUTE_ROLL_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<PAR_ROUTE_ROLL_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_A_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<PAR_ROUTE_ROLL_EXIT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {PAR_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<PAR_ROUTE_ROLL_EXIT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {PAR_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<PAR_ROUTE_ROLL_EXIT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {PAR_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<PAR_ROUTE_ROLL_EXIT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {PAR_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_seq_route_roll_head_reenters_before_seq_tail_and_outer_exit() {
    {
        let controller_program = rolled_seq_route_roll_head_program::<0>();
        let worker_program = rolled_seq_route_roll_head_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(992);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ROUTE_ROLL_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ROUTE_ROLL_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_A_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ROUTE_ROLL_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ROUTE_ROLL_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ROUTE_ROLL_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_B_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ROUTE_ROLL_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ROUTE_ROLL_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_A_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {SEQ_ROUTE_ROLL_A_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<SEQ_ROUTE_ROLL_A_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<SEQ_ROUTE_ROLL_A_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_A_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_A_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_A_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_A_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ROUTE_ROLL_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ROUTE_ROLL_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<SEQ_ROUTE_ROLL_EXIT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {SEQ_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<SEQ_ROUTE_ROLL_EXIT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {SEQ_ROUTE_ROLL_EXIT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<SEQ_ROUTE_ROLL_EXIT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {SEQ_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<SEQ_ROUTE_ROLL_EXIT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {SEQ_ROUTE_ROLL_EXIT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_route_par_seq_route_roll_mixed_reentry_keeps_single_authority() {
    {
        let controller_program = rolled_route_route_par_seq_route_roll_mixed_program::<0>();
        let worker_program = rolled_route_route_par_seq_route_roll_mixed_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1994);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {
    { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=20;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_INNER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_INNER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_INNER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_INNER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_INNER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_INNER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_INNER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_INNER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_INNER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=30;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_A_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_A_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_A_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_A_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_A_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_A_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_A_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_A_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_A_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=40;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_B_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
};
                {
    let err = match controller.send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {MIX_OUTER_LEFT_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {MIX_OUTER_LEFT_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = rolled_route_route_par_seq_route_roll_mixed_program::<0>();
        let worker_program = rolled_route_route_par_seq_route_roll_mixed_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(2994);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {
    { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=20;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_INNER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_INNER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_INNER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_INNER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_INNER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_INNER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_INNER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_INNER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_INNER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=30;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_A_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_A_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_A_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_A_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_A_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_A_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_A_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_A_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_A_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=40;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_B_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
};
                { let value:u8=50;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_SEQ_TAIL_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_SEQ_TAIL_REQ}: {err:?}"));
};
    assert_eq!({
    worker
        .recv::<Msg<MIX_SEQ_TAIL_REQ, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {MIX_SEQ_TAIL_REQ}: {err:?}"))
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_SEQ_TAIL_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_SEQ_TAIL_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_SEQ_TAIL_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_SEQ_TAIL_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
                {
    let err = match controller.send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {MIX_OUTER_LEFT_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {MIX_OUTER_LEFT_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program = rolled_route_route_par_seq_route_roll_mixed_program::<0>();
        let worker_program = rolled_route_route_par_seq_route_roll_mixed_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(994);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                {
                    let value: u8 = 10;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 20;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_INNER_LEFT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_INNER_LEFT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_INNER_LEFT_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_INNER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_INNER_LEFT_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_INNER_LEFT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_INNER_LEFT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_INNER_LEFT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_INNER_LEFT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 30;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_DEEP_A_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_DEEP_A_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_DEEP_A_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_DEEP_A_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_DEEP_A_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_DEEP_A_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_DEEP_A_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_DEEP_A_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_DEEP_A_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 40;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_DEEP_B_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_SEQ_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_SEQ_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_SEQ_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_SEQ_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_SEQ_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_SEQ_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_SEQ_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_SEQ_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_PAR_SIBLING_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_PAR_SIBLING_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_PAR_SIBLING_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_PAR_SIBLING_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_PAR_SIBLING_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_PAR_SIBLING_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_PAR_SIBLING_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_PAR_SIBLING_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 70;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<MIX_DEEP_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 80;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_SEQ_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_SEQ_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_SEQ_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_SEQ_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_SEQ_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_SEQ_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_SEQ_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_SEQ_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 90;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_PAR_SIBLING_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_PAR_SIBLING_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_PAR_SIBLING_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_PAR_SIBLING_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_PAR_SIBLING_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_PAR_SIBLING_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_PAR_SIBLING_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_PAR_SIBLING_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 100;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_left_route_left_route_par_seq_route_roll_mixed_reentry_keeps_single_authority() {
    {
        let controller_program =
            rolled_left_route_left_route_par_seq_route_roll_mixed_program::<0>();
        let worker_program = rolled_left_route_left_route_par_seq_route_roll_mixed_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(1995);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
                {
    { let value:u8=10;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=20;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_INNER_LEFT_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_INNER_LEFT_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_INNER_LEFT_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_INNER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_INNER_LEFT_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_INNER_LEFT_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_INNER_LEFT_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_INNER_LEFT_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_INNER_LEFT_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=30;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_PAR_SIBLING_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_PAR_SIBLING_REQ}: {err:?}"));
};
    assert_eq!({
    worker
        .recv::<Msg<MIX_PAR_SIBLING_REQ, u8>>()
        .await
        .unwrap_or_else(|err| panic!("worker recv label {MIX_PAR_SIBLING_REQ}: {err:?}"))
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_PAR_SIBLING_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_PAR_SIBLING_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_PAR_SIBLING_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_PAR_SIBLING_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=40;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_A_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_A_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_A_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_A_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_A_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_A_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_A_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_A_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_A_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
    { let value:u8=50;
    {let value:u8=value;
    controller
        .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}"));
};
    assert_eq!({
    let branch = worker
        .offer()
        .await
        .unwrap_or_else(|err| panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL);
    branch.recv::<Msg<MIX_DEEP_B_REQ, u8>>().await.expect("worker recv")
}, value);
    {let value:u8=value.wrapping_add(1);
    worker
        .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
        .await
        .unwrap_or_else(|err| panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}"));
};
    assert_eq!(
        {
    controller
        .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
        .await
        .unwrap_or_else(|err| panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}"))
},
        value.wrapping_add(1)
    );
};
};
                {
    let err = match controller.send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&0).await {
        Ok(()) => panic!("controller send label {MIX_OUTER_LEFT_REQ} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "controller send label {MIX_OUTER_LEFT_REQ} must remain blocked by roll/par progress: {rendered}"
    );
};
            }).await;
    };
    {
        let controller_program =
            rolled_left_route_left_route_par_seq_route_roll_mixed_program::<0>();
        let worker_program = rolled_left_route_left_route_par_seq_route_roll_mixed_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(995);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                {
                    let value: u8 = 10;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 20;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_INNER_LEFT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_INNER_LEFT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_INNER_LEFT_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_INNER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_INNER_LEFT_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_INNER_LEFT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_INNER_LEFT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_INNER_LEFT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_INNER_LEFT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 30;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_PAR_SIBLING_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_PAR_SIBLING_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<MIX_PAR_SIBLING_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {MIX_PAR_SIBLING_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_PAR_SIBLING_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_PAR_SIBLING_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_PAR_SIBLING_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_PAR_SIBLING_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 40;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_DEEP_A_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_DEEP_A_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_DEEP_A_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_DEEP_A_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_DEEP_A_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_DEEP_A_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_DEEP_A_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_DEEP_A_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_DEEP_A_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 50;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<MIX_DEEP_B_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_SEQ_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_SEQ_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_SEQ_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_SEQ_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_SEQ_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_SEQ_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_SEQ_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_SEQ_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 70;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_DEEP_B_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_DEEP_B_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {MIX_DEEP_B_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<MIX_DEEP_B_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<MIX_DEEP_B_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_DEEP_B_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_DEEP_B_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_DEEP_B_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_DEEP_B_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 80;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_SEQ_TAIL_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_SEQ_TAIL_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_SEQ_TAIL_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_SEQ_TAIL_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_SEQ_TAIL_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_SEQ_TAIL_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_SEQ_TAIL_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_SEQ_TAIL_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 90;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_PAR_SIBLING_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_PAR_SIBLING_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<MIX_PAR_SIBLING_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {MIX_PAR_SIBLING_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_PAR_SIBLING_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_PAR_SIBLING_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_PAR_SIBLING_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_PAR_SIBLING_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 100;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<MIX_OUTER_LEFT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {MIX_OUTER_LEFT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {MIX_OUTER_LEFT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<MIX_OUTER_LEFT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<MIX_OUTER_LEFT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<MIX_OUTER_LEFT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {MIX_OUTER_LEFT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<MIX_OUTER_LEFT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {MIX_OUTER_LEFT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn wasi_shape_rolled_route_seq_arm_keeps_send_authority_for_continuation() {
    {
        let controller_program = wasi_shape_rolled_route_seq_arm_program::<0>();
        let worker_program = wasi_shape_rolled_route_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(979);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_PRESTAT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 40;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_PATH_FILESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_PATH_FILESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_PATH_FILESTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_PATH_FILESTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_PATH_FILESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_PATH_FILESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_PATH_FILESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_PATH_FILESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 50;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_FDSTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_FDSTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_FDSTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_FDSTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_FDSTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_FDSTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_FDSTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_FDSTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 60;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_PATH_OPEN_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_PATH_OPEN_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_PATH_OPEN_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_PATH_OPEN_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_PATH_OPEN_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_PATH_OPEN_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_PATH_OPEN_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_PATH_OPEN_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 70;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_FILESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_FILESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_FILESTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_FILESTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_FILESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_FILESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_FILESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_FILESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 80;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 90;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn wasi_shape_rolled_route_seq_arm_survives_prior_sibling_arms() {
    {
        let controller_program = wasi_shape_rolled_route_seq_arm_program::<0>();
        let worker_program = wasi_shape_rolled_route_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(980);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 1;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_WRITE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_WRITE_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_WRITE_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_WRITE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_WRITE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_WRITE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_WRITE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 2;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 3;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_FDSTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_FDSTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_FDSTAT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_FDSTAT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_FDSTAT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_FDSTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_FDSTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_FDSTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_FDSTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 4;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_PATH_OPEN_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_PATH_OPEN_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_PATH_OPEN_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_PATH_OPEN_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_PATH_OPEN_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_PATH_OPEN_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_PATH_OPEN_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_PATH_OPEN_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_PRESTAT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn wasi_shape_rolled_route_seq_arm_survives_prompt_read_after_open_selector() {
    {
        let controller_program = wasi_shape_rolled_route_seq_arm_program::<0>();
        let worker_program = wasi_shape_rolled_route_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(981);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 1;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_WRITE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_WRITE_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_WRITE_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_WRITE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_WRITE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_WRITE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_WRITE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 2;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 3;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_FDSTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_FDSTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_FDSTAT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_FDSTAT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_FDSTAT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_FDSTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_FDSTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_FDSTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_FDSTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 4;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_PATH_OPEN_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_PATH_OPEN_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_PATH_OPEN_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_PATH_OPEN_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_PATH_OPEN_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_PATH_OPEN_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_PATH_OPEN_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_PATH_OPEN_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 5;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_WRITE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_WRITE_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_WRITE_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_WRITE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_WRITE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_WRITE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_WRITE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 6;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };

            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_PRESTAT_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_long_route_arm_reenters_without_visit_capacity_abort() {
    {
        let controller_program = wasi_shape_rolled_route_seq_arm_program::<0>();
        let worker_program = wasi_shape_rolled_route_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(986);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                {
                    let value: u8 = 10;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            let branch = worker.offer().await.unwrap_or_else(|err| {
                                panic!("worker offer for label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            });
                            assert_eq!(
                                branch.label(),
                                <Msg<WASI_FD_PRESTAT_REQ, u8> as Message>::LOGICAL_LABEL
                            );
                            branch
                                .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                                .await
                                .expect("worker recv")
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 20;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_PRESTAT_DIR_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_PRESTAT_DIR_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_PRESTAT_DIR_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!(
                                        "controller recv label {WASI_FD_PRESTAT_DIR_ACK}: {err:?}"
                                    )
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 30;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_PRESTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_PRESTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_PRESTAT_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_PRESTAT_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_PRESTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_PRESTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_PRESTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_PRESTAT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 40;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_PATH_FILESTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_PATH_FILESTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_PATH_FILESTAT_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_PATH_FILESTAT_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_PATH_FILESTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_PATH_FILESTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_PATH_FILESTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!(
                                        "controller recv label {WASI_PATH_FILESTAT_ACK}: {err:?}"
                                    )
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 50;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_FDSTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_FDSTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_FDSTAT_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_FDSTAT_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_FDSTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_FDSTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_FDSTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_FDSTAT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 60;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_PATH_OPEN_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_PATH_OPEN_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_PATH_OPEN_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_PATH_OPEN_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_PATH_OPEN_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_PATH_OPEN_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_PATH_OPEN_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_PATH_OPEN_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 70;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_FILESTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_FILESTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_FILESTAT_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_FILESTAT_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_FILESTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_FILESTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_FILESTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_FILESTAT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 80;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 90;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 100;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };

                {
                    let value: u8 = 110;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_FDSTAT_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_FDSTAT_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_FDSTAT_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_FDSTAT_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_FDSTAT_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_FDSTAT_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_FDSTAT_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_FDSTAT_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 120;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_PATH_OPEN_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_PATH_OPEN_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_PATH_OPEN_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_PATH_OPEN_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_PATH_OPEN_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_PATH_OPEN_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_PATH_OPEN_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_PATH_OPEN_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 130;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_WRITE_REFINED_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_WRITE_REFINED_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_WRITE_REFINED_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_WRITE_REFINED_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_WRITE_REFINED_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_WRITE_REFINED_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_WRITE_REFINED_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!(
                                        "controller recv label {WASI_FD_WRITE_REFINED_ACK}: {err:?}"
                                    )
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 140;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_WRITE_REFINED_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_WRITE_REFINED_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_WRITE_REFINED_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_WRITE_REFINED_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_WRITE_REFINED_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_WRITE_REFINED_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_WRITE_REFINED_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!(
                                        "controller recv label {WASI_FD_WRITE_REFINED_ACK}: {err:?}"
                                    )
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 150;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_WRITE_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_WRITE_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_WRITE_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_WRITE_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_WRITE_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_WRITE_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_WRITE_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_WRITE_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
                {
                    let value: u8 = 160;
                    {
                        let value: u8 = value;
                        controller
                            .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            worker
                                .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                                })
                        },
                        value
                    );
                    {
                        let value: u8 = value.wrapping_add(1);
                        worker
                            .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                            });
                    };
                    assert_eq!(
                        {
                            controller
                                .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                                .await
                                .unwrap_or_else(|err| {
                                    panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                                })
                        },
                        value.wrapping_add(1)
                    );
                };
            };
            {
                let value: u8 = 200;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_WRITE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_WRITE_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_WRITE_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_WRITE_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_WRITE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_WRITE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_WRITE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_WRITE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_left_repeated_label_seq_arm_keeps_continuation() {
    {
        let controller_program = rolled_left_repeated_label_seq_arm_program::<0>();
        let worker_program = rolled_left_repeated_label_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(982);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_right_route_repeated_label_seq_arm_keeps_continuation() {
    {
        let controller_program = rolled_nested_right_repeated_label_seq_arm_program::<0>();
        let worker_program = rolled_nested_right_repeated_label_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(983);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_left_route_repeated_label_seq_arm_keeps_continuation() {
    {
        let controller_program = rolled_nested_left_repeated_label_seq_arm_program::<0>();
        let worker_program = rolled_nested_left_repeated_label_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(984);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_balanced_route_repeated_label_seq_arm_keeps_continuation() {
    {
        let controller_program = rolled_balanced_repeated_label_seq_arm_program::<0>();
        let worker_program = rolled_balanced_repeated_label_seq_arm_program::<1>();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .expect("register rendezvous");
        let sid = SessionId::new(985);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
        (async {
            {
                let value: u8 = 10;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        let branch = worker.offer().await.unwrap_or_else(|err| {
                            panic!("worker offer for label {WASI_FD_READ_REQ}: {err:?}")
                        });
                        assert_eq!(
                            branch.label(),
                            <Msg<WASI_FD_READ_REQ, u8> as Message>::LOGICAL_LABEL
                        );
                        branch
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .expect("worker recv")
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 20;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_READ_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_READ_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_READ_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_READ_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_READ_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_READ_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_READ_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_READ_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
            {
                let value: u8 = 30;
                {
                    let value: u8 = value;
                    controller
                        .send::<Msg<WASI_FD_CLOSE_REQ, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("controller send label {WASI_FD_CLOSE_REQ}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        worker
                            .recv::<Msg<WASI_FD_CLOSE_REQ, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("worker recv label {WASI_FD_CLOSE_REQ}: {err:?}")
                            })
                    },
                    value
                );
                {
                    let value: u8 = value.wrapping_add(1);
                    worker
                        .send::<Msg<WASI_FD_CLOSE_ACK, u8>>(&value)
                        .await
                        .unwrap_or_else(|err| {
                            panic!("worker send label {WASI_FD_CLOSE_ACK}: {err:?}")
                        });
                };
                assert_eq!(
                    {
                        controller
                            .recv::<Msg<WASI_FD_CLOSE_ACK, u8>>()
                            .await
                            .unwrap_or_else(|err| {
                                panic!("controller recv label {WASI_FD_CLOSE_ACK}: {err:?}")
                            })
                    },
                    value.wrapping_add(1)
                );
            };
        })
        .await;
    };
}
