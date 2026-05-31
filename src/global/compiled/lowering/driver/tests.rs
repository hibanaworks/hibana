use crate::control::cap::mint::LocalControlKind;
use crate::control::cap::resource_kinds::RouteDecisionKind;
use crate::eff::{EffAtom, EffIndex, EffStruct};
use crate::global::StaticControlDesc;
use crate::global::const_dsl::{ControlScopeKind, EffList, PolicyMode, ScopeId, ScopeKind};
use crate::global::program::boundary_source_program_image;

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
        .push_control_spec(0, StaticControlDesc::of_local::<RouteDecisionKind>())
        .push_control_marker(0, ControlScopeKind::Route, 77)
        .push_policy(0, PolicyMode::dynamic(77))
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
        resource: Some(<RouteDecisionKind as LocalControlKind>::TAG),
        lane: 0,
    })
}

const fn policy_side_table_regression_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < SIDE_TABLE_CAPACITY_REGRESSION_ROWS {
        let left = EffList::new()
            .push(control_atom(idx as u8))
            .push_control_spec(0, StaticControlDesc::of_local::<RouteDecisionKind>())
            .push_control_marker(0, ControlScopeKind::Route, idx as u16)
            .push_policy(0, PolicyMode::dynamic(7))
            .with_scope(ScopeId::new(ScopeKind::Route, idx as u16));
        let right = EffList::new()
            .push(control_atom((idx + 1) as u8))
            .push_control_spec(0, StaticControlDesc::of_local::<RouteDecisionKind>())
            .push_control_marker(0, ControlScopeKind::Route, idx as u16)
            .push_policy(0, PolicyMode::dynamic(7))
            .with_scope(ScopeId::new(ScopeKind::Route, idx as u16));
        list = list.extend_list(left).extend_list(right);
        idx += 2;
    }
    list
}

const fn control_side_table_regression_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx < SIDE_TABLE_CAPACITY_REGRESSION_ROWS {
        list = list.push(control_atom(idx as u8));
        list = list.push_control_spec(idx, StaticControlDesc::of_local::<RouteDecisionKind>());
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
static CONTROL_SIDE_TABLE_REGRESSION_PROGRAM: EffList = control_side_table_regression_program();

fn regression_policy_lookup(offset: usize) -> Option<PolicyMode> {
    POLICY_SIDE_TABLE_REGRESSION_PROGRAM
        .policy_with_scope(offset)
        .map(|(policy, _scope)| policy)
}

fn regression_control_lookup(offset: usize) -> Option<crate::global::ControlDesc> {
    let spec = CONTROL_SIDE_TABLE_REGRESSION_PROGRAM.control_spec_at(offset)?;
    Some(crate::global::ControlDesc::from_static(spec).with_sites(
        EffIndex::from_dense_ordinal(offset),
        crate::global::ControlDesc::STATIC_POLICY_SITE,
    ))
}

fn no_regression_policy_lookup(_: usize) -> Option<PolicyMode> {
    None
}

fn no_regression_control_lookup(_: usize) -> Option<crate::global::ControlDesc> {
    None
}

static SCOPE_ENTER_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    boundary_source_program_image(&SCOPE_ENTER_AT_BOUNDARY);
static SCOPE_EXIT_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    boundary_source_program_image(&SCOPE_EXIT_AT_BOUNDARY);
static CONTROL_SPEC_AT_BOUNDARY_SUMMARY: super::CompiledProgramImage =
    boundary_source_program_image(&CONTROL_SPEC_AT_BOUNDARY);
static ATOM_HEAVY_SUMMARY: super::CompiledProgramImage =
    boundary_source_program_image(&ATOM_HEAVY_PROGRAM);
static POLICY_SIDE_TABLE_REGRESSION_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const_with_lookup(
        &POLICY_SIDE_TABLE_REGRESSION_PROGRAM,
        super::ProgramSourceLookup::new(regression_policy_lookup, no_regression_control_lookup),
    );
static CONTROL_SIDE_TABLE_REGRESSION_SUMMARY: super::CompiledProgramImage =
    super::CompiledProgramImage::scan_const_with_lookup(
        &CONTROL_SIDE_TABLE_REGRESSION_PROGRAM,
        super::ProgramSourceLookup::new(no_regression_policy_lookup, regression_control_lookup),
    );

#[test]
fn ordinary_atom_capacity_is_not_tied_to_tap_event_budget() {
    assert!(ATOM_HEAVY_PROGRAM.len() > super::MAX_COMPILED_PROGRAM_TAP_EVENTS);
    ATOM_HEAVY_SUMMARY.validate_projection_program();
    crate::global::compiled::lowering::seal::validate_all_roles(
        &ATOM_HEAVY_SUMMARY,
        &ATOM_HEAVY_PROGRAM,
    );
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
    crate::global::compiled::lowering::seal::validate_all_roles(
        &POLICY_SIDE_TABLE_REGRESSION_SUMMARY,
        &POLICY_SIDE_TABLE_REGRESSION_PROGRAM,
    );

    let last = SIDE_TABLE_CAPACITY_REGRESSION_ROWS - 1;
    let view = POLICY_SIDE_TABLE_REGRESSION_SUMMARY.view();
    assert_eq!(
        view.policy_at(last)
            .and_then(|policy| policy.dynamic_policy_id()),
        Some(7)
    );
}

#[test]
fn control_side_tables_keep_0_6_0_program_capacity() {
    assert!(SIDE_TABLE_CAPACITY_REGRESSION_ROWS > crate::eff::meta::MAX_SEGMENTS * 2);
    CONTROL_SIDE_TABLE_REGRESSION_SUMMARY.validate_projection_program();
    crate::global::compiled::lowering::seal::validate_all_roles(
        &CONTROL_SIDE_TABLE_REGRESSION_SUMMARY,
        &CONTROL_SIDE_TABLE_REGRESSION_PROGRAM,
    );

    let last = SIDE_TABLE_CAPACITY_REGRESSION_ROWS - 1;
    let view = CONTROL_SIDE_TABLE_REGRESSION_SUMMARY.view();
    assert!(view.control_desc_at(last).is_some());
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
            .control_markers()
            .len(),
        crate::eff::meta::MAX_SEGMENTS * 2
    );
}

#[test]
fn lowering_scope_enter_at_exact_segment_boundary_belongs_to_next_segment() {
    let summary = &SCOPE_ENTER_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.segment_summary(0).scope_marker_len(), 0);
    assert_eq!(summary.segment_summary(1).scope_marker_len(), 2);
    assert_eq!(summary.segment_summary(1).route_scope_enter_len(), 1);
    assert_eq!(summary.validation.segments[1].scope_marker_start, 0);
    assert_eq!(summary.validation.segments[1].scope_marker_len, 2);
}

#[test]
fn lowering_scope_exit_at_exact_segment_boundary_belongs_to_previous_segment() {
    let summary = &SCOPE_EXIT_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.segment_summary(0).scope_marker_len(), 2);
    assert_eq!(summary.segment_summary(0).route_scope_enter_len(), 1);
    assert_eq!(summary.segment_summary(1).scope_marker_len(), 0);
    assert_eq!(summary.validation.segments[0].scope_marker_start, 0);
    assert_eq!(summary.validation.segments[0].scope_marker_len, 2);
}

#[test]
fn lowering_control_spec_at_segment_boundary_belongs_to_effect_segment() {
    let summary = &CONTROL_SPEC_AT_BOUNDARY_SUMMARY;

    assert_eq!(summary.segment_summary(0).control_marker_len(), 0);
    assert_eq!(summary.segment_summary(0).policy_marker_len(), 0);
    assert_eq!(summary.segment_summary(0).control_spec_len(), 0);
    assert_eq!(summary.segment_summary(1).control_marker_len(), 1);
    assert_eq!(summary.segment_summary(1).policy_marker_len(), 1);
    assert_eq!(summary.segment_summary(1).control_spec_len(), 1);
    assert_eq!(summary.validation.segments[1].control_marker_start, 0);
    assert_eq!(summary.validation.segments[1].control_marker_len, 1);
}
