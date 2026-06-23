mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;
use std::cell::RefCell;

use common::TestTransport;
use futures::FutureExt;
use hibana::{
    Endpoint,
    g::{self, Message, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

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

static UNIT: () = ();

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static SCRIPTS: RefCell<ResolverScripts> = RefCell::new(ResolverScripts::new());
}

#[derive(Clone, Copy)]
enum ScriptSlot {
    Row,
    SameLabel,
    Outer,
    Inner,
    ParLeft,
    ParRight,
    PassiveOffer,
    DeepOuter,
    DeepMiddle,
    DeepInner,
}

#[derive(Default)]
struct DecisionScript {
    decisions: Vec<DecisionArm>,
    cursor: usize,
    calls: usize,
}

impl DecisionScript {
    fn set(&mut self, decisions: &[DecisionArm]) {
        self.decisions.clear();
        self.decisions.extend_from_slice(decisions);
        self.cursor = 0;
        self.calls = 0;
    }

    fn pop(&mut self) -> Result<DecisionArm, ResolverError> {
        let decision = self
            .decisions
            .get(self.cursor)
            .copied()
            .expect("resolver script exhausted");
        self.cursor += 1;
        self.calls += 1;
        Ok(decision)
    }
}

#[derive(Default)]
struct ResolverScripts {
    row: DecisionScript,
    same_label: DecisionScript,
    outer: DecisionScript,
    inner: DecisionScript,
    par_left: DecisionScript,
    par_right: DecisionScript,
    passive_offer: DecisionScript,
    deep_outer: DecisionScript,
    deep_middle: DecisionScript,
    deep_inner: DecisionScript,
}

impl ResolverScripts {
    fn new() -> Self {
        Self::default()
    }

    fn script_mut(&mut self, slot: ScriptSlot) -> &mut DecisionScript {
        match slot {
            ScriptSlot::Row => &mut self.row,
            ScriptSlot::SameLabel => &mut self.same_label,
            ScriptSlot::Outer => &mut self.outer,
            ScriptSlot::Inner => &mut self.inner,
            ScriptSlot::ParLeft => &mut self.par_left,
            ScriptSlot::ParRight => &mut self.par_right,
            ScriptSlot::PassiveOffer => &mut self.passive_offer,
            ScriptSlot::DeepOuter => &mut self.deep_outer,
            ScriptSlot::DeepMiddle => &mut self.deep_middle,
            ScriptSlot::DeepInner => &mut self.deep_inner,
        }
    }

    fn script(&self, slot: ScriptSlot) -> &DecisionScript {
        match slot {
            ScriptSlot::Row => &self.row,
            ScriptSlot::SameLabel => &self.same_label,
            ScriptSlot::Outer => &self.outer,
            ScriptSlot::Inner => &self.inner,
            ScriptSlot::ParLeft => &self.par_left,
            ScriptSlot::ParRight => &self.par_right,
            ScriptSlot::PassiveOffer => &self.passive_offer,
            ScriptSlot::DeepOuter => &self.deep_outer,
            ScriptSlot::DeepMiddle => &self.deep_middle,
            ScriptSlot::DeepInner => &self.deep_inner,
        }
    }
}

fn set_script(slot: ScriptSlot, decisions: &[DecisionArm]) {
    SCRIPTS.with(|scripts| scripts.borrow_mut().script_mut(slot).set(decisions));
}

fn calls(slot: ScriptSlot) -> usize {
    SCRIPTS.with(|scripts| scripts.borrow().script(slot).calls)
}

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

fn decide(slot: ScriptSlot) -> Result<DecisionArm, ResolverError> {
    SCRIPTS.with(|scripts| scripts.borrow_mut().script_mut(slot).pop())
}

fn decide_row(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::Row)
}

fn decide_same_label(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::SameLabel)
}

fn decide_outer(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::Outer)
}

fn decide_inner(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::Inner)
}

fn decide_par_left(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::ParLeft)
}

fn decide_par_right(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::ParRight)
}

fn decide_passive_offer(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::PassiveOffer)
}

fn decide_deep_outer(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::DeepOuter)
}

fn decide_deep_middle(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::DeepMiddle)
}

fn decide_deep_inner(_: &()) -> Result<DecisionArm, ResolverError> {
    decide(ScriptSlot::DeepInner)
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
            g::send::<0, 2, Msg<SAME_REQ, u32>>(),
        )
        .resolve::<SAME_LABEL_RESOLVER>()
        .roll(),
    )
}

fn rolled_nested_resolved_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<INNER_SAME_REQ, u32>>(),
        g::seq(
            g::send::<0, 2, Msg<INNER_SAME_REQ, u32>>(),
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
            g::send::<0, 2, Msg<MIRROR_INNER_SAME_REQ, u32>>(),
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

async fn roundtrip_01<const REQ: u8, const ACK: u8>(
    origin: &mut Endpoint<'static, 0>,
    peer: &mut Endpoint<'static, 1>,
    value: u32,
) {
    origin
        .send::<Msg<REQ, u32>>(&value)
        .await
        .expect("request send");
    assert_eq!(
        peer.recv::<Msg<REQ, u32>>().await.expect("request recv"),
        value
    );
    let ack = value + 10_000;
    peer.send::<Msg<ACK, u32>>(&ack).await.expect("ack send");
    assert_eq!(origin.recv::<Msg<ACK, u32>>().await.expect("ack recv"), ack);
}

async fn passive_offer_recv<const MSG: u8>(receiver: &mut Endpoint<'static, 0>, value: u32) {
    let branch = receiver
        .offer()
        .await
        .unwrap_or_else(|err| panic!("offer selected route arm for message {MSG}: {err:?}"));
    assert_eq!(branch.label(), <Msg<MSG, u32> as Message>::LOGICAL_LABEL);
    assert_eq!(
        branch
            .recv::<Msg<MSG, u32>>()
            .await
            .expect("passive branch recv"),
        value
    );
}

#[test]
fn rolled_resolved_route_reenters_left_right_left_rows() {
    set_script(
        ScriptSlot::Row,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_row_program::<0>();
            let role1 = rolled_resolved_row_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<ROW_RESOLVER>::decision_state(&UNIT, decide_row),
            )
            .expect("install row resolver");
            let sid = SessionId::new(0x0009_3001);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                roundtrip_01::<ROW_LEFT_REQ, ROW_LEFT_ACK>(&mut origin, &mut peer, 1).await;
                roundtrip_01::<ROW_RIGHT_REQ, ROW_RIGHT_ACK>(&mut origin, &mut peer, 2).await;
                roundtrip_01::<ROW_LEFT_REQ, ROW_LEFT_ACK>(&mut origin, &mut peer, 3).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Row), 3);
}

#[test]
fn rolled_resolved_same_label_reenters_and_targets_selected_peer_only() {
    set_script(
        ScriptSlot::SameLabel,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_same_label_target_program::<0>();
            let role1 = rolled_same_label_target_program::<1>();
            let role2 = rolled_same_label_target_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<SAME_LABEL_RESOLVER>::decision_state(&UNIT, decide_same_label),
            )
            .expect("install same-label resolver");
            let sid = SessionId::new(0x0009_3002);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<SAME_REQ, u32>>(&11)
                    .await
                    .expect("left selected same-label send");
                assert!(
                    right_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .await
                        .expect("left recv"),
                    11
                );

                origin
                    .send::<Msg<SAME_REQ, u32>>(&12)
                    .await
                    .expect("right selected same-label send");
                assert!(
                    left_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    right_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .await
                        .expect("right recv"),
                    12
                );

                origin
                    .send::<Msg<SAME_REQ, u32>>(&13)
                    .await
                    .expect("left selected same-label reentry");
                assert!(
                    right_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_REQ, u32>>()
                        .await
                        .expect("left recv"),
                    13
                );
            });
        });
    });
    assert_eq!(calls(ScriptSlot::SameLabel), 3);
}

#[test]
fn rolled_nested_resolved_route_reenters_asymmetric_paths() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    set_script(ScriptSlot::Inner, &[DecisionArm::Right, DecisionArm::Left]);
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_nested_resolved_program::<0>();
            let role1 = rolled_nested_resolved_program::<1>();
            let role2 = rolled_nested_resolved_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install inner resolver");
            let sid = SessionId::new(0x0009_3003);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");
            let mut alternate_peer = rv.enter(sid, &role2).expect("attach alternate peer");

            futures::executor::block_on(async {
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
                    .send::<Msg<INNER_SAME_REQ, u32>>(&22)
                    .await
                    .expect("inner right selected send");
                assert!(
                    peer.recv::<Msg<INNER_SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    alternate_peer
                        .recv::<Msg<INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner right selected recv"),
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
                    .expect("inner left selected send");
                assert!(
                    alternate_peer
                        .recv::<Msg<INNER_SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    peer.recv::<Msg<INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner left selected recv"),
                    25
                );
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 2);
}

#[test]
fn rolled_mirrored_nested_resolved_route_reenters_asymmetric_paths() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
    );
    set_script(ScriptSlot::Inner, &[DecisionArm::Right, DecisionArm::Left]);
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_mirrored_nested_resolved_program::<0>();
            let role1 = rolled_mirrored_nested_resolved_program::<1>();
            let role2 = rolled_mirrored_nested_resolved_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install inner resolver");
            let sid = SessionId::new(0x0009_3007);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");
            let mut alternate_peer = rv.enter(sid, &role2).expect("attach alternate peer");

            futures::executor::block_on(async {
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
                    .send::<Msg<MIRROR_INNER_SAME_REQ, u32>>(&62)
                    .await
                    .expect("inner right selected send");
                assert!(
                    peer.recv::<Msg<MIRROR_INNER_SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    alternate_peer
                        .recv::<Msg<MIRROR_INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner right selected recv"),
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
                    .expect("inner left selected send");
                assert!(
                    alternate_peer
                        .recv::<Msg<MIRROR_INNER_SAME_REQ, u32>>()
                        .now_or_never()
                        .is_none()
                );
                assert_eq!(
                    peer.recv::<Msg<MIRROR_INNER_SAME_REQ, u32>>()
                        .await
                        .expect("inner left selected recv"),
                    65
                );
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 2);
}

#[test]
fn rolled_parallel_resolved_routes_reenter_after_both_lanes_settle() {
    set_script(
        ScriptSlot::ParLeft,
        &[DecisionArm::Left, DecisionArm::Right],
    );
    set_script(
        ScriptSlot::ParRight,
        &[DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_parallel_resolved_program::<0>();
            let role1 = rolled_parallel_resolved_program::<1>();
            let role2 = rolled_parallel_resolved_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<PAR_LEFT_RESOLVER>::decision_state(&UNIT, decide_par_left),
            )
            .expect("install left parallel resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<PAR_RIGHT_RESOLVER>::decision_state(&UNIT, decide_par_right),
            )
            .expect("install right parallel resolver");
            let sid = SessionId::new(0x0009_3004);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            futures::executor::block_on(async {
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
            });
        });
    });
    assert_eq!(calls(ScriptSlot::ParLeft), 2);
    assert_eq!(calls(ScriptSlot::ParRight), 2);
}

#[test]
fn rolled_resolved_route_par_seq_route_roll_mixed_corpus_reenters_by_scope() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    set_script(
        ScriptSlot::Inner,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Right],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_mixed_left_arm_program::<0>();
            let role1 = rolled_resolved_mixed_left_arm_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install mixed outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install mixed inner resolver");
            let sid = SessionId::new(0x0009_3011);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                roundtrip_01::<MIXED_INNER_LEFT_REQ, MIXED_INNER_LEFT_ACK>(
                    &mut origin,
                    &mut peer,
                    101,
                )
                .await;
                roundtrip_01::<MIXED_INNER_RIGHT_REQ, MIXED_INNER_RIGHT_ACK>(
                    &mut origin,
                    &mut peer,
                    102,
                )
                .await;
                roundtrip_01::<MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK>(&mut origin, &mut peer, 103)
                    .await;
                roundtrip_01::<MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK>(
                    &mut origin,
                    &mut peer,
                    104,
                )
                .await;
                roundtrip_01::<MIXED_OUTER_RIGHT_REQ, MIXED_OUTER_RIGHT_ACK>(
                    &mut origin,
                    &mut peer,
                    105,
                )
                .await;
                roundtrip_01::<MIXED_INNER_RIGHT_REQ, MIXED_INNER_RIGHT_ACK>(
                    &mut origin,
                    &mut peer,
                    106,
                )
                .await;
                roundtrip_01::<MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK>(&mut origin, &mut peer, 107)
                    .await;
                roundtrip_01::<MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK>(
                    &mut origin,
                    &mut peer,
                    108,
                )
                .await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 3);
}

#[test]
fn rolled_resolved_route_par_seq_route_roll_mixed_right_corpus_reenters_by_scope() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
    );
    set_script(
        ScriptSlot::Inner,
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = rolled_resolved_mixed_right_arm_program::<0>();
            let role1 = rolled_resolved_mixed_right_arm_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install mixed right outer resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install mixed right inner resolver");
            let sid = SessionId::new(0x0009_3012);
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                roundtrip_01::<MIXED_INNER_RIGHT_REQ, MIXED_INNER_RIGHT_ACK>(
                    &mut origin,
                    &mut peer,
                    201,
                )
                .await;
                roundtrip_01::<MIXED_INNER_LEFT_REQ, MIXED_INNER_LEFT_ACK>(
                    &mut origin,
                    &mut peer,
                    202,
                )
                .await;
                roundtrip_01::<MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK>(&mut origin, &mut peer, 203)
                    .await;
                roundtrip_01::<MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK>(
                    &mut origin,
                    &mut peer,
                    204,
                )
                .await;
                roundtrip_01::<MIXED_OUTER_LEFT_REQ, MIXED_OUTER_LEFT_ACK>(
                    &mut origin,
                    &mut peer,
                    205,
                )
                .await;
                roundtrip_01::<MIXED_INNER_LEFT_REQ, MIXED_INNER_LEFT_ACK>(
                    &mut origin,
                    &mut peer,
                    206,
                )
                .await;
                roundtrip_01::<MIXED_SEQ_TAIL_REQ, MIXED_SEQ_TAIL_ACK>(&mut origin, &mut peer, 207)
                    .await;
                roundtrip_01::<MIXED_PAR_SIBLING_REQ, MIXED_PAR_SIBLING_ACK>(
                    &mut origin,
                    &mut peer,
                    208,
                )
                .await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 3);
}

#[test]
fn rolled_resolved_route_reenters_passive_offer_left_right_left() {
    set_script(
        ScriptSlot::PassiveOffer,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_passive_offer_program::<0>();
            let sender_program = rolled_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<PASSIVE_OFFER_RESOLVER>::decision_state(&UNIT, decide_passive_offer),
            )
            .expect("install passive-offer resolver");
            let sid = SessionId::new(0x0009_3005);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            futures::executor::block_on(async {
                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&41)
                    .await
                    .expect("left selected send");
                passive_offer_recv::<PASSIVE_OFFER_LEFT>(&mut receiver, 41).await;

                sender
                    .send::<Msg<PASSIVE_OFFER_RIGHT, u32>>(&42)
                    .await
                    .expect("right selected send");
                passive_offer_recv::<PASSIVE_OFFER_RIGHT>(&mut receiver, 42).await;

                sender
                    .send::<Msg<PASSIVE_OFFER_LEFT, u32>>(&43)
                    .await
                    .expect("left selected reentry send");
                passive_offer_recv::<PASSIVE_OFFER_LEFT>(&mut receiver, 43).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::PassiveOffer), 3);
}

#[test]
fn rolled_nested_resolved_route_reenters_passive_offer_asymmetric_paths() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    set_script(ScriptSlot::Inner, &[DecisionArm::Right, DecisionArm::Left]);
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_nested_passive_offer_program::<0>();
            let sender_program = rolled_nested_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install nested passive outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install nested passive inner resolver");
            let sid = SessionId::new(0x0009_3006);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            futures::executor::block_on(async {
                sender
                    .send::<Msg<PASSIVE_NESTED_PREFIX, u32>>(&51)
                    .await
                    .expect("outer left prefix send");
                passive_offer_recv::<PASSIVE_NESTED_PREFIX>(&mut receiver, 51).await;
                sender
                    .send::<Msg<PASSIVE_NESTED_INNER_RIGHT, u32>>(&52)
                    .await
                    .expect("inner right selected send");
                passive_offer_recv::<PASSIVE_NESTED_INNER_RIGHT>(&mut receiver, 52).await;

                sender
                    .send::<Msg<PASSIVE_NESTED_OUTER_RIGHT, u32>>(&53)
                    .await
                    .expect("outer right selected send");
                passive_offer_recv::<PASSIVE_NESTED_OUTER_RIGHT>(&mut receiver, 53).await;

                sender
                    .send::<Msg<PASSIVE_NESTED_PREFIX, u32>>(&54)
                    .await
                    .expect("outer left reentry prefix send");
                passive_offer_recv::<PASSIVE_NESTED_PREFIX>(&mut receiver, 54).await;
                sender
                    .send::<Msg<PASSIVE_NESTED_INNER_LEFT, u32>>(&55)
                    .await
                    .expect("inner left selected send");
                passive_offer_recv::<PASSIVE_NESTED_INNER_LEFT>(&mut receiver, 55).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 2);
}

#[test]
fn rolled_mirrored_nested_resolved_route_reenters_passive_offer_asymmetric_paths() {
    set_script(
        ScriptSlot::Outer,
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
    );
    set_script(ScriptSlot::Inner, &[DecisionArm::Right, DecisionArm::Left]);
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_mirrored_nested_passive_offer_program::<0>();
            let sender_program = rolled_mirrored_nested_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<OUTER_RESOLVER>::decision_state(&UNIT, decide_outer),
            )
            .expect("install nested passive outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<INNER_RESOLVER>::decision_state(&UNIT, decide_inner),
            )
            .expect("install nested passive inner resolver");
            let sid = SessionId::new(0x0009_3008);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            futures::executor::block_on(async {
                sender
                    .send::<Msg<PASSIVE_MIRROR_PREFIX, u32>>(&71)
                    .await
                    .expect("outer right prefix send");
                passive_offer_recv::<PASSIVE_MIRROR_PREFIX>(&mut receiver, 71).await;
                sender
                    .send::<Msg<PASSIVE_MIRROR_INNER_RIGHT, u32>>(&72)
                    .await
                    .expect("inner right selected send");
                passive_offer_recv::<PASSIVE_MIRROR_INNER_RIGHT>(&mut receiver, 72).await;

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
                    Some(0),
                    "outer-left reentry route selection must publish on the offer lane: {events:?}"
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
                passive_offer_recv::<PASSIVE_MIRROR_OUTER_LEFT>(&mut receiver, 73).await;

                sender
                    .send::<Msg<PASSIVE_MIRROR_PREFIX, u32>>(&74)
                    .await
                    .expect("outer right reentry prefix send");
                passive_offer_recv::<PASSIVE_MIRROR_PREFIX>(&mut receiver, 74).await;
                sender
                    .send::<Msg<PASSIVE_MIRROR_INNER_LEFT, u32>>(&75)
                    .await
                    .expect("inner left selected send");
                passive_offer_recv::<PASSIVE_MIRROR_INNER_LEFT>(&mut receiver, 75).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::Outer), 3);
    assert_eq!(calls(ScriptSlot::Inner), 2);
}

#[test]
fn rolled_deep_right_spine_passive_offer_reenters_across_all_depths() {
    set_script(
        ScriptSlot::DeepOuter,
        &[
            DecisionArm::Right,
            DecisionArm::Left,
            DecisionArm::Right,
            DecisionArm::Right,
        ],
    );
    set_script(
        ScriptSlot::DeepMiddle,
        &[DecisionArm::Right, DecisionArm::Left, DecisionArm::Right],
    );
    set_script(
        ScriptSlot::DeepInner,
        &[DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_deep_right_spine_passive_offer_program::<0>();
            let sender_program = rolled_deep_right_spine_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_OUTER_RESOLVER>::decision_state(&UNIT, decide_deep_outer),
            )
            .expect("install deep outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_MIDDLE_RESOLVER>::decision_state(&UNIT, decide_deep_middle),
            )
            .expect("install deep middle resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_INNER_RESOLVER>::decision_state(&UNIT, decide_deep_inner),
            )
            .expect("install deep inner resolver");
            let sid = SessionId::new(0x0009_3009);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            futures::executor::block_on(async {
                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&81)
                    .await
                    .expect("deep right outer prefix send");
                passive_offer_recv::<DEEP_RIGHT_OUTER_PREFIX>(&mut receiver, 81).await;
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>(&82)
                    .await
                    .expect("deep right middle prefix send");
                passive_offer_recv::<DEEP_RIGHT_MIDDLE_PREFIX>(&mut receiver, 82).await;
                sender
                    .send::<Msg<DEEP_RIGHT_INNER_RIGHT, u32>>(&83)
                    .await
                    .expect("deep right inner right send");
                passive_offer_recv::<DEEP_RIGHT_INNER_RIGHT>(&mut receiver, 83).await;

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_LEFT, u32>>(&84)
                    .await
                    .expect("deep outer left reentry send");
                passive_offer_recv::<DEEP_RIGHT_OUTER_LEFT>(&mut receiver, 84).await;

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&85)
                    .await
                    .expect("deep right outer prefix reentry send");
                passive_offer_recv::<DEEP_RIGHT_OUTER_PREFIX>(&mut receiver, 85).await;
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_LEFT, u32>>(&86)
                    .await
                    .expect("deep middle left reentry send");
                passive_offer_recv::<DEEP_RIGHT_MIDDLE_LEFT>(&mut receiver, 86).await;

                sender
                    .send::<Msg<DEEP_RIGHT_OUTER_PREFIX, u32>>(&87)
                    .await
                    .expect("deep right outer prefix second reentry send");
                passive_offer_recv::<DEEP_RIGHT_OUTER_PREFIX>(&mut receiver, 87).await;
                sender
                    .send::<Msg<DEEP_RIGHT_MIDDLE_PREFIX, u32>>(&88)
                    .await
                    .expect("deep right middle prefix second reentry send");
                passive_offer_recv::<DEEP_RIGHT_MIDDLE_PREFIX>(&mut receiver, 88).await;
                sender
                    .send::<Msg<DEEP_RIGHT_INNER_LEFT, u32>>(&89)
                    .await
                    .expect("deep inner left reentry send");
                passive_offer_recv::<DEEP_RIGHT_INNER_LEFT>(&mut receiver, 89).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::DeepOuter), 4);
    assert_eq!(calls(ScriptSlot::DeepMiddle), 3);
    assert_eq!(calls(ScriptSlot::DeepInner), 2);
}

#[test]
fn rolled_deep_left_spine_passive_offer_reenters_across_all_depths() {
    set_script(
        ScriptSlot::DeepOuter,
        &[
            DecisionArm::Left,
            DecisionArm::Right,
            DecisionArm::Left,
            DecisionArm::Left,
        ],
    );
    set_script(
        ScriptSlot::DeepMiddle,
        &[DecisionArm::Left, DecisionArm::Right, DecisionArm::Left],
    );
    set_script(
        ScriptSlot::DeepInner,
        &[DecisionArm::Right, DecisionArm::Left],
    );
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let receiver_program = rolled_deep_left_spine_passive_offer_program::<0>();
            let sender_program = rolled_deep_left_spine_passive_offer_program::<1>();
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_OUTER_RESOLVER>::decision_state(&UNIT, decide_deep_outer),
            )
            .expect("install deep outer resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_MIDDLE_RESOLVER>::decision_state(&UNIT, decide_deep_middle),
            )
            .expect("install deep middle resolver");
            rv.set_resolver(
                &sender_program,
                ResolverRef::<DEEP_INNER_RESOLVER>::decision_state(&UNIT, decide_deep_inner),
            )
            .expect("install deep inner resolver");
            let sid = SessionId::new(0x0009_3010);
            let mut receiver = rv.enter(sid, &receiver_program).expect("attach receiver");
            let mut sender = rv.enter(sid, &sender_program).expect("attach sender");

            futures::executor::block_on(async {
                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&91)
                    .await
                    .expect("deep left outer prefix send");
                passive_offer_recv::<DEEP_LEFT_OUTER_PREFIX>(&mut receiver, 91).await;
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>(&92)
                    .await
                    .expect("deep left middle prefix send");
                passive_offer_recv::<DEEP_LEFT_MIDDLE_PREFIX>(&mut receiver, 92).await;
                sender
                    .send::<Msg<DEEP_LEFT_INNER_RIGHT, u32>>(&93)
                    .await
                    .expect("deep left inner right send");
                passive_offer_recv::<DEEP_LEFT_INNER_RIGHT>(&mut receiver, 93).await;

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_RIGHT, u32>>(&94)
                    .await
                    .expect("deep outer right reentry send");
                passive_offer_recv::<DEEP_LEFT_OUTER_RIGHT>(&mut receiver, 94).await;

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&95)
                    .await
                    .expect("deep left outer prefix reentry send");
                passive_offer_recv::<DEEP_LEFT_OUTER_PREFIX>(&mut receiver, 95).await;
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_RIGHT, u32>>(&96)
                    .await
                    .expect("deep middle right reentry send");
                passive_offer_recv::<DEEP_LEFT_MIDDLE_RIGHT>(&mut receiver, 96).await;

                sender
                    .send::<Msg<DEEP_LEFT_OUTER_PREFIX, u32>>(&97)
                    .await
                    .expect("deep left outer prefix second reentry send");
                passive_offer_recv::<DEEP_LEFT_OUTER_PREFIX>(&mut receiver, 97).await;
                sender
                    .send::<Msg<DEEP_LEFT_MIDDLE_PREFIX, u32>>(&98)
                    .await
                    .expect("deep left middle prefix second reentry send");
                passive_offer_recv::<DEEP_LEFT_MIDDLE_PREFIX>(&mut receiver, 98).await;
                sender
                    .send::<Msg<DEEP_LEFT_INNER_LEFT, u32>>(&99)
                    .await
                    .expect("deep inner left reentry send");
                passive_offer_recv::<DEEP_LEFT_INNER_LEFT>(&mut receiver, 99).await;
            });
        });
    });
    assert_eq!(calls(ScriptSlot::DeepOuter), 4);
    assert_eq!(calls(ScriptSlot::DeepMiddle), 3);
    assert_eq!(calls(ScriptSlot::DeepInner), 2);
}
