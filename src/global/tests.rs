use super::role_program::RoleProgram;
use crate::g::Program;
use crate::global::compiled::images::{
    PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY, PROGRAM_IMAGE_ATOM_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
};
use crate::global::const_dsl::{EffList, ScopeId};
use core::mem::size_of;

const _: () = assert!(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE <= 12);

#[test]
fn descriptor_first_size_gates_hold() {
    assert_eq!(
        size_of::<Program<()>>(),
        0,
        "Program<Steps> must stay zero-sized"
    );
    assert!(
        size_of::<RoleProgram<0>>() <= 24,
        "RoleProgram<ROLE> must stay compact"
    );
}

#[test]
fn compact_event_identity_and_descriptor_byte_domains_are_not_conflated() {
    assert_eq!(crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY, 65_535);
    assert_eq!(PROGRAM_IMAGE_ATOM_STRIDE, 11);
    assert_eq!(PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY, 5_957);
}

#[test]
fn dynamic_route_resolver_lookup_is_exactly_route_scoped() {
    let mut effects = EffList::<1>::new_partitioned(0, 0, 1);
    effects.push_route_resolver_mut(ScopeId::route(3), 7);
    let resolver = effects
        .resolver_for_scope(ScopeId::route(3))
        .expect("dynamic resolver");
    assert_eq!(resolver.scope(), ScopeId::route(3));
    assert_eq!(resolver.resolver_id(), 7);
    assert!(effects.resolver_for_scope(ScopeId::route(2)).is_none());
}

#[test]
#[should_panic]
fn absent_scope_cannot_query_dynamic_route_resolver() {
    let _ = EffList::<0>::new().resolver_for_scope(ScopeId::none());
}

#[test]
#[should_panic]
fn non_route_scope_cannot_query_dynamic_route_resolver() {
    let _ = EffList::<0>::new().resolver_for_scope(ScopeId::parallel(0));
}

#[test]
fn highest_scope_ordinal_can_query_dynamic_route_resolver() {
    assert!(
        EffList::<0>::new()
            .resolver_for_scope(ScopeId::route(ScopeId::MAX_LOCAL_ORDINAL))
            .is_none()
    );
}

#[test]
#[should_panic(expected = "duplicate route resolver scope")]
fn route_scope_cannot_acquire_two_compile_time_resolver_authorities() {
    let mut effects = EffList::<2>::new_partitioned(0, 0, 2);
    effects.push_route_resolver_mut(ScopeId::route(3), 7);
    effects.push_route_resolver_mut(ScopeId::route(3), 8);
}
