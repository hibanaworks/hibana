use super::{PackedRouteAuthority, RouteResolverRow};
use crate::global::compiled::images::image::columns::ProgramImageFacts;
use crate::global::{
    compiled::images::image::{CompiledProgramRef, ProgramImageColumns},
    const_dsl::{DynamicRouteResolver, ScopeId},
};

#[kani::proof]
fn route_resolver_row_decoding_accepts_exact_range_domain() {
    let packed_scope: u16 = kani::any();
    let resolver_id: u16 = kani::any();
    let controller_role: u8 = kani::any();
    let participant_start: u16 = kani::any();
    let left_len_minus_one: u8 = kani::any();
    let participant_end: u16 = kani::any();
    let participant_count: u16 = kani::any();

    let participant_mid = participant_start.checked_add(u16::from(left_len_minus_one) + 1);
    let dynamic = (packed_scope & ScopeId::RESERVED_BIT) != 0;
    let scope = packed_scope & !ScopeId::RESERVED_BIT;
    let expected = scope < ScopeId::LOCAL_CAPACITY
        && (dynamic || resolver_id == 0)
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
    kani::cover!(dynamic && scope == 0 && resolver_id == u16::MAX);
    kani::cover!(!dynamic && scope == 0 && resolver_id == 0);
    let decoded = RouteResolverRow::decode(
        packed_scope,
        resolver_id,
        controller_role,
        participant_start,
        left_len_minus_one,
        participant_end,
        participant_count as usize,
    );

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.scope.raw() == scope);
        assert!(row.controller_role == controller_role);
        assert!(row.participant_range(0).start == participant_start);
        assert!(row.participant_range(0).end == participant_mid.unwrap());
        assert!(row.participant_range(1).start == participant_mid.unwrap());
        assert!(row.participant_range(1).end == participant_end);
        assert!(row.resolver().is_some() == dynamic);
        if let Some(resolver) = row.resolver() {
            assert!(resolver.scope() == row.scope);
            assert!(resolver.resolver_id() == resolver_id);
        }
    }
}

#[kani::proof]
fn packed_route_authority_roundtrip_is_exact() {
    let scope_ordinal: u16 = kani::any();
    let resolver_id: u16 = kani::any();
    let dynamic: bool = kani::any();

    if scope_ordinal < ScopeId::LOCAL_CAPACITY {
        let scope = ScopeId::route(scope_ordinal);
        let resolver = if dynamic {
            Some(DynamicRouteResolver::new(scope, resolver_id))
        } else {
            None
        };
        let packed = PackedRouteAuthority::encode(scope, resolver);

        kani::cover!(dynamic && resolver_id == u16::MAX);
        kani::cover!(!dynamic && resolver_id == 0);
        assert!(
            PackedRouteAuthority::decode(packed.packed_scope(), packed.resolver_id())
                == Some((scope, resolver))
        );
        if dynamic {
            assert!((packed.packed_scope() & ScopeId::RESERVED_BIT) != 0);
        } else {
            assert!(packed.resolver_id() == 0);
        }
    }
}

// Separate the two wire tags so each constructor has a fixed byte layout.
// Together the harnesses retain both tags, all u8 roles and all dynamic u16 IDs.
fn check_canonical_participant_identity<const DYNAMIC: bool>(controller: u8, resolver_id: u16) {
    let packed_scope = if DYNAMIC { ScopeId::RESERVED_BIT } else { 0 };
    let bytes = [
        packed_scope as u8,
        (packed_scope >> 8) as u8,
        resolver_id as u8,
        (resolver_id >> 8) as u8,
        controller,
        0,
        0,
        0,
        controller,
        controller,
        0,
        0,
    ];
    let bytes: &'static [u8; 12] = unsafe {
        /* SAFETY: `bytes` remains live until the descriptor has completed every
        query and the forged program reference does not escape this harness. */
        core::mem::transmute(&bytes)
    };
    let columns = ProgramImageColumns::new(0, 1, 2, 0);
    let program =
        CompiledProgramRef::compact(ProgramImageFacts { max_role: u8::MAX }, columns, bytes);
    let scope = ScopeId::route(0);

    kani::cover!(controller == u8::MAX);
    if DYNAMIC {
        kani::cover!(resolver_id == u16::MAX);
    }
    assert!(program.role_count() == 256);
    assert!(program.route_controller_role(scope) == controller);
    assert!(
        program
            .route_resolver(scope)
            .map(|resolver| resolver.resolver_id())
            == if DYNAMIC { Some(resolver_id) } else { None }
    );
    assert!(program.route_participant_count(scope, 0) == 1);
    assert!(program.route_participant_count(scope, 1) == 1);
    assert!(program.route_participant_at(scope, 0, 0) == Some(controller));
    assert!(program.route_participant_at(scope, 1, 0) == Some(controller));
    assert!(program.route_participant_at(scope, 0, 1).is_none());
    assert!(program.route_has_participant(scope, 0, controller));
    assert!(program.route_has_participant(scope, 1, controller));
}

#[kani::proof]
#[kani::unwind(258)]
fn canonical_route_participant_identity_accepts_full_u8_role_domain() {
    check_canonical_participant_identity::<false>(kani::any(), 0);
}

#[kani::proof]
#[kani::unwind(258)]
fn dynamic_canonical_route_identity_accepts_all_role_and_resolver_bits() {
    check_canonical_participant_identity::<true>(kani::any(), kani::any());
}

#[kani::proof]
fn dynamic_route_resolver_identity_is_scope_and_id() {
    let left_scope: u16 = kani::any();
    let right_scope: u16 = kani::any();
    let left_id: u16 = kani::any();
    let right_id: u16 = kani::any();

    if left_scope < ScopeId::LOCAL_CAPACITY && right_scope < ScopeId::LOCAL_CAPACITY {
        let left = DynamicRouteResolver::new(ScopeId::route(left_scope), left_id);
        let right = DynamicRouteResolver::new(ScopeId::route(right_scope), right_id);
        kani::cover!(left_id == u16::MAX);
        kani::cover!(right_id == u16::MAX);
        assert!((left == right) == (left_scope == right_scope && left_id == right_id));
    }
}
