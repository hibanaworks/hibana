use super::RouteResolverRow;
use crate::global::const_dsl::{DynamicRouteResolver, INTRINSIC_ROUTE_RESOLVER_ID, ScopeId};

#[kani::proof]
fn route_resolver_row_decoding_accepts_exact_domain() {
    let raw_scope: u16 = kani::any();
    let resolver_id: u16 = kani::any();
    let controller_role: u8 = kani::any();
    let role_count: u8 = kani::any();

    let scope_valid = (raw_scope as usize) < crate::eff::meta::MAX_EFF_NODES;
    let role_count_valid = role_count != 0 && role_count <= crate::g::ROLE_DOMAIN_SIZE;
    let controller_valid = controller_role == u8::MAX || controller_role < role_count;
    let intrinsic_has_controller =
        resolver_id != INTRINSIC_ROUTE_RESOLVER_ID || controller_role != u8::MAX;
    let expected = scope_valid && role_count_valid && controller_valid && intrinsic_has_controller;
    let decoded = RouteResolverRow::decode(raw_scope, resolver_id, controller_role, role_count);

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.scope.raw() == raw_scope);
        assert!(row.controller_role() == (controller_role != u8::MAX).then_some(controller_role));
        assert!(row.resolver().is_some() == (resolver_id != INTRINSIC_ROUTE_RESOLVER_ID));
        if let Some(resolver) = row.resolver() {
            assert!(resolver.scope() == row.scope);
            assert!(resolver.resolver_id() == resolver_id);
        }
    }
}

#[kani::proof]
fn dynamic_route_resolver_identity_is_scope_and_id() {
    let left_scope: u16 = kani::any();
    let right_scope: u16 = kani::any();
    let left_id: u16 = kani::any();
    let right_id: u16 = kani::any();

    if (left_scope as usize) < crate::eff::meta::MAX_EFF_NODES
        && (right_scope as usize) < crate::eff::meta::MAX_EFF_NODES
        && left_id != INTRINSIC_ROUTE_RESOLVER_ID
        && right_id != INTRINSIC_ROUTE_RESOLVER_ID
    {
        let left = DynamicRouteResolver::new(ScopeId::route(left_scope), left_id);
        let right = DynamicRouteResolver::new(ScopeId::route(right_scope), right_id);
        assert!((left == right) == (left_scope == right_scope && left_id == right_id));
    }
}
