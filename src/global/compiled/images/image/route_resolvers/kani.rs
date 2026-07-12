use super::RouteResolverRow;
use crate::global::const_dsl::{DynamicRouteResolver, INTRINSIC_ROUTE_RESOLVER_ID, ScopeId};

#[kani::proof]
fn route_resolver_row_decoding_accepts_exact_domain() {
    let raw_scope: u16 = kani::any();
    let resolver_id: u16 = kani::any();
    let controller_role: u8 = kani::any();
    let left_participant_mask: u16 = kani::any();
    let right_participant_mask: u16 = kani::any();
    let role_count: u8 = kani::any();

    let scope_valid = (raw_scope as usize) < crate::eff::meta::MAX_EFF_NODES;
    let role_count_valid = role_count != 0 && role_count <= crate::g::ROLE_DOMAIN_SIZE;
    let role_mask = if role_count == u16::BITS as u8 {
        u16::MAX
    } else if role_count_valid {
        (1u16 << role_count) - 1
    } else {
        0
    };
    let participant_mask = left_participant_mask | right_participant_mask;
    let participants_valid = left_participant_mask != 0
        && right_participant_mask != 0
        && (participant_mask & !role_mask) == 0;
    let controller_valid = role_count_valid
        && controller_role < role_count
        && (left_participant_mask & (1u16 << controller_role)) != 0
        && (right_participant_mask & (1u16 << controller_role)) != 0;
    let expected = scope_valid && role_count_valid && participants_valid && controller_valid;
    let decoded = RouteResolverRow::decode(
        raw_scope,
        resolver_id,
        controller_role,
        left_participant_mask,
        right_participant_mask,
        role_count,
    );

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.scope.raw() == raw_scope);
        assert!(row.controller_role() == controller_role);
        assert!(row.participant_mask(0) == left_participant_mask);
        assert!(row.participant_mask(1) == right_participant_mask);
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
