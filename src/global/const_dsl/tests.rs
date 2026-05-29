use core::mem::size_of;

use super::{
    CompactScopeId, ControlMarker, ControlScopeKind, EffList, PolicyMode, ScopeEvent, ScopeId,
    ScopeKind,
};
use crate::eff::{EffAtom, EffKind, EffStruct};
use crate::g;
use crate::integration::cap::control::{LoopBreakKind, LoopContinueKind};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;
const LOOP_POLICY_ID: u16 = 120;
type LoopContinueHead = g::Policy<
    g::Send<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>>,
    LOOP_POLICY_ID,
>;
type LoopBreakHead = g::Policy<
    g::Send<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>>,
    LOOP_POLICY_ID,
>;
type LoopContinueProgram =
    g::Seq<LoopContinueHead, g::Send<g::Role<0>, g::Role<1>, g::Msg<1, u32>>>;
type LoopDecisionProgram = g::Route<LoopContinueProgram, LoopBreakHead>;

const fn atom(label: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 0,
        to: 1,
        label,
        is_control: false,
        resource: None,
        lane: 0,
    })
}

const fn segment_boundary_list() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= crate::eff::meta::MAX_SEGMENT_EFFS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list
}

static SEGMENT_BOUNDARY_LIST: EffList = segment_boundary_list();

const fn scope_exit_at_exact_segment_boundary_list() -> EffList {
    let route_scope = ScopeId::new(ScopeKind::Route, 1);
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < crate::eff::meta::MAX_SEGMENT_EFFS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list.push_scope_marker_full(
        crate::eff::meta::MAX_SEGMENT_EFFS,
        route_scope,
        ScopeKind::Route,
        ScopeEvent::Exit,
        false,
        Some(0),
    )
}

static SCOPE_EXIT_AT_EXACT_SEGMENT_BOUNDARY_LIST: EffList =
    scope_exit_at_exact_segment_boundary_list();

const fn scope_enter_at_exact_segment_boundary_list() -> EffList {
    let route_scope = ScopeId::new(ScopeKind::Route, 2);
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < crate::eff::meta::MAX_SEGMENT_EFFS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list = list.push_scope_marker_full(
        crate::eff::meta::MAX_SEGMENT_EFFS,
        route_scope,
        ScopeKind::Route,
        ScopeEvent::Enter,
        false,
        Some(0),
    );
    list.push(atom(0xaa))
}

static SCOPE_ENTER_AT_EXACT_SEGMENT_BOUNDARY_LIST: EffList =
    scope_enter_at_exact_segment_boundary_list();

const fn control_spec_at_segment_boundary_list() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= crate::eff::meta::MAX_SEGMENT_EFFS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list.push_control_spec(
        crate::eff::meta::MAX_SEGMENT_EFFS,
        crate::global::StaticControlDesc::of::<LoopContinueKind>(),
    )
    .push_control_marker(
        crate::eff::meta::MAX_SEGMENT_EFFS,
        ControlScopeKind::Route,
        9,
    )
    .push_policy(crate::eff::meta::MAX_SEGMENT_EFFS, PolicyMode::dynamic(44))
}

static CONTROL_SPEC_AT_SEGMENT_BOUNDARY_LIST: EffList = control_spec_at_segment_boundary_list();

const fn segment_metadata_list() -> EffList {
    let route_scope = ScopeId::new(ScopeKind::Route, 0);
    segment_boundary_list()
        .push_scope_marker_full(
            0,
            route_scope,
            ScopeKind::Route,
            ScopeEvent::Enter,
            false,
            Some(0),
        )
        .push_control_marker(
            crate::eff::meta::MAX_SEGMENT_EFFS,
            ControlScopeKind::Route,
            7,
        )
        .push_policy(crate::eff::meta::MAX_SEGMENT_EFFS, PolicyMode::dynamic(33))
}

static SEGMENT_METADATA_LIST: EffList = segment_metadata_list();

const fn over_single_descriptor_cap_list() -> EffList {
    const SINGLE_DESCRIPTOR_CAP: usize = 256;
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= SINGLE_DESCRIPTOR_CAP {
        list = list.push(atom(1));
        idx += 1;
    }
    list
}

static OVER_SINGLE_DESCRIPTOR_CAP_LIST: EffList = over_single_descriptor_cap_list();

#[test]
fn control_marker_stays_compact() {
    assert!(
        size_of::<ControlMarker>() <= 8,
        "ControlMarker regressed to a wide offset layout: {} bytes",
        size_of::<ControlMarker>()
    );
}

#[test]
fn compact_scope_id_roundtrips_scope_id() {
    let scope = ScopeId::compose(ScopeKind::Route, 256, 255, 254);
    let compact = CompactScopeId::from_scope_id(scope);
    assert_eq!(compact.to_scope_id(), scope);
    assert_eq!(CompactScopeId::none().to_scope_id(), ScopeId::none());
    assert!(
        size_of::<CompactScopeId>() <= 4,
        "CompactScopeId regressed beyond its packed u32 storage: {} bytes",
        size_of::<CompactScopeId>()
    );
}

#[test]
fn eff_list_crosses_segment_boundary_without_public_dsl_change() {
    let list = &SEGMENT_BOUNDARY_LIST;

    assert_eq!(list.len(), crate::eff::meta::MAX_SEGMENT_EFFS + 1);
    assert_eq!(list.segment_count(), 2);
    assert!(matches!(list.node_at(0).kind, EffKind::Atom));
    assert_eq!(
        list.node_at(crate::eff::meta::MAX_SEGMENT_EFFS)
            .atom_data()
            .label,
        crate::eff::meta::MAX_SEGMENT_EFFS as u8
    );
}

#[test]
fn eff_list_segment_summaries_track_metadata_by_segment() {
    let list = &SEGMENT_METADATA_LIST;

    let first = list.segment_summary(0);
    let second = list.segment_summary(1);
    assert_eq!(first.eff_len(), crate::eff::meta::MAX_SEGMENT_EFFS);
    assert_eq!(first.scope_marker_len(), 1);
    assert_eq!(first.route_scope_enter_len(), 1);
    assert_eq!(second.eff_len(), 1);
    assert_eq!(second.control_marker_len(), 1);
    assert_eq!(second.policy_marker_len(), 1);
    assert_eq!(second.control_spec_len(), 0);
}

#[test]
fn scope_exit_at_exact_segment_boundary_belongs_to_previous_segment() {
    let list = &SCOPE_EXIT_AT_EXACT_SEGMENT_BOUNDARY_LIST;

    assert_eq!(list.len(), crate::eff::meta::MAX_SEGMENT_EFFS);
    assert_eq!(list.segment_count(), 1);
    let first = list.segment_summary(0);
    assert_eq!(first.eff_len(), crate::eff::meta::MAX_SEGMENT_EFFS);
    assert_eq!(first.scope_marker_len(), 1);
    assert_eq!(first.route_scope_enter_len(), 0);
}

#[test]
fn scope_enter_at_exact_segment_boundary_belongs_to_next_segment() {
    let list = &SCOPE_ENTER_AT_EXACT_SEGMENT_BOUNDARY_LIST;

    assert_eq!(list.len(), crate::eff::meta::MAX_SEGMENT_EFFS + 1);
    assert_eq!(list.segment_count(), 2);
    let first = list.segment_summary(0);
    let second = list.segment_summary(1);
    assert_eq!(first.eff_len(), crate::eff::meta::MAX_SEGMENT_EFFS);
    assert_eq!(first.scope_marker_len(), 0);
    assert_eq!(second.eff_len(), 1);
    assert_eq!(second.scope_marker_len(), 1);
    assert_eq!(second.route_scope_enter_len(), 1);
}

#[test]
fn control_spec_at_segment_boundary_belongs_to_effect_segment() {
    let list = &CONTROL_SPEC_AT_SEGMENT_BOUNDARY_LIST;

    assert_eq!(list.len(), crate::eff::meta::MAX_SEGMENT_EFFS + 1);
    assert_eq!(list.segment_count(), 2);
    let first = list.segment_summary(0);
    let second = list.segment_summary(1);
    assert_eq!(first.eff_len(), crate::eff::meta::MAX_SEGMENT_EFFS);
    assert_eq!(first.control_marker_len(), 0);
    assert_eq!(first.policy_marker_len(), 0);
    assert_eq!(first.control_spec_len(), 0);
    assert_eq!(second.eff_len(), 1);
    assert_eq!(second.control_marker_len(), 1);
    assert_eq!(second.policy_marker_len(), 1);
    assert_eq!(second.control_spec_len(), 1);
}

#[test]
fn eff_list_with_more_than_256_effects_keeps_segment_summaries() {
    const SINGLE_DESCRIPTOR_CAP: usize = 256;
    let list = &OVER_SINGLE_DESCRIPTOR_CAP_LIST;

    assert_eq!(list.len(), SINGLE_DESCRIPTOR_CAP + 1);
    assert!(list.segment_count() > 1);
    assert_eq!(
        list.segment_summary(0).eff_len(),
        crate::eff::meta::MAX_SEGMENT_EFFS,
    );
    assert!(
        crate::eff::EffIndex::from_dense_ordinal(SINGLE_DESCRIPTOR_CAP).segment() > 0,
        "effect 256 must be represented as a segmented descriptor position",
    );
}

#[test]
#[should_panic(expected = "EffList marker offset out of bounds")]
fn segment_marker_offset_rejects_over_capacity_marker() {
    let _ = EffList::summary_segment_for_scope_marker_offset(
        crate::eff::meta::MAX_EFF_NODES + 1,
        crate::eff::meta::MAX_EFF_NODES,
        ScopeEvent::Enter,
    );
}

#[test]
#[should_panic(expected = "EffList effect marker offset out of bounds")]
fn segment_effect_offset_rejects_capacity_marker() {
    let _ = EffList::summary_segment_for_effect_indexed_offset(crate::eff::meta::MAX_EFF_NODES);
}

fn loop_body() -> g::Program<g::Send<g::Role<0>, g::Role<1>, g::Msg<1, u32>>> {
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>()
}
fn loop_break_arm() -> g::Program<
    g::Policy<
        g::Send<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>>,
        LOOP_POLICY_ID,
    >,
> {
    g::send::<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
        .policy::<LOOP_POLICY_ID>()
}
fn loop_continue_arm() -> g::Program<
    g::Seq<
        g::Policy<
            g::Send<
                g::Role<0>,
                g::Role<0>,
                g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
            >,
            LOOP_POLICY_ID,
        >,
        g::Send<g::Role<0>, g::Role<1>, g::Msg<1, u32>>,
    >,
> {
    g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_body(),
    )
}
fn loop_decision() -> g::Program<
    g::Route<
        g::Seq<
            g::Policy<
                g::Send<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
                >,
                LOOP_POLICY_ID,
            >,
            g::Send<g::Role<0>, g::Role<1>, g::Msg<1, u32>>,
        >,
        g::Policy<
            g::Send<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>>,
            LOOP_POLICY_ID,
        >,
    >,
> {
    g::route(loop_continue_arm(), loop_break_arm())
}

#[test]
fn policy_scope_stays_internal() {
    let _ = loop_decision().program_image();
    let list: &EffList = <LoopDecisionProgram as crate::g::Choreography>::SOURCE.eff_list();
    let mut policies = 0usize;
    let mut offset = 0usize;
    while offset < list.len() {
        if list.policy_at(offset).is_some() {
            policies += 1;
            let (_, scope) = list
                .policy_with_scope(offset)
                .expect("policy scope should be derivable");
            assert!(!scope.is_none(), "loop policy should expose a scope id");
            assert_eq!(scope.kind(), ScopeKind::Route, "loop scope kind matches");
        }
        offset += 1;
    }
    assert!(
        policies >= 2,
        "loop continue/break policies should be present"
    );
}
