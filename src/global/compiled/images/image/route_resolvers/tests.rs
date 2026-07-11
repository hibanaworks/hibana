use super::{CompiledProgramRef, RouteResolverRow};
use crate::global::{
    compiled::images::image::columns::{
        PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange,
        ProgramImageColumns, ProgramImageFacts,
    },
    const_dsl::{INTRINSIC_ROUTE_RESOLVER_ID, ScopeId},
};

const fn encoded_row(scope: u16, resolver_id: u16, controller_role: u8) -> [u8; 5] {
    [
        scope as u8,
        (scope >> 8) as u8,
        resolver_id as u8,
        (resolver_id >> 8) as u8,
        controller_role,
    ]
}

fn forged_program_ref(bytes: &'static [u8; 5], role_count: u8) -> CompiledProgramRef {
    let columns = ProgramImageColumns {
        atoms: ProgramColumnRange::new(0, 0, PROGRAM_IMAGE_ATOM_STRIDE),
        route_resolvers: ProgramColumnRange::new(0, 1, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE),
    };
    CompiledProgramRef::compact(ProgramImageFacts { role_count }, columns, bytes)
}

static INTRINSIC: [u8; 5] = encoded_row(0, INTRINSIC_ROUTE_RESOLVER_ID, 0);
static DYNAMIC: [u8; 5] = encoded_row(1, 7, u8::MAX);
static ABSENT_SCOPE: [u8; 5] = encoded_row(u16::MAX, 7, 0);
static ROLL_SCOPE: [u8; 5] = encoded_row(ScopeId::roll_scope(0).raw(), 7, 0);
static ROUTE_ORDINAL_OUT_OF_RANGE: [u8; 5] =
    encoded_row(crate::eff::meta::MAX_EFF_NODES as u16, 7, 0);
static CONTROLLER_OUT_OF_RANGE: [u8; 5] = encoded_row(0, 7, 2);
static INTRINSIC_WITHOUT_CONTROLLER: [u8; 5] = encoded_row(0, INTRINSIC_ROUTE_RESOLVER_ID, u8::MAX);

#[test]
fn compiled_program_descriptor_decodes_canonical_resolver_rows() {
    let intrinsic = forged_program_ref(&INTRINSIC, 2);
    assert_eq!(
        intrinsic.route_resolver_scope_at_row(0),
        Some(ScopeId::route(0))
    );
    assert_eq!(intrinsic.route_controller_role(ScopeId::route(0)), Some(0));
    assert!(intrinsic.route_resolver(ScopeId::route(0)).is_none());

    let dynamic = forged_program_ref(&DYNAMIC, 2);
    assert_eq!(dynamic.route_controller_role(ScopeId::route(1)), None);
    let resolver = dynamic
        .route_resolver(ScopeId::route(1))
        .expect("dynamic resolver");
    assert_eq!(resolver.scope(), ScopeId::route(1));
    assert_eq!(resolver.resolver_id(), 7);
}

#[test]
fn compiled_program_resolver_decoder_rejects_exact_invalid_boundaries() {
    assert!(RouteResolverRow::decode(0, INTRINSIC_ROUTE_RESOLVER_ID, 0, 1).is_some());
    assert!(RouteResolverRow::decode(0, 0, u8::MAX, 1).is_some());
    assert!(RouteResolverRow::decode(u16::MAX, 0, 0, 1).is_none());
    assert!(RouteResolverRow::decode(ScopeId::roll_scope(0).raw(), 0, 0, 1).is_none());
    assert!(RouteResolverRow::decode(crate::eff::meta::MAX_EFF_NODES as u16, 0, 0, 1).is_none());
    assert!(RouteResolverRow::decode(0, 0, 1, 1).is_none());
    assert!(RouteResolverRow::decode(0, INTRINSIC_ROUTE_RESOLVER_ID, u8::MAX, 1).is_none());
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
fn compiled_program_descriptor_rejects_intrinsic_without_controller() {
    let _ = forged_program_ref(&INTRINSIC_WITHOUT_CONTROLLER, 1).route_resolver_scope_at_row(0);
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
