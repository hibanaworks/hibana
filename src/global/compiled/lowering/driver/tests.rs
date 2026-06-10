#![allow(long_running_const_eval)]

use crate::control::cap::mint::LocalControlKind;
use crate::eff::{EffAtom, EffStruct};
use crate::global::StaticControlDesc;
use crate::global::const_dsl::{
    ControlScopeKind, EffList, ResolverMode, ScopeEvent, ScopeId, ScopeKind,
};

struct TestLoopContinueControl;

impl LocalControlKind for TestLoopContinueControl {
    const TAG: u8 = 0x4E;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = crate::observe::ids::LOOP_DECISION;
    const SHOT: crate::control::cap::mint::CapShot = crate::control::cap::mint::CapShot::One;
    const OP: crate::control::cap::mint::ControlOp =
        crate::control::cap::mint::ControlOp::LoopContinue;

    fn encode_local_handle(
        _sid: crate::control::types::SessionId,
        _lane: crate::control::types::Lane,
        _scope: ScopeId,
    ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN] {
        [0; crate::control::cap::mint::CAP_HANDLE_LEN]
    }
}

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

const fn inbound_atom(label: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 1,
        to: 0,
        label,
        is_control: false,
        resource: None,
        lane: 0,
    })
}

const fn prefix_at_segment_boundary() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < crate::eff::meta::MAX_SEGMENT_EFFS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list
}

const fn scoped_suffix() -> EffList {
    EffList::new()
        .push(atom(0xaa))
        .with_scope(ScopeId::new(ScopeKind::Route, 9))
}

const fn scope_enter_at_boundary_program() -> EffList {
    prefix_at_segment_boundary().extend_list(scoped_suffix())
}

const fn scope_exit_at_boundary_program() -> EffList {
    prefix_at_segment_boundary().with_scope(ScopeId::new(ScopeKind::Route, 10))
}

const fn control_spec_at_boundary_program() -> EffList {
    let suffix = EffList::new()
        .push(control_atom(0xbb))
        .push_control_spec(0, StaticControlDesc::of_local::<TestLoopContinueControl>())
        .push_control_marker(0, ControlScopeKind::Loop, 77)
        .push_policy(0, ResolverMode::dynamic(77))
        .with_scope(ScopeId::new(ScopeKind::Route, 77));
    prefix_at_segment_boundary().extend_list(suffix)
}

const fn atom_heavy_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= super::MAX_COMPILED_PROGRAM_TAP_EVENTS {
        list = list.push(atom(idx as u8));
        idx += 1;
    }
    list
}

const SIDE_TABLE_CAPACITY_REGRESSION_ROWS: usize = (crate::eff::meta::MAX_SEGMENTS * 2) + 2;

const fn control_atom(label: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 0,
        to: 0,
        label,
        is_control: true,
        resource: Some(<TestLoopContinueControl as LocalControlKind>::TAG),
        lane: 0,
    })
}

const fn policy_side_table_regression_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < SIDE_TABLE_CAPACITY_REGRESSION_ROWS {
        let left = EffList::new()
            .push(control_atom(idx as u8))
            .push_control_spec(0, StaticControlDesc::of_local::<TestLoopContinueControl>())
            .push_control_marker(0, ControlScopeKind::Loop, idx as u16)
            .push_policy(0, ResolverMode::dynamic(7))
            .with_scope(ScopeId::new(ScopeKind::Route, idx as u16));
        let right = EffList::new()
            .push(control_atom((idx + 1) as u8))
            .push_control_spec(0, StaticControlDesc::of_local::<TestLoopContinueControl>())
            .push_control_marker(0, ControlScopeKind::Loop, idx as u16)
            .push_policy(0, ResolverMode::dynamic(7))
            .with_scope(ScopeId::new(ScopeKind::Route, idx as u16));
        list = list.extend_list(left).extend_list(right);
        idx += 2;
    }
    list
}

const fn route_scope(scope: u16, left: EffList, right: EffList) -> EffList {
    let scope_id = ScopeId::new(ScopeKind::Route, scope);
    left.with_scope(scope_id)
        .extend_list(right.with_scope(scope_id))
}

const fn passive_first_recv_dispatch_overflow_program() -> EffList {
    let mut tree = EffList::new().push(inbound_atom(0));
    let mut idx = 1usize;
    while idx <= crate::global::typestate::MAX_FIRST_RECV_DISPATCH {
        let right = EffList::new().push(inbound_atom(idx as u8));
        tree = route_scope(idx as u16, tree, right);
        idx += 1;
    }
    tree
}

const fn control_side_table_regression_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < SIDE_TABLE_CAPACITY_REGRESSION_ROWS {
        list = list.push(control_atom(idx as u8));
        list = list.push_control_spec(
            idx,
            StaticControlDesc::of_local::<TestLoopContinueControl>(),
        );
        list = list.push_control_marker(idx, ControlScopeKind::Loop, idx as u16);
        idx += 1;
    }
    list
}

static SCOPE_ENTER_AT_BOUNDARY: EffList = scope_enter_at_boundary_program();
static SCOPE_EXIT_AT_BOUNDARY: EffList = scope_exit_at_boundary_program();
static CONTROL_SPEC_AT_BOUNDARY: EffList = control_spec_at_boundary_program();
static ATOM_HEAVY_PROGRAM: EffList = atom_heavy_program();
static POLICY_SIDE_TABLE_REGRESSION_PROGRAM: EffList = policy_side_table_regression_program();
static PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_PROGRAM: EffList =
    passive_first_recv_dispatch_overflow_program();
static CONTROL_SIDE_TABLE_REGRESSION_PROGRAM: EffList = control_side_table_regression_program();

static SCOPE_ENTER_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&SCOPE_ENTER_AT_BOUNDARY);
static SCOPE_EXIT_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&SCOPE_EXIT_AT_BOUNDARY);
static CONTROL_SPEC_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&CONTROL_SPEC_AT_BOUNDARY);
static ATOM_HEAVY_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&ATOM_HEAVY_PROGRAM);
static POLICY_SIDE_TABLE_REGRESSION_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&POLICY_SIDE_TABLE_REGRESSION_PROGRAM);
static PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_PROGRAM);
static CONTROL_SIDE_TABLE_REGRESSION_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const(&CONTROL_SIDE_TABLE_REGRESSION_PROGRAM);

fn route_scope_enter_len(summary: &super::CompiledProgramImage, segment: usize) -> usize {
    let segment = &summary.validation.segments[segment];
    let start = segment.scope_marker_start as usize;
    let end = start + segment.scope_marker_len as usize;
    summary.validation.scope_markers[start..end]
        .iter()
        .filter(|marker| {
            matches!(marker.scope_kind, ScopeKind::Route)
                && matches!(marker.event, ScopeEvent::Enter)
        })
        .count()
}

fn assert_all_roles_project(summary: &super::CompiledProgramImage, eff_list: &EffList) {
    if let Some(error) =
        crate::global::compiled::lowering::seal::projection_error_all_roles(summary, eff_list)
    {
        crate::g::panic_choreography_error(error);
    }
}

#[test]
fn ordinary_atom_capacity_is_not_tied_to_tap_event_budget() {
    assert!(ATOM_HEAVY_PROGRAM.len() > super::MAX_COMPILED_PROGRAM_TAP_EVENTS);
    ATOM_HEAVY_SUMMARY.validate_projection_program();
    assert_all_roles_project(&ATOM_HEAVY_SUMMARY, &ATOM_HEAVY_PROGRAM);
    let view = ATOM_HEAVY_SUMMARY.view();
    let offset = super::MAX_COMPILED_PROGRAM_TAP_EVENTS;
    assert_eq!(
        view.atom_at(offset).map(|atom| atom.label),
        Some(offset as u8)
    );
}

#[test]
fn policy_side_table_capacity_matches_0_6_0_program_capacity() {
    assert!(SIDE_TABLE_CAPACITY_REGRESSION_ROWS > crate::eff::meta::MAX_SEGMENTS * 2);
    POLICY_SIDE_TABLE_REGRESSION_SUMMARY.validate_projection_program();
    assert_all_roles_project(
        &POLICY_SIDE_TABLE_REGRESSION_SUMMARY,
        &POLICY_SIDE_TABLE_REGRESSION_PROGRAM,
    );

    let last = SIDE_TABLE_CAPACITY_REGRESSION_ROWS - 1;
    let view = POLICY_SIDE_TABLE_REGRESSION_SUMMARY.view();
    assert_eq!(
        view.resident_policy_at(last)
            .and_then(|policy| policy.dynamic_policy_id()),
        Some(7)
    );
}

#[test]
fn control_side_tables_keep_0_6_0_program_capacity() {
    assert!(SIDE_TABLE_CAPACITY_REGRESSION_ROWS > crate::eff::meta::MAX_SEGMENTS * 2);
    CONTROL_SIDE_TABLE_REGRESSION_SUMMARY.validate_projection_program();
    assert_all_roles_project(
        &CONTROL_SIDE_TABLE_REGRESSION_SUMMARY,
        &CONTROL_SIDE_TABLE_REGRESSION_PROGRAM,
    );

    let last = SIDE_TABLE_CAPACITY_REGRESSION_ROWS - 1;
    let view = CONTROL_SIDE_TABLE_REGRESSION_SUMMARY.view();
    assert!(view.resident_control_desc_at(last).is_some());
    assert_eq!(
        CONTROL_SIDE_TABLE_REGRESSION_SUMMARY
            .program
            .compiled_program_counts
            .controls,
        SIDE_TABLE_CAPACITY_REGRESSION_ROWS
    );
    assert_eq!(
        CONTROL_SIDE_TABLE_REGRESSION_SUMMARY
            .program
            .control_marker_len,
        crate::eff::meta::MAX_SEGMENTS * 2
    );
}

#[test]
fn passive_first_recv_dispatch_capacity_is_projection_sealed() {
    PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_SUMMARY.validate_projection_program();
    assert!(matches!(
        crate::global::compiled::lowering::seal::projection_error_all_roles(
            &PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_SUMMARY,
            &PASSIVE_FIRST_RECV_DISPATCH_OVERFLOW_PROGRAM,
        ),
        Some(crate::g::ProgramSourceError::ProjectionRouteUnprojectable)
    ));
}

#[test]
fn lowering_scope_enter_at_exact_segment_boundary_belongs_to_next_segment() {
    let summary = &SCOPE_ENTER_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.validation.segments[0].scope_marker_len, 0);
    assert_eq!(summary.validation.segments[1].scope_marker_len, 2);
    assert_eq!(route_scope_enter_len(summary, 1), 1);
    assert_eq!(summary.validation.segments[1].scope_marker_start, 0);
    assert_eq!(summary.validation.segments[1].scope_marker_len, 2);
}

#[test]
fn lowering_scope_exit_at_exact_segment_boundary_belongs_to_previous_segment() {
    let summary = &SCOPE_EXIT_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.validation.segments[0].scope_marker_len, 2);
    assert_eq!(route_scope_enter_len(summary, 0), 1);
    assert_eq!(summary.validation.segments[1].scope_marker_len, 0);
    assert_eq!(summary.validation.segments[0].scope_marker_start, 0);
    assert_eq!(summary.validation.segments[0].scope_marker_len, 2);
}

#[test]
fn lowering_control_spec_at_segment_boundary_belongs_to_effect_segment() {
    let summary = &CONTROL_SPEC_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.validation.segments[0].control_marker_len, 0);
    assert_eq!(summary.validation.segments[0].policy_row_len, 0);
    assert_eq!(summary.validation.segments[0].control_desc_row_len, 0);
    assert_eq!(summary.validation.segments[1].control_marker_len, 1);
    assert_eq!(summary.validation.segments[1].policy_row_len, 1);
    assert_eq!(summary.validation.segments[1].control_desc_row_len, 1);
    assert_eq!(summary.validation.segments[1].control_marker_start, 0);
    assert_eq!(summary.validation.segments[1].control_marker_len, 1);
}
