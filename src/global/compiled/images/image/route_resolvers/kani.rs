use super::RouteResolverRow;
use crate::global::compiled::images::image::columns::ProgramImageFacts;
use crate::global::{
    compiled::images::image::{CompiledProgramRef, ProgramImageColumns},
    const_dsl::{DynamicRouteResolver, INTRINSIC_ROUTE_RESOLVER_ID, ScopeId},
};

#[kani::proof]
fn route_resolver_row_decoding_accepts_exact_range_domain() {
    let raw_scope: u16 = kani::any();
    let resolver_id: u16 = kani::any();
    let controller_role: u8 = kani::any();
    let participant_start: u16 = kani::any();
    let left_len_minus_one: u8 = kani::any();
    let participant_end: u16 = kani::any();
    let participant_count: u16 = kani::any();

    let participant_mid = participant_start.checked_add(u16::from(left_len_minus_one) + 1);
    let expected = (raw_scope as usize) < crate::eff::meta::MAX_EFF_NODES
        && participant_mid.is_some_and(|mid| {
            mid < participant_end
                && participant_end - mid <= 256
                && participant_end <= participant_count
        });
    kani::cover!(
        participant_start == 0
            && left_len_minus_one == u8::MAX
            && participant_end == 257
            && participant_count == 257
    );
    let decoded = RouteResolverRow::decode(
        raw_scope,
        resolver_id,
        controller_role,
        participant_start,
        left_len_minus_one,
        participant_end,
        participant_count as usize,
    );

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.scope.raw() == raw_scope);
        assert!(row.controller_role == controller_role);
        assert!(row.participant_range(0).start == participant_start);
        assert!(row.participant_range(0).end == participant_mid.unwrap());
        assert!(row.participant_range(1).start == participant_mid.unwrap());
        assert!(row.participant_range(1).end == participant_end);
        assert!(row.resolver().is_some() == (resolver_id != INTRINSIC_ROUTE_RESOLVER_ID));
        if let Some(resolver) = row.resolver() {
            assert!(resolver.scope() == row.scope);
            assert!(resolver.resolver_id() == resolver_id);
        }
    }
}

#[kani::proof]
fn canonical_route_participant_identity_accepts_full_u8_role_domain() {
    let controller: u8 = kani::any();
    let bytes = [
        0,
        0,
        u8::MAX,
        u8::MAX,
        controller,
        0,
        0,
        0,
        controller,
        controller,
    ];
    let bytes: &'static [u8; 10] = unsafe {
        /* SAFETY: `bytes` remains live until the descriptor has completed every
        query and the forged program reference does not escape this harness. */
        core::mem::transmute(&bytes)
    };
    let columns = ProgramImageColumns::new(0, 1, 2, 0);
    let program =
        CompiledProgramRef::compact(ProgramImageFacts { max_role: u8::MAX }, columns, bytes);
    let scope = ScopeId::route(0);

    kani::cover!(controller == u8::MAX);
    assert!(program.role_count() == 256);
    assert!(program.route_controller_role(scope) == controller);
    assert!(program.route_participant_count(scope, 0) == 1);
    assert!(program.route_participant_count(scope, 1) == 1);
    assert!(program.route_participant_at(scope, 0, 0) == Some(controller));
    assert!(program.route_participant_at(scope, 1, 0) == Some(controller));
    assert!(program.route_participant_at(scope, 0, 1).is_none());
    assert!(program.route_has_participant(scope, 0, controller));
    assert!(program.route_has_participant(scope, 1, controller));
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
