use super::role_program::RoleProgram;
use crate::g::Program;
use crate::global::compiled::images::PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
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
fn dynamic_route_resolver_lookup_is_exactly_route_scoped() {
    let mut effects = EffList::new();
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
    let _ = EffList::new().resolver_for_scope(ScopeId::none());
}

#[test]
#[should_panic]
fn non_route_scope_cannot_query_dynamic_route_resolver() {
    let _ = EffList::new().resolver_for_scope(ScopeId::parallel(0));
}

#[test]
#[should_panic]
fn out_of_domain_scope_cannot_query_dynamic_route_resolver() {
    let _ =
        EffList::new().resolver_for_scope(ScopeId::route(crate::eff::meta::MAX_EFF_NODES as u16));
}

#[test]
#[should_panic(expected = "duplicate route resolver scope")]
fn route_scope_cannot_acquire_two_compile_time_resolver_authorities() {
    let mut effects = EffList::new();
    effects.push_route_resolver_mut(ScopeId::route(3), 7);
    effects.push_route_resolver_mut(ScopeId::route(3), 8);
}
