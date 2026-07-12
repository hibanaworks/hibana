use super::{CompiledProgramRef, RouteResolverRow};
use crate::global::{
    compiled::images::image::columns::{ProgramImageColumns, ProgramImageFacts},
    const_dsl::{INTRINSIC_ROUTE_RESOLVER_ID, ScopeId},
};

const fn encoded_row(
    scope: u16,
    resolver_id: u16,
    controller_role: u8,
    left_participant_mask: u16,
    right_participant_mask: u16,
) -> [u8; 9] {
    [
        scope as u8,
        (scope >> 8) as u8,
        resolver_id as u8,
        (resolver_id >> 8) as u8,
        controller_role,
        left_participant_mask as u8,
        (left_participant_mask >> 8) as u8,
        right_participant_mask as u8,
        (right_participant_mask >> 8) as u8,
    ]
}

fn forged_program_ref(bytes: &'static [u8; 9], role_count: u8) -> CompiledProgramRef {
    let columns = ProgramImageColumns::new(0, 1, 0);
    CompiledProgramRef::compact(ProgramImageFacts { role_count }, columns, bytes)
}

static INTRINSIC: [u8; 9] = encoded_row(0, INTRINSIC_ROUTE_RESOLVER_ID, 0, 0b11, 0b11);
static DYNAMIC: [u8; 9] = encoded_row(1, 7, 0, 0b01, 0b11);
static ABSENT_SCOPE: [u8; 9] = encoded_row(u16::MAX, 7, 0, 1, 1);
static ROLL_SCOPE: [u8; 9] = encoded_row(ScopeId::roll_scope(0).raw(), 7, 0, 1, 1);
static ROUTE_ORDINAL_OUT_OF_RANGE: [u8; 9] =
    encoded_row(crate::eff::meta::MAX_EFF_NODES as u16, 7, 0, 1, 1);
static CONTROLLER_OUT_OF_RANGE: [u8; 9] = encoded_row(0, 7, 2, 0b01, 0b10);
static CONTROLLER_NOT_PARTICIPANT: [u8; 9] = encoded_row(0, 7, 1, 0b01, 0b01);
static CONTROLLER_MISSING_FROM_RIGHT_ARM: [u8; 9] = encoded_row(0, 7, 0, 0b01, 0b10);
static PARTICIPANT_OUT_OF_RANGE: [u8; 9] = encoded_row(0, 7, 0, 0b101, 0b001);
static EMPTY_PARTICIPANTS: [u8; 9] = encoded_row(0, 7, 0, 0, 1);

#[test]
fn compiled_program_descriptor_decodes_canonical_resolver_rows() {
    let intrinsic = forged_program_ref(&INTRINSIC, 2);
    assert_eq!(
        intrinsic.route_resolver_scope_at_row(0),
        Some(ScopeId::route(0))
    );
    assert_eq!(intrinsic.route_controller_role(ScopeId::route(0)), 0);
    assert_eq!(intrinsic.route_participant_mask(ScopeId::route(0), 0), 0b11);
    assert_eq!(intrinsic.route_participant_mask(ScopeId::route(0), 1), 0b11);
    assert!(intrinsic.route_resolver(ScopeId::route(0)).is_none());

    let dynamic = forged_program_ref(&DYNAMIC, 2);
    assert_eq!(dynamic.route_controller_role(ScopeId::route(1)), 0);
    assert_eq!(dynamic.route_participant_mask(ScopeId::route(1), 0), 0b01);
    assert_eq!(dynamic.route_participant_mask(ScopeId::route(1), 1), 0b11);
    let resolver = dynamic
        .route_resolver(ScopeId::route(1))
        .expect("dynamic resolver");
    assert_eq!(resolver.scope(), ScopeId::route(1));
    assert_eq!(resolver.resolver_id(), 7);
}

#[test]
fn compiled_program_resolver_decoder_rejects_exact_invalid_boundaries() {
    assert!(RouteResolverRow::decode(0, INTRINSIC_ROUTE_RESOLVER_ID, 0, 1, 1, 1).is_some());
    assert!(RouteResolverRow::decode(0, 0, u8::MAX, 1, 1, 1).is_none());
    assert!(RouteResolverRow::decode(u16::MAX, 0, 0, 1, 1, 1).is_none());
    assert!(RouteResolverRow::decode(ScopeId::roll_scope(0).raw(), 0, 0, 1, 1, 1).is_none());
    assert!(
        RouteResolverRow::decode(crate::eff::meta::MAX_EFF_NODES as u16, 0, 0, 1, 1, 1).is_none()
    );
    assert!(RouteResolverRow::decode(0, 0, 1, 1, 1, 1).is_none());
    assert!(RouteResolverRow::decode(0, 0, 0, 0, 1, 1).is_none());
    assert!(RouteResolverRow::decode(0, 0, 0, 1, 0b10, 1).is_none());
    assert!(RouteResolverRow::decode(0, INTRINSIC_ROUTE_RESOLVER_ID, u8::MAX, 1, 1, 1).is_none());
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_absent_route_scope() {
    let _ = forged_program_ref(&ABSENT_SCOPE, 1).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_non_route_scope() {
    let _ = forged_program_ref(&ROLL_SCOPE, 1).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_route_ordinal_out_of_range() {
    let _ = forged_program_ref(&ROUTE_ORDINAL_OUT_OF_RANGE, 1).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_zero_role_count() {
    let _ = forged_program_ref(&DYNAMIC, 0).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_role_count_out_of_domain() {
    let _ =
        forged_program_ref(&DYNAMIC, crate::g::ROLE_DOMAIN_SIZE + 1).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_controller_out_of_range() {
    let _ = forged_program_ref(&CONTROLLER_OUT_OF_RANGE, 2).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_controller_outside_participant_mask() {
    let _ = forged_program_ref(&CONTROLLER_NOT_PARTICIPANT, 2).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_controller_missing_from_one_arm() {
    let _ =
        forged_program_ref(&CONTROLLER_MISSING_FROM_RIGHT_ARM, 2).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_participant_out_of_role_range() {
    let _ = forged_program_ref(&PARTICIPANT_OUT_OF_RANGE, 2).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_empty_participant_mask() {
    let _ = forged_program_ref(&EMPTY_PARTICIPANTS, 2).route_resolver_scope_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_absent_scope_query() {
    let _ = forged_program_ref(&INTRINSIC, 1).route_controller_role(ScopeId::none());
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_non_route_scope_query() {
    let _ = forged_program_ref(&INTRINSIC, 1).route_controller_role(ScopeId::parallel(0));
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_missing_route_scope_query() {
    let _ = forged_program_ref(&INTRINSIC, 1).route_controller_role(ScopeId::route(1));
}
