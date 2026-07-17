use super::{CompiledProgramRef, RouteResolverRow};
use crate::{
    g::{self, Msg},
    global::{
        compiled::images::image::columns::{ProgramImageColumns, ProgramImageFacts},
        const_dsl::ScopeId,
        role_program::{RoleProgram, project},
    },
};

const MAX_ROUTE_RESOLVER: u16 = u16::MAX;

const fn dynamic_scope(scope: u16) -> u16 {
    scope | ScopeId::RESERVED_BIT
}

const fn encoded_row(
    scope: u16,
    resolver_id: u16,
    controller_role: u8,
    start: u16,
    left_len: u16,
) -> [u8; 8] {
    [
        scope as u8,
        (scope >> 8) as u8,
        resolver_id as u8,
        (resolver_id >> 8) as u8,
        controller_role,
        start as u8,
        (start >> 8) as u8,
        (left_len - 1) as u8,
    ]
}

const fn encoded_descriptor<const N: usize, const M: usize>(
    row: [u8; 8],
    participants: [u8; N],
) -> [u8; M] {
    if M != 8 + N {
        crate::invariant();
    }
    let mut bytes = [0u8; M];
    let mut idx = 0usize;
    while idx < row.len() {
        bytes[idx] = row[idx];
        idx += 1;
    }
    idx = 0;
    while idx < participants.len() {
        bytes[8 + idx] = participants[idx];
        idx += 1;
    }
    bytes
}

fn forged_program_ref<const N: usize>(
    bytes: &'static [u8; N],
    max_role: u8,
    participant_count: usize,
) -> CompiledProgramRef {
    let columns = ProgramImageColumns::new(0, 1, participant_count, 0);
    CompiledProgramRef::compact(ProgramImageFacts { max_role }, columns, bytes)
}

static INTRINSIC: [u8; 12] = encoded_descriptor(encoded_row(0, 0, 0, 0, 2), [0, 1, 0, 1]);
static DYNAMIC: [u8; 11] = encoded_descriptor(encoded_row(dynamic_scope(1), 7, 0, 0, 1), [0, 0, 1]);
static DYNAMIC_MAX: [u8; 10] =
    encoded_descriptor(encoded_row(dynamic_scope(0), u16::MAX, 0, 0, 1), [0, 0]);
static NONCANONICAL_INTRINSIC: [u8; 10] = encoded_descriptor(encoded_row(0, 1, 0, 0, 1), [0, 0]);
static ROLE_255: [u8; 10] =
    encoded_descriptor(encoded_row(0, 0, u8::MAX, 0, 1), [u8::MAX, u8::MAX]);
static INVALID_PACKED_SCOPE: [u8; 10] =
    encoded_descriptor(encoded_row(u16::MAX, 7, 0, 0, 1), [0, 0]);
static NON_ROUTE_SCOPE: [u8; 10] = encoded_descriptor(
    encoded_row(dynamic_scope(ScopeId::roll_scope(0).raw()), 7, 0, 0, 1),
    [0, 0],
);
static EMPTY_RIGHT_PARTICIPANTS: [u8; 10] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 0, 0, 2), [0, 1]);
static ORPHAN_PARTICIPANT_PREFIX: [u8; 11] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 0, 1, 1), [0, 0, 0]);
static CONTROLLER_OUT_OF_RANGE: [u8; 12] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 2, 0, 2), [0, 1, 0, 1]);
static CONTROLLER_MISSING_FROM_ARM: [u8; 10] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 1, 0, 1), [0, 1]);
static UNSORTED_PARTICIPANTS: [u8; 12] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 0, 0, 2), [1, 0, 0, 1]);
static DUPLICATE_PARTICIPANTS: [u8; 12] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 0, 0, 2), [0, 0, 0, 1]);
static PARTICIPANT_OUT_OF_RANGE: [u8; 12] =
    encoded_descriptor(encoded_row(dynamic_scope(0), 7, 0, 0, 2), [0, 2, 0, 1]);

#[test]
fn compiled_program_descriptor_decodes_canonical_resolver_rows() {
    let intrinsic = forged_program_ref(&INTRINSIC, 1, 4);
    let scope = ScopeId::route(0);
    let (decoded_scope, resolver) = intrinsic
        .route_resolver_authority_at_row(0)
        .expect("intrinsic route authority");
    assert_eq!(decoded_scope, scope);
    assert!(resolver.is_none());
    assert_eq!(intrinsic.route_controller_role(scope), 0);
    assert_eq!(intrinsic.route_participant_count(scope, 0), 2);
    assert_eq!(intrinsic.route_participant_at(scope, 0, 0), Some(0));
    assert_eq!(intrinsic.route_participant_at(scope, 0, 1), Some(1));
    assert_eq!(intrinsic.route_participant_at(scope, 1, 0), Some(0));
    assert_eq!(intrinsic.route_participant_at(scope, 1, 1), Some(1));
    assert!(intrinsic.route_resolver(scope).is_none());

    let dynamic = forged_program_ref(&DYNAMIC, 1, 3);
    let scope = ScopeId::route(1);
    assert_eq!(dynamic.route_controller_role(scope), 0);
    assert_eq!(dynamic.route_participant_count(scope, 0), 1);
    assert_eq!(dynamic.route_participant_count(scope, 1), 2);
    let resolver = dynamic.route_resolver(scope).expect("dynamic resolver");
    assert_eq!(resolver.scope(), scope);
    assert_eq!(resolver.resolver_id(), 7);

    let dynamic_max = forged_program_ref(&DYNAMIC_MAX, 0, 2);
    let resolver = dynamic_max
        .route_resolver(ScopeId::route(0))
        .expect("maximum dynamic resolver id");
    assert_eq!(resolver.resolver_id(), u16::MAX);
}

#[test]
fn public_projection_preserves_the_full_u16_resolver_id_domain() {
    let route = g::route(
        g::send::<0, 1, Msg<35, u8>>(),
        g::send::<0, 1, Msg<36, u8>>(),
    )
    .resolve::<MAX_ROUTE_RESOLVER>();
    let role0: RoleProgram<0> = project(&route);
    let program = role0.role_image_ref().program;

    assert_eq!(program.route_resolver_row_count(), 1);
    let (_, resolver) = program
        .route_resolver_authority_at_row(0)
        .expect("dynamic route authority");
    assert_eq!(
        resolver.map(|resolver| resolver.resolver_id()),
        Some(MAX_ROUTE_RESOLVER)
    );
}

#[test]
fn compiled_program_resolver_decoder_rejects_invalid_ranges_and_scopes() {
    assert!(RouteResolverRow::decode(0, 0, 0, 0, 0, 2, 2).is_some());
    assert!(RouteResolverRow::decode(0, 1, 0, 0, 0, 2, 2).is_none());
    assert!(RouteResolverRow::decode(dynamic_scope(0), u16::MAX, 0, 0, 0, 2, 2).is_some());
    assert!(RouteResolverRow::decode(u16::MAX, 0, 0, 0, 0, 2, 2).is_none());
    assert!(
        RouteResolverRow::decode(
            dynamic_scope(ScopeId::roll_scope(0).raw()),
            0,
            0,
            0,
            0,
            2,
            2,
        )
        .is_none()
    );
    assert!(
        RouteResolverRow::decode(
            dynamic_scope(0),
            0,
            0,
            u16::MAX,
            0,
            u16::MAX,
            u16::MAX as usize,
        )
        .is_none()
    );
    assert!(RouteResolverRow::decode(dynamic_scope(0), 0, 0, 0, 0, 1, 1).is_none());
    assert!(RouteResolverRow::decode(dynamic_scope(0), 0, 0, 0, 0, 258, 258).is_none());
    assert!(
        RouteResolverRow::decode(
            dynamic_scope(0),
            0,
            0,
            u16::MAX - 2,
            0,
            u16::MAX,
            u16::MAX as usize,
        )
        .is_some()
    );
    let full_arm = RouteResolverRow::decode(dynamic_scope(0), 0, 0, 0, u8::MAX, 257, 257)
        .expect("encoded length 255 denotes 256 participants");
    assert_eq!(full_arm.participant_range(0).len(), 256);
    assert_eq!(full_arm.participant_range(1).len(), 1);
}

#[test]
fn compiled_program_descriptor_accepts_role_255_participant() {
    let descriptor = forged_program_ref(&ROLE_255, u8::MAX, 2);
    let scope = ScopeId::route(0);
    assert!(descriptor.route_has_participant(scope, 0, u8::MAX));
    assert!(descriptor.route_has_participant(scope, 1, u8::MAX));
    assert_eq!(descriptor.role_count(), 256);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_invalid_packed_scope() {
    let descriptor = forged_program_ref(&INVALID_PACKED_SCOPE, 0, 2);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_noncanonical_intrinsic_authority() {
    let descriptor = forged_program_ref(&NONCANONICAL_INTRINSIC, 0, 2);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_non_route_scope() {
    let descriptor = forged_program_ref(&NON_ROUTE_SCOPE, 0, 2);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_empty_right_participant_range() {
    let descriptor = forged_program_ref(&EMPTY_RIGHT_PARTICIPANTS, 1, 2);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_orphan_participant_prefix() {
    let descriptor = forged_program_ref(&ORPHAN_PARTICIPANT_PREFIX, 0, 3);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_controller_out_of_range() {
    let descriptor = forged_program_ref(&CONTROLLER_OUT_OF_RANGE, 1, 4);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_controller_missing_from_arm() {
    let descriptor = forged_program_ref(&CONTROLLER_MISSING_FROM_ARM, 1, 2);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_unsorted_participants() {
    let descriptor = forged_program_ref(&UNSORTED_PARTICIPANTS, 1, 4);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_duplicate_participants() {
    let descriptor = forged_program_ref(&DUPLICATE_PARTICIPANTS, 1, 4);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_participant_out_of_role_range() {
    let descriptor = forged_program_ref(&PARTICIPANT_OUT_OF_RANGE, 1, 4);
    let _ = descriptor.route_resolver_authority_at_row(0);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_absent_scope_query() {
    let descriptor = forged_program_ref(&INTRINSIC, 1, 4);
    let _ = descriptor.route_controller_role(ScopeId::none());
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_non_route_scope_query() {
    let descriptor = forged_program_ref(&INTRINSIC, 1, 4);
    let _ = descriptor.route_controller_role(ScopeId::parallel(0));
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_missing_route_scope_query() {
    let descriptor = forged_program_ref(&INTRINSIC, 1, 4);
    let _ = descriptor.route_controller_role(ScopeId::route(1));
}
