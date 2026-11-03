mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::{
    g::{self, Message, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};

const ROW_RESOLVER: u16 = 0x0310;
const SAME_LABEL_RESOLVER: u16 = 0x0311;
const OUTER_RESOLVER: u16 = 0x0312;
const INNER_RESOLVER: u16 = 0x0313;
const PAR_LEFT_RESOLVER: u16 = 0x0314;
const PAR_RIGHT_RESOLVER: u16 = 0x0315;
const PASSIVE_OFFER_RESOLVER: u16 = 0x0316;
const DEEP_OUTER_RESOLVER: u16 = 0x0317;
const DEEP_MIDDLE_RESOLVER: u16 = 0x0318;
const DEEP_INNER_RESOLVER: u16 = 0x0319;

const ROW_LEFT_REQ: u8 = 31;
const ROW_LEFT_ACK: u8 = 32;
const ROW_RIGHT_REQ: u8 = 33;
const ROW_RIGHT_ACK: u8 = 34;

const SAME_REQ: u8 = 35;

const OUTER_PREFIX_REQ: u8 = 36;
const OUTER_RIGHT_REQ: u8 = 38;
const INNER_SAME_REQ: u8 = 40;
const INNER_NOTIFY: u8 = 41;

const PAR_LEFT_A: u8 = 44;
const PAR_LEFT_B: u8 = 45;
const PAR_RIGHT_A: u8 = 46;
const PAR_RIGHT_B: u8 = 47;
const PASSIVE_OFFER_LEFT: u8 = 48;
const PASSIVE_OFFER_RIGHT: u8 = 49;
const PASSIVE_NESTED_PREFIX: u8 = 50;
const PASSIVE_NESTED_OUTER_RIGHT: u8 = 51;
const PASSIVE_NESTED_INNER_LEFT: u8 = 52;
const PASSIVE_NESTED_INNER_RIGHT: u8 = 53;
const MIRROR_OUTER_LEFT_REQ: u8 = 54;
const MIRROR_OUTER_PREFIX_REQ: u8 = 55;
const MIRROR_INNER_SAME_REQ: u8 = 56;
const MIRROR_INNER_NOTIFY: u8 = 57;
const PASSIVE_MIRROR_OUTER_LEFT: u8 = 58;
const PASSIVE_MIRROR_PREFIX: u8 = 59;
const PASSIVE_MIRROR_INNER_LEFT: u8 = 60;
const PASSIVE_MIRROR_INNER_RIGHT: u8 = 61;
const DEEP_RIGHT_OUTER_LEFT: u8 = 62;
const DEEP_RIGHT_OUTER_PREFIX: u8 = 63;
const DEEP_RIGHT_MIDDLE_LEFT: u8 = 64;
const DEEP_RIGHT_MIDDLE_PREFIX: u8 = 65;
const DEEP_RIGHT_INNER_LEFT: u8 = 66;
const DEEP_RIGHT_INNER_RIGHT: u8 = 67;
const DEEP_LEFT_OUTER_PREFIX: u8 = 68;
const DEEP_LEFT_MIDDLE_PREFIX: u8 = 69;
const DEEP_LEFT_INNER_LEFT: u8 = 70;
const DEEP_LEFT_INNER_RIGHT: u8 = 71;
const DEEP_LEFT_MIDDLE_RIGHT: u8 = 72;
const DEEP_LEFT_OUTER_RIGHT: u8 = 73;
const MIXED_OUTER_RIGHT_REQ: u8 = 74;
const MIXED_OUTER_RIGHT_ACK: u8 = 75;
const MIXED_INNER_LEFT_REQ: u8 = 76;
const MIXED_INNER_LEFT_ACK: u8 = 77;
const MIXED_INNER_RIGHT_REQ: u8 = 78;
const MIXED_INNER_RIGHT_ACK: u8 = 79;
const MIXED_SEQ_TAIL_REQ: u8 = 80;
const MIXED_SEQ_TAIL_ACK: u8 = 81;
const MIXED_PAR_SIBLING_REQ: u8 = 82;
const MIXED_PAR_SIBLING_ACK: u8 = 83;
const MIXED_OUTER_LEFT_REQ: u8 = 84;
const MIXED_OUTER_LEFT_ACK: u8 = 85;

fn last_route_selection(events: &[tap::TapEvent]) -> Option<tap::TapEvent> {
    events
        .iter()
        .rev()
        .find(|event| event.id() == tap::ROUTE_ARM_SELECTION)
        .copied()
}

fn route_selection_count(events: &[tap::TapEvent]) -> usize {
    events
        .iter()
        .filter(|event| event.id() == tap::ROUTE_ARM_SELECTION)
        .count()
}

fn next_decision(
    (decisions, calls): &(&[DecisionArm], core::cell::Cell<usize>),
) -> Result<DecisionArm, ResolverError> {
    let index = calls.get();
    let decision = *decisions.get(index).expect("resolver script exhausted");
    calls.set(index + 1);
    Ok(decision)
}

macro_rules! row {
    ($req:ident, $ack:ident) => {
        g::seq(
            g::send::<0, 1, Msg<$req, u32>>(),
            g::send::<1, 0, Msg<$ack, u32>>(),
        )
    };
}

fn rolled_resolved_row_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            row!(ROW_LEFT_REQ, ROW_LEFT_ACK),
            row!(ROW_RIGHT_REQ, ROW_RIGHT_ACK),
        )
        .resolve::<ROW_RESOLVER>()
        .roll(),
    )
}

fn rolled_same_label_target_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::send::<0, 1, Msg<SAME_REQ, u32>>(),
            g::send::<0, 1, Msg<SAME_REQ, u64>>(),
        )
        .resolve::<SAME_LABEL_RESOLVER>()
        .roll(),
    )
}

fn rolled_nested_resolved_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<INNER_SAME_REQ, u32>>(),
        g::seq(
            g::send::<0, 1, Msg<INNER_SAME_REQ, u64>>(),
            g::send::<0, 1, Msg<INNER_NOTIFY, u32>>(),
        ),
    )
    .resolve::<INNER_RESOLVER>();
    let outer_left = g::seq(g::send::<0, 1, Msg<OUTER_PREFIX_REQ, u32>>(), inner);
    project(
        &g::route(outer_left, g::send::<0, 1, Msg<OUTER_RIGHT_REQ, u32>>())
            .resolve::<OUTER_RESOLVER>()
            .roll(),
    )
}

fn rolled_mirrored_nested_resolved_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<MIRROR_INNER_SAME_REQ, u32>>(),
        g::seq(
            g::send::<0, 1, Msg<MIRROR_INNER_SAME_REQ, u64>>(),
            g::send::<0, 1, Msg<MIRROR_INNER_NOTIFY, u32>>(),
        ),
    )
    .resolve::<INNER_RESOLVER>();
    let outer_right = g::seq(g::send::<0, 1, Msg<MIRROR_OUTER_PREFIX_REQ, u32>>(), inner);
    project(
        &g::route(
            g::send::<0, 1, Msg<MIRROR_OUTER_LEFT_REQ, u32>>(),
            outer_right,
        )
        .resolve::<OUTER_RESOLVER>()
        .roll(),
    )
}

fn rolled_parallel_resolved_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left_route = g::route(
        g::send::<0, 1, Msg<PAR_LEFT_A, u32>>(),
        g::send::<0, 1, Msg<PAR_LEFT_B, u32>>(),
    )
    .resolve::<PAR_LEFT_RESOLVER>();
    let right_route = g::route(
        g::send::<0, 2, Msg<PAR_RIGHT_A, u32>>(),
        g::send::<0, 2, Msg<PAR_RIGHT_B, u32>>(),
    )
    .resolve::<PAR_RIGHT_RESOLVER>();
    project(&g::par(left_route, right_route).roll())
}

fn rolled_passive_offer_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::send::<1, 0, Msg<PASSIVE_OFFER_LEFT, u32>>(),
            g::send::<1, 0, Msg<PASSIVE_OFFER_RIGHT, u32>>(),
        )
        .resolve::<PASSIVE_OFFER_RESOLVER>()
        .roll(),
    )
}

fn rolled_nested_passive_offer_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<1, 0, Msg<PASSIVE_NESTED_INNER_LEFT, u32>>(),
        g::send::<1, 0, Msg<PASSIVE_NESTED_INNER_RIGHT, u32>>(),
    )
    .resolve::<INNER_RESOLVER>();
    let outer_left = g::seq(g::send::<1, 0, Msg<PASSIVE_NESTED_PREFIX, u32>>(), inner);
    project(
        &g::route(
            outer_left,
            g::send::<1, 0, Msg<PASSIVE_NESTED_OUTER_RIGHT, u32>>(),
        )
        .resolve::<OUTER_RESOLVER>()
        .roll(),
    )
}

fn rolled_mirrored_nested_passive_offer_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<1, 0, Msg<PASSIVE_MIRROR_INNER_LEFT, u32>>(),
        g::send::<1, 0, Msg<PASSIVE_MIRROR_INNER_RIGHT, u32>>(),
    )
    .resolve::<INNER_RESOLVER>();
    let outer_right = g::seq(g::send::<1, 0, Msg<PASSIVE_MIRROR_PREFIX, u32>>(), inner);
    project(
        &g::route(
            g::send::<1, 0, Msg<PASSIVE_MIRROR_OUTER_LEFT, u32>>(),
            outer_right,
        )
        .resolve::<OUTER_RESOLVER>()
        .roll(),
    )
}

fn rolled_deep_right_spine_passive_offer_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<1, 0, Msg<DEEP_RIGHT_INNER_LEFT, u32>>(),
        g::send::<1, 0, Msg<DEEP_RIGHT_INNER_RIGHT, u32>>(),
    )
    .resolve::<DEEP_INNER_RESOLVER>();
    let middle_right = g::seq(g::send::<1, 0, Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>(), inner);
    let middle = g::route(
        g::send::<1, 0, Msg<DEEP_RIGHT_MIDDLE_LEFT, u32>>(),
        middle_right,
    )
    .resolve::<DEEP_MIDDLE_RESOLVER>();
    let outer_right = g::seq(g::send::<1, 0, Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(), middle);
    project(
        &g::route(
            g::send::<1, 0, Msg<DEEP_RIGHT_OUTER_LEFT, u32>>(),
            outer_right,
        )
        .resolve::<DEEP_OUTER_RESOLVER>()
        .roll(),
    )
}

fn rolled_deep_left_spine_passive_offer_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<1, 0, Msg<DEEP_LEFT_INNER_LEFT, u32>>(),
        g::send::<1, 0, Msg<DEEP_LEFT_INNER_RIGHT, u32>>(),
    )
    .resolve::<DEEP_INNER_RESOLVER>();
    let middle_left = g::seq(g::send::<1, 0, Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>(), inner);
    let middle = g::route(
        middle_left,
        g::send::<1, 0, Msg<DEEP_LEFT_MIDDLE_RIGHT, u32>>(),
    )
    .resolve::<DEEP_MIDDLE_RESOLVER>();
    let outer_left = g::seq(g::send::<1, 0, Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(), middle);
    project(
        &g::route(
            outer_left,
            g::send::<1, 0, Msg<DEEP_LEFT_OUTER_RIGHT, u32>>(),
        )
        .resolve::<DEEP_OUTER_RESOLVER>()
        .roll(),
    )
}

fn rolled_resolved_mixed_left_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner_route = g::route(
        row!(MIXED_INNER_LEFT_REQ, MIXED_INNER_LEFT_ACK),
        row!(MIXED_INNER_RIGHT_REQ, MIXED_INNER_RIGHT_ACK),
    )
    .resolve::<INNER_RESOLVER>()
    .roll();
    let seq_roll = g::seq(inner_route, row!(MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK)).roll();
    let par_roll = g::par(seq_roll, row!(MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK)).roll();
    project(
        &g::route(par_roll, row!(MIXED_OUTER_RIGHT_REQ, MIXED_OUTER_RIGHT_ACK))
            .resolve::<OUTER_RESOLVER>()
            .roll(),
    )
}

fn rolled_resolved_mixed_right_arm_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner_route = g::route(
        row!(MIXED_INNER_LEFT_REQ, MIXED_INNER_LEFT_ACK),
        row!(MIXED_INNER_RIGHT_REQ, MIXED_INNER_RIGHT_ACK),
    )
    .resolve::<INNER_RESOLVER>()
    .roll();
    let seq_roll = g::seq(inner_route, row!(MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK)).roll();
    let par_roll = g::par(seq_roll, row!(MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK)).roll();
    project(
        &g::route(row!(MIXED_OUTER_LEFT_REQ, MIXED_OUTER_LEFT_ACK), par_roll)
            .resolve::<OUTER_RESOLVER>()
            .roll(),
    )
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_route_reenters_left_right_left_rows() {
    let row: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_row_program::<0>();
            let role1 = rolled_resolved_row_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<ROW_RESOLVER>::decision_state(&row, next_decision),
            )
            .expect("install row resolver");
            let sid = SessionId::new(0x0009_3001);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                {
                    origin
                        .send::<Msg<ROW_LEFT_REQ, u32>>(&1)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<ROW_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        1
                    );
                    let ack = 1 + 10_000;
                    peer.send::<Msg<ROW_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<ROW_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<ROW_RIGHT_REQ, u32>>(&2)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<ROW_RIGHT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        2
                    );
                    let ack = 2 + 10_000;
                    peer.send::<Msg<ROW_RIGHT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<ROW_RIGHT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<ROW_LEFT_REQ, u32>>(&3)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<ROW_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        3
                    );
                    let ack = 3 + 10_000;
                    peer.send::<Msg<ROW_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<ROW_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
            }
        };
    };
    assert_eq!(row.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_same_label_reenters_with_selected_schema() {
    let same_label: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_same_label_target_program::<0>();
            let role1 = rolled_same_label_target_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<SAME_LABEL_RESOLVER>::decision_state(&same_label, next_decision),
            )
            .expect("install same-label resolver");
            let sid = SessionId::new(0x0009_3002);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                origin
                    .send::<Msg<SAME_REQ, u32>>(&11)
                    .await
                    .expect("left schema selected");
                assert_eq!(
                    peer.recv::<Msg<SAME_REQ, u32>>()
                        .await
                        .expect("left schema recv"),
                    11
                );

                origin
                    .send::<Msg<SAME_REQ, u64>>(&12)
                    .await
                    .expect("right schema selected");
                assert_eq!(
                    peer.recv::<Msg<SAME_REQ, u64>>()
                        .await
                        .expect("right schema recv"),
                    12
                );

                origin
                    .send::<Msg<SAME_REQ, u32>>(&13)
                    .await
                    .expect("left schema reentry");
                assert_eq!(
                    peer.recv::<Msg<SAME_REQ, u32>>()
                        .await
                        .expect("left schema reentry recv"),
                    13
                );
            }
        };
    };
    assert_eq!(same_label.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_resolved_route_reenters_asymmetric_paths() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_nested_resolved_program::<0>();
            let role1 = rolled_nested_resolved_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install inner resolver");
            let sid = SessionId::new(0x0009_3003);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                origin
                    .send::<Msg<OUTER_PREFIX_REQ, u32>>(&21)
                    .await
                    .expect("outer prefix send");
                assert_eq!(
                    peer.recv::<Msg<OUTER_PREFIX_REQ, u32>>()
                        .await
                        .expect("outer prefix recv"),
                    21
                );
                origin
                    .send::<Msg<INNER_SAME_REQ, u64>>(&22)
                    .await
                    .expect("inner right schema selected");
                assert_eq!(
                    peer.recv::<Msg<INNER_SAME_REQ, u64>>()
                        .await
                        .expect("inner right schema recv"),
                    22
                );
                origin
                    .send::<Msg<INNER_NOTIFY, u32>>(&22)
                    .await
                    .expect("inner right completion notify send");
                assert_eq!(
                    peer.recv::<Msg<INNER_NOTIFY, u32>>()
                        .await
                        .expect("inner right completion notify"),
                    22
                );
                origin
                    .send::<Msg<OUTER_RIGHT_REQ, u32>>(&23)
                    .await
                    .expect("outer right send");
                assert_eq!(
                    peer.recv::<Msg<OUTER_RIGHT_REQ, u32>>()
                        .await
                        .expect("outer right recv"),
                    23
                );
                origin
                    .send::<Msg<OUTER_PREFIX_REQ, u32>>(&24)
                    .await
                    .expect("outer prefix reentry send");
                assert_eq!(
                    peer.recv::<Msg<OUTER_PREFIX_REQ, u32>>()
                        .await
                        .expect("outer prefix reentry recv"),
                    24
                );
                origin
                    .send::<Msg<INNER_SAME_REQ, u32>>(&25)
                    .await
                    .expect("inner left schema selected");
                assert_eq!(
                    peer.recv::<Msg<INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner left schema recv"),
                    25
                );
            }
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_mirrored_nested_resolved_route_reenters_asymmetric_paths() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_mirrored_nested_resolved_program::<0>();
            let role1 = rolled_mirrored_nested_resolved_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install inner resolver");
            let sid = SessionId::new(0x0009_3007);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                origin
                    .send::<Msg<MIRROR_OUTER_PREFIX_REQ, u32>>(&61)
                    .await
                    .expect("outer right prefix send");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_OUTER_PREFIX_REQ, u32>>()
                        .await
                        .expect("outer right prefix recv"),
                    61
                );
                origin
                    .send::<Msg<MIRROR_INNER_SAME_REQ, u64>>(&62)
                    .await
                    .expect("inner right schema selected");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_INNER_SAME_REQ, u64>>()
                        .await
                        .expect("inner right schema recv"),
                    62
                );
                origin
                    .send::<Msg<MIRROR_INNER_NOTIFY, u32>>(&62)
                    .await
                    .expect("inner right completion notify send");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_INNER_NOTIFY, u32>>()
                        .await
                        .expect("inner right completion notify"),
                    62
                );

                origin
                    .send::<Msg<MIRROR_OUTER_LEFT_REQ, u32>>(&63)
                    .await
                    .expect("outer left selected send");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_OUTER_LEFT_REQ, u32>>()
                        .await
                        .expect("outer left recv"),
                    63
                );

                origin
                    .send::<Msg<MIRROR_OUTER_PREFIX_REQ, u32>>(&64)
                    .await
                    .expect("outer right reentry prefix send");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_OUTER_PREFIX_REQ, u32>>()
                        .await
                        .expect("outer right reentry prefix recv"),
                    64
                );
                origin
                    .send::<Msg<MIRROR_INNER_SAME_REQ, u32>>(&65)
                    .await
                    .expect("inner left schema selected");
                assert_eq!(
                    peer.recv::<Msg<MIRROR_INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner left schema recv"),
                    65
                );
            }
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_parallel_resolved_routes_reenter_after_both_lanes_settle() {
    let par_left: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    let par_right: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_parallel_resolved_program::<0>();
            let role1 = rolled_parallel_resolved_program::<1>();
            let role2 = rolled_parallel_resolved_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<PAR_LEFT_RESOLVER>::decision_state(&par_left, next_decision),
            )
            .expect("install left parallel resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<PAR_RIGHT_RESOLVER>::decision_state(&par_right, next_decision),
            )
            .expect("install right parallel resolver");
            let sid = SessionId::new(0x0009_3004);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            {
                origin
                    .send::<Msg<PAR_LEFT_A, u32>>(&31)
                    .await
                    .expect("first left-route send");
                origin
                    .send::<Msg<PAR_RIGHT_B, u32>>(&32)
                    .await
                    .expect("first right-route send");
                assert_eq!(
                    left_peer
                        .recv::<Msg<PAR_LEFT_A, u32>>()
                        .await
                        .expect("left recv"),
                    31
                );
                assert_eq!(
                    right_peer
                        .recv::<Msg<PAR_RIGHT_B, u32>>()
                        .await
                        .expect("right recv"),
                    32
                );

                origin
                    .send::<Msg<PAR_LEFT_B, u32>>(&33)
                    .await
                    .expect("second left-route send after roll reentry");
                origin
                    .send::<Msg<PAR_RIGHT_A, u32>>(&34)
                    .await
                    .expect("second right-route send after roll reentry");
                assert_eq!(
                    left_peer
                        .recv::<Msg<PAR_LEFT_B, u32>>()
                        .await
                        .expect("left recv"),
                    33
                );
                assert_eq!(
                    right_peer
                        .recv::<Msg<PAR_RIGHT_A, u32>>()
                        .await
                        .expect("right recv"),
                    34
                );
            }
        };
    };
    assert_eq!(par_left.1.get(), 2);
    assert_eq!(par_right.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_route_par_seq_route_roll_mixed_corpus_reenters_by_scope() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_mixed_left_arm_program::<0>();
            let role1 = rolled_resolved_mixed_left_arm_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install mixed outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install mixed inner resolver");
            let sid = SessionId::new(0x0009_3011);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                {
                    origin
                        .send::<Msg<MIXED_INNER_LEFT_REQ, u32>>(&101)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        101
                    );
                    let ack = 101 + 10_000;
                    peer.send::<Msg<MIXED_INNER_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_INNER_RIGHT_REQ, u32>>(&102)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_RIGHT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        102
                    );
                    let ack = 102 + 10_000;
                    peer.send::<Msg<MIXED_INNER_RIGHT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_RIGHT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_SEQ_TAIL_REQ, u32>>(&103)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_SEQ_TAIL_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        103
                    );
                    let ack = 103 + 10_000;
                    peer.send::<Msg<MIXED_SEQ_TAIL_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_SEQ_TAIL_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_PAR_SIBLING_REQ, u32>>(&104)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_PAR_SIBLING_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        104
                    );
                    let ack = 104 + 10_000;
                    peer.send::<Msg<MIXED_PAR_SIBLING_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_PAR_SIBLING_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_OUTER_RIGHT_REQ, u32>>(&105)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_OUTER_RIGHT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        105
                    );
                    let ack = 105 + 10_000;
                    peer.send::<Msg<MIXED_OUTER_RIGHT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_OUTER_RIGHT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_INNER_RIGHT_REQ, u32>>(&106)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_RIGHT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        106
                    );
                    let ack = 106 + 10_000;
                    peer.send::<Msg<MIXED_INNER_RIGHT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_RIGHT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_SEQ_TAIL_REQ, u32>>(&107)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_SEQ_TAIL_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        107
                    );
                    let ack = 107 + 10_000;
                    peer.send::<Msg<MIXED_SEQ_TAIL_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_SEQ_TAIL_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_PAR_SIBLING_REQ, u32>>(&108)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_PAR_SIBLING_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        108
                    );
                    let ack = 108 + 10_000;
                    peer.send::<Msg<MIXED_PAR_SIBLING_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_PAR_SIBLING_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
            }
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_route_par_seq_route_roll_mixed_right_corpus_reenters_by_scope() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_mixed_right_arm_program::<0>();
            let role1 = rolled_resolved_mixed_right_arm_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install mixed right outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install mixed right inner resolver");
            let sid = SessionId::new(0x0009_3012);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            {
                {
                    origin
                        .send::<Msg<MIXED_INNER_RIGHT_REQ, u32>>(&201)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_RIGHT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        201
                    );
                    let ack = 201 + 10_000;
                    peer.send::<Msg<MIXED_INNER_RIGHT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_RIGHT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_INNER_LEFT_REQ, u32>>(&202)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        202
                    );
                    let ack = 202 + 10_000;
                    peer.send::<Msg<MIXED_INNER_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_SEQ_TAIL_REQ, u32>>(&203)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_SEQ_TAIL_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        203
                    );
                    let ack = 203 + 10_000;
                    peer.send::<Msg<MIXED_SEQ_TAIL_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_SEQ_TAIL_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_PAR_SIBLING_REQ, u32>>(&204)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_PAR_SIBLING_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        204
                    );
                    let ack = 204 + 10_000;
                    peer.send::<Msg<MIXED_PAR_SIBLING_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_PAR_SIBLING_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_OUTER_LEFT_REQ, u32>>(&205)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_OUTER_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        205
                    );
                    let ack = 205 + 10_000;
                    peer.send::<Msg<MIXED_OUTER_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_OUTER_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_INNER_LEFT_REQ, u32>>(&206)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_INNER_LEFT_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        206
                    );
                    let ack = 206 + 10_000;
                    peer.send::<Msg<MIXED_INNER_LEFT_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_INNER_LEFT_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_SEQ_TAIL_REQ, u32>>(&207)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_SEQ_TAIL_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        207
                    );
                    let ack = 207 + 10_000;
                    peer.send::<Msg<MIXED_SEQ_TAIL_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_SEQ_TAIL_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
                {
                    origin
                        .send::<Msg<MIXED_PAR_SIBLING_REQ, u32>>(&208)
                        .await
                        .expect("request send");
                    assert_eq!(
                        peer.recv::<Msg<MIXED_PAR_SIBLING_REQ, u32>>()
                            .await
                            .expect("request recv"),
                        208
                    );
                    let ack = 208 + 10_000;
                    peer.send::<Msg<MIXED_PAR_SIBLING_ACK, u32>>(&ack)
                        .await
                        .expect("ack send");
                    assert_eq!(
                        origin
                            .recv::<Msg<MIXED_PAR_SIBLING_ACK, u32>>()
                            .await
                            .expect("ack recv"),
                        ack
                    );
                }
            }
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_route_reenters_passive_offer_left_right_left() {
    let passive_offer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_passive_offer_program::<0>();
            let sender_program = rolled_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<PASSIVE_OFFER_RESOLVER>::decision_state(
                    &passive_offer,
                    next_decision,
                ),
            )
            .expect("install passive-offer resolver");
            let sid = SessionId::new(0x0009_3005);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&41)
                    .await
                    .expect("left selected send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        41
                    );
                }

                sender
                    .send::<Msg<PASSIVE_OFFER_RIGHT, u32>>(&42)
                    .await
                    .expect("right selected send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        42
                    );
                }

                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&43)
                    .await
                    .expect("left selected reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        43
                    );
                }
            })
            .await;
        };
    };
    assert_eq!(passive_offer.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_resolved_route_preserves_buffered_decision_order() {
    let passive_offer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_passive_offer_program::<0>();
            let sender_program = rolled_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<PASSIVE_OFFER_RESOLVER>::decision_state(
                    &passive_offer,
                    next_decision,
                ),
            )
            .expect("install passive-offer resolver");
            let sid = SessionId::new(0x0009_3013);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&51)
                    .await
                    .expect("buffer first left decision");
                sender
                    .send::<Msg<PASSIVE_OFFER_RIGHT, u32>>(&52)
                    .await
                    .expect("buffer following right decision");
                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&53)
                    .await
                    .expect("buffer final left decision");

                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        51
                    );
                }
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        52
                    );
                }
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_OFFER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_OFFER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_OFFER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        53
                    );
                }
            })
            .await;
        };
    };
    assert_eq!(passive_offer.1.get(), 3);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_nested_resolved_route_reenters_passive_offer_asymmetric_paths() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_nested_passive_offer_program::<0>();
            let sender_program = rolled_nested_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install nested passive outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install nested passive inner resolver");
            let sid = SessionId::new(0x0009_3006);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<PASSIVE_NESTED_PREFIX, u32>>(&51)
                    .await
                    .expect("outer left prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_NESTED_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_NESTED_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_NESTED_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        51
                    );
                }
                sender
                    .send::<Msg<PASSIVE_NESTED_INNER_RIGHT, u32>>(&52)
                    .await
                    .expect("inner right selected send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_NESTED_INNER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_NESTED_INNER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_NESTED_INNER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        52
                    );
                }

                sender
                    .send::<Msg<PASSIVE_NESTED_OUTER_RIGHT, u32>>(&53)
                    .await
                    .expect("outer right selected send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_NESTED_OUTER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_NESTED_OUTER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_NESTED_OUTER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        53
                    );
                }

                sender
                    .send::<Msg<PASSIVE_NESTED_PREFIX, u32>>(&54)
                    .await
                    .expect("outer left reentry prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_NESTED_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_NESTED_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_NESTED_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        54
                    );
                }
                sender
                    .send::<Msg<PASSIVE_NESTED_INNER_LEFT, u32>>(&55)
                    .await
                    .expect("inner left selected send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            PASSIVE_NESTED_INNER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<PASSIVE_NESTED_INNER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<PASSIVE_NESTED_INNER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        55
                    );
                }
            })
            .await;
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_mirrored_nested_resolved_route_reenters_passive_offer_asymmetric_paths() {
    let outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    let inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_mirrored_nested_passive_offer_program::<0>();
            let sender_program = rolled_mirrored_nested_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&outer, next_decision),
            )
            .expect("install nested passive outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<INNER_RESOLVER>::decision_state(&inner, next_decision),
            )
            .expect("install nested passive inner resolver");
            let sid = SessionId::new(0x0009_3008);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<PASSIVE_MIRROR_PREFIX, u32>>(&71)
                    .await
                    .expect("outer right prefix send");
                {let branch=receiver.offer().await.unwrap_or_else(|err| panic!("offer selected route arm for message {}: {err:?}",PASSIVE_MIRROR_PREFIX)); assert_eq!(branch.label(),<Msg<PASSIVE_MIRROR_PREFIX,u32> as Message>::LOGICAL_LABEL); assert_eq!(branch.recv::<Msg<PASSIVE_MIRROR_PREFIX,u32>>().await.expect("passive branch recv"),71);}
                sender
                    .send::<Msg<PASSIVE_MIRROR_INNER_RIGHT, u32>>(&72)
                    .await
                    .expect("inner right selected send");
                {let branch=receiver.offer().await.unwrap_or_else(|err| panic!("offer selected route arm for message {}: {err:?}",PASSIVE_MIRROR_INNER_RIGHT)); assert_eq!(branch.label(),<Msg<PASSIVE_MIRROR_INNER_RIGHT,u32> as Message>::LOGICAL_LABEL); assert_eq!(branch.recv::<Msg<PASSIVE_MIRROR_INNER_RIGHT,u32>>().await.expect("passive branch recv"),72);}

                let before_outer_left = rv.tap().collect::<Vec<_>>();
                sender
                    .send::<Msg<PASSIVE_MIRROR_OUTER_LEFT, u32>>(&73)
                    .await
                    .expect("outer left selected send");
                let events = rv.tap().collect::<Vec<_>>();
                assert!(
                    route_selection_count(&events) > route_selection_count(&before_outer_left),
                    "outer-left reentry send must publish a fresh route selection: before={before_outer_left:?} after={events:?}"
                );
                let last_selection = last_route_selection(&events);
                assert_eq!(
                    last_selection.map(|event| event.arg1() & 0xff),
                    Some(0),
                    "outer-left reentry send must publish the new route arm before passive offer"
                );
                assert_eq!(
                    last_selection.map(|event| event.causal_key() >> 8),
                    Some(1),
                    "outer-left reentry route selection must identify the executing controller: {events:?}"
                );
                let committed_send = events
                    .iter()
                    .rev()
                    .find(|event| event.id() == tap::ENDPOINT_SEND);
                assert_eq!(
                    committed_send.map(|event| event.arg1() >> 24),
                    Some(1),
                    "outer-left reentry send must retain its logical view: {events:?}"
                );
                assert_eq!(
                    committed_send.map(|event| (event.arg1() >> 16) & 0xff),
                    Some(0),
                    "outer-left reentry send must commit on the offer lane: {events:?}"
                );
                assert_eq!(
                    committed_send.map(|event| event.arg1() & 0xffff),
                    Some(0),
                    "outer-left reentry send must identify its actual source event: {events:?}"
                );
                assert_eq!(
                    last_selection.map(|event| event.arg1() >> 16),
                    Some(1),
                    "outer-left reentry route selection must publish the outer route site: {events:?}"
                );
                assert_eq!(
                    last_selection.map(|event| event.causal_key() & 0xff),
                    Some(2),
                    "outer-left reentry route selection must be resolver authority: {events:?}"
                );
                {let branch=receiver.offer().await.unwrap_or_else(|err| panic!("offer selected route arm for message {}: {err:?}",PASSIVE_MIRROR_OUTER_LEFT)); assert_eq!(branch.label(),<Msg<PASSIVE_MIRROR_OUTER_LEFT,u32> as Message>::LOGICAL_LABEL); assert_eq!(branch.recv::<Msg<PASSIVE_MIRROR_OUTER_LEFT,u32>>().await.expect("passive branch recv"),73);}

                sender
                    .send::<Msg<PASSIVE_MIRROR_PREFIX, u32>>(&74)
                    .await
                    .expect("outer right reentry prefix send");
                {let branch=receiver.offer().await.unwrap_or_else(|err| panic!("offer selected route arm for message {}: {err:?}",PASSIVE_MIRROR_PREFIX)); assert_eq!(branch.label(),<Msg<PASSIVE_MIRROR_PREFIX,u32> as Message>::LOGICAL_LABEL); assert_eq!(branch.recv::<Msg<PASSIVE_MIRROR_PREFIX,u32>>().await.expect("passive branch recv"),74);}
                sender
                    .send::<Msg<PASSIVE_MIRROR_INNER_LEFT, u32>>(&75)
                    .await
                    .expect("inner left selected send");
                {let branch=receiver.offer().await.unwrap_or_else(|err| panic!("offer selected route arm for message {}: {err:?}",PASSIVE_MIRROR_INNER_LEFT)); assert_eq!(branch.label(),<Msg<PASSIVE_MIRROR_INNER_LEFT,u32> as Message>::LOGICAL_LABEL); assert_eq!(branch.recv::<Msg<PASSIVE_MIRROR_INNER_LEFT,u32>>().await.expect("passive branch recv"),75);}
            }).await;
        };
    };
    assert_eq!(outer.1.get(), 3);
    assert_eq!(inner.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_deep_right_spine_passive_offer_reenters_across_all_depths() {
    let deep_outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[
            DecisionArm::Right,
            DecisionArm::Left,
            DecisionArm::Right,
            DecisionArm::Right,
        ],
        core::cell::Cell::new(0),
    );
    let deep_middle: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
        core::cell::Cell::new(0),
    );
    let deep_inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_deep_right_spine_passive_offer_program::<0>();
            let sender_program = rolled_deep_right_spine_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_OUTER_RESOLVER>::decision_state(&deep_outer, next_decision),
            )
            .expect("install deep outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_MIDDLE_RESOLVER>::decision_state(&deep_middle, next_decision),
            )
            .expect("install deep middle resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_INNER_RESOLVER>::decision_state(&deep_inner, next_decision),
            )
            .expect("install deep inner resolver");
            let sid = SessionId::new(0x0009_3009);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&81)
                    .await
                    .expect("deep right outer prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        81
                    );
                }
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>(&82)
                    .await
                    .expect("deep right middle prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_MIDDLE_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        82
                    );
                }
                sender
                    .send::<Msg<DEEP_RIGHT_INNER_RIGHT, u32>>(&83)
                    .await
                    .expect("deep right inner right send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_INNER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_INNER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_INNER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        83
                    );
                }

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_LEFT, u32>>(&84)
                    .await
                    .expect("deep outer left reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_OUTER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_OUTER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_OUTER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        84
                    );
                }

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&85)
                    .await
                    .expect("deep right outer prefix reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        85
                    );
                }
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_LEFT, u32>>(&86)
                    .await
                    .expect("deep middle left reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_MIDDLE_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_MIDDLE_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_MIDDLE_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        86
                    );
                }

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&87)
                    .await
                    .expect("deep right outer prefix second reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        87
                    );
                }
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>(&88)
                    .await
                    .expect("deep right middle prefix second reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_MIDDLE_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        88
                    );
                }
                sender
                    .send::<Msg<DEEP_RIGHT_INNER_LEFT, u32>>(&89)
                    .await
                    .expect("deep inner left reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_RIGHT_INNER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_RIGHT_INNER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_RIGHT_INNER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        89
                    );
                }
            })
            .await;
        };
    };
    assert_eq!(deep_outer.1.get(), 4);
    assert_eq!(deep_middle.1.get(), 3);
    assert_eq!(deep_inner.1.get(), 2);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_deep_left_spine_passive_offer_reenters_across_all_depths() {
    let deep_outer: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[
            DecisionArm::Left,
            DecisionArm::Right,
            DecisionArm::Left,
            DecisionArm::Left,
        ],
        core::cell::Cell::new(0),
    );
    let deep_middle: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    let deep_inner: (&[DecisionArm], core::cell::Cell<usize>) = (
        &[DecisionArm::Right, DecisionArm::Left],
        core::cell::Cell::new(0),
    );
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_deep_left_spine_passive_offer_program::<0>();
            let sender_program = rolled_deep_left_spine_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_OUTER_RESOLVER>::decision_state(&deep_outer, next_decision),
            )
            .expect("install deep outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_MIDDLE_RESOLVER>::decision_state(&deep_middle, next_decision),
            )
            .expect("install deep middle resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_INNER_RESOLVER>::decision_state(&deep_inner, next_decision),
            )
            .expect("install deep inner resolver");
            let sid = SessionId::new(0x0009_3010);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            (async {
                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&91)
                    .await
                    .expect("deep left outer prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        91
                    );
                }
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>(&92)
                    .await
                    .expect("deep left middle prefix send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_MIDDLE_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_MIDDLE_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        92
                    );
                }
                sender
                    .send::<Msg<DEEP_LEFT_INNER_RIGHT, u32>>(&93)
                    .await
                    .expect("deep left inner right send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_INNER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_INNER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_INNER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        93
                    );
                }

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_RIGHT, u32>>(&94)
                    .await
                    .expect("deep outer right reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_OUTER_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_OUTER_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_OUTER_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        94
                    );
                }

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&95)
                    .await
                    .expect("deep left outer prefix reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        95
                    );
                }
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_RIGHT, u32>>(&96)
                    .await
                    .expect("deep middle right reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_MIDDLE_RIGHT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_MIDDLE_RIGHT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_MIDDLE_RIGHT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        96
                    );
                }

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&97)
                    .await
                    .expect("deep left outer prefix second reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_OUTER_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_OUTER_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        97
                    );
                }
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>(&98)
                    .await
                    .expect("deep left middle prefix second reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_MIDDLE_PREFIX
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_MIDDLE_PREFIX, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>()
                            .await
                            .expect("passive branch recv"),
                        98
                    );
                }
                sender
                    .send::<Msg<DEEP_LEFT_INNER_LEFT, u32>>(&99)
                    .await
                    .expect("deep inner left reentry send");
                {
                    let branch = receiver.offer().await.unwrap_or_else(|err| {
                        panic!(
                            "offer selected route arm for message {}: {err:?}",
                            DEEP_LEFT_INNER_LEFT
                        )
                    });
                    assert_eq!(
                        branch.label(),
                        <Msg<DEEP_LEFT_INNER_LEFT, u32> as Message>::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch
                            .recv::<Msg<DEEP_LEFT_INNER_LEFT, u32>>()
                            .await
                            .expect("passive branch recv"),
                        99
                    );
                }
            })
            .await;
        };
    };
    assert_eq!(deep_outer.1.get(), 4);
    assert_eq!(deep_middle.1.get(), 3);
    assert_eq!(deep_inner.1.get(), 2);
}
