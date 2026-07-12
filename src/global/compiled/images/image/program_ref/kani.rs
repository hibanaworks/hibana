use super::{CompiledProgramRef, PackedProgramAtomFields, ProgramAtomRow};
use crate::global::compiled::images::image::blob_storage::{
    ProgramImageBytes, scope_marker_identity_tag,
};
use crate::global::compiled::images::image::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};
use crate::global::const_dsl::{EffList, ReentryMark, ScopeEvent};
use crate::global::role_program::ColumnRange;

static CANONICAL_IMAGE: [u8; 27] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26,
];
static SAME_IMAGE: [u8; 27] = CANONICAL_IMAGE;
static LAST_BYTE_DIFFERENT: [u8; 27] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    27,
];

fn route_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 3, 0)
}

fn identity_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(1, 0, 1)
}

fn alternate_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(2, 0, 1)
}

fn program<const N: usize>(
    bytes: &'static [u8; N],
    facts: ProgramImageFacts,
    columns: ProgramImageColumns,
) -> CompiledProgramRef {
    CompiledProgramRef::compact(facts, columns, bytes)
}

#[kani::proof]
fn program_image_columns_are_canonical_for_exact_count_domain() {
    let atom_len: u16 = kani::any();
    let route_resolver_len: u16 = kani::any();
    let scope_marker_len: u16 = kani::any();
    let atom_bytes = usize::from(atom_len) * PROGRAM_IMAGE_ATOM_STRIDE;
    let route_resolver_bytes =
        usize::from(route_resolver_len) * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
    let scope_marker_bytes = usize::from(scope_marker_len) * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
    let route_resolver_offset = atom_bytes;
    let scope_marker_offset = route_resolver_offset + route_resolver_bytes;
    let blob_len = scope_marker_offset + scope_marker_bytes;
    let valid = blob_len <= usize::from(u16::MAX);

    kani::cover!(valid);
    kani::cover!(!valid);
    if valid {
        let columns = ProgramImageColumns::new(
            usize::from(atom_len),
            usize::from(route_resolver_len),
            usize::from(scope_marker_len),
        );
        assert!(columns.atoms().offset == 0);
        assert!(columns.atoms().len == atom_len);
        assert!(columns.route_resolvers().offset as usize == route_resolver_offset);
        assert!(columns.route_resolvers().len == route_resolver_len);
        assert!(columns.scope_markers().offset as usize == scope_marker_offset);
        assert!(columns.scope_markers().len == scope_marker_len);
        assert!(columns.blob_len() == blob_len);
    }
}

#[kani::proof]
#[kani::should_panic]
fn program_image_columns_reject_total_byte_overflow() {
    let atom_len: u16 = kani::any();
    let route_resolver_len: u16 = kani::any();
    let scope_marker_len: u16 = kani::any();
    let blob_len = usize::from(atom_len) * PROGRAM_IMAGE_ATOM_STRIDE
        + usize::from(route_resolver_len) * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE
        + usize::from(scope_marker_len) * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
    let overflow = blob_len > usize::from(u16::MAX);

    kani::cover!(overflow);
    kani::cover!(!overflow);
    let (atom_len, route_resolver_len, scope_marker_len) = if overflow {
        (atom_len, route_resolver_len, scope_marker_len)
    } else {
        (u16::MAX, u16::MAX, u16::MAX)
    };
    let _ = ProgramImageColumns::new(
        usize::from(atom_len),
        usize::from(route_resolver_len),
        usize::from(scope_marker_len),
    );
}

#[kani::proof]
fn program_image_fit_probe_rejects_undersized_storage() {
    let source = EffList::new();
    let columns = ProgramImageColumns::new(1, 0, 0);
    assert!(ProgramImageBytes::<10>::from_image_if_fits(&source, columns).is_none());
}

#[kani::proof]
#[kani::should_panic]
fn program_image_constructor_rejects_undersized_storage() {
    let source = EffList::new();
    let columns = ProgramImageColumns::new(1, 0, 0);
    let _ = ProgramImageBytes::<10>::from_image(&source, columns);
}

#[kani::proof]
fn scope_marker_identity_tag_is_exact_and_injective() {
    let left_event_raw = kani::any::<u8>() % 3;
    let right_event_raw = kani::any::<u8>() % 3;
    let left_reentry_raw = kani::any::<u8>() % 2;
    let right_reentry_raw = kani::any::<u8>() % 2;
    let event = |raw| match raw {
        0 => ScopeEvent::Enter,
        1 => ScopeEvent::Split,
        2 => ScopeEvent::Exit,
        _ => crate::invariant(),
    };
    let reentry = |raw| match raw {
        0 => ReentryMark::SinglePass,
        1 => ReentryMark::Reentrant,
        _ => crate::invariant(),
    };
    let left = scope_marker_identity_tag(event(left_event_raw), reentry(left_reentry_raw));
    let right = scope_marker_identity_tag(event(right_event_raw), reentry(right_reentry_raw));

    kani::cover!(left_event_raw == right_event_raw && left_reentry_raw == right_reentry_raw);
    kani::cover!(left_event_raw != right_event_raw);
    kani::cover!(left_event_raw == right_event_raw && left_reentry_raw != right_reentry_raw);
    assert!(
        (left == right)
            == (left_event_raw == right_event_raw && left_reentry_raw == right_reentry_raw)
    );
}

#[kani::proof]
fn packed_column_range_construction_is_exact_for_resident_stride_domain() {
    let offset: u16 = kani::any();
    let len: u16 = kani::any();
    let stride_index = kani::any::<u8>() % 9;
    let stride = match stride_index {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 5,
        4 => 6,
        5 => 7,
        6 => 8,
        7 => 10,
        8 => 11,
        _ => crate::invariant(),
    };
    let byte_len = usize::from(len) * stride;
    let valid = stride != 0 && byte_len <= usize::from(u16::MAX - offset);

    kani::cover!(valid);
    kani::cover!(!valid);
    kani::cover!(stride_index == 0);
    kani::cover!(stride_index == 1);
    kani::cover!(stride_index == 2);
    kani::cover!(stride_index == 3);
    kani::cover!(stride_index == 4);
    kani::cover!(stride_index == 5);
    kani::cover!(stride_index == 6);
    kani::cover!(stride_index == 7);
    kani::cover!(stride_index == 8);
    if valid {
        let program = ProgramColumnRange::new(usize::from(offset), usize::from(len), stride);
        let role = ColumnRange::new(usize::from(offset), usize::from(len), stride);
        assert!(program.offset == offset);
        assert!(program.len == len);
        assert!(program.byte_len(stride) == byte_len);
        assert!(program.end_offset(stride) == usize::from(offset) + byte_len);
        assert!(role.offset == offset);
        assert!(role.len == len);
        assert!(role.byte_len(stride) == byte_len);
        assert!(role.end_offset(stride) == usize::from(offset) + byte_len);
    }
}

#[kani::proof]
#[kani::should_panic]
fn compiled_program_column_range_rejects_stride_multiplication_overflow() {
    let _ = ProgramColumnRange::new(0, 2, usize::MAX);
}

#[kani::proof]
#[kani::should_panic]
fn role_image_column_range_rejects_stride_multiplication_overflow() {
    let _ = ColumnRange::new(0, 2, usize::MAX);
}

#[kani::proof]
fn compiled_program_blob_comparison_matches_array_equality() {
    let left_bytes: [u8; 16] = kani::any();
    let right_bytes: [u8; 16] = kani::any();
    let left_static: &'static [u8; 16] = unsafe {
        /* SAFETY: both arrays remain live until the two program refs have been
        compared and neither ref escapes this proof harness. */
        core::mem::transmute(&left_bytes)
    };
    let right_static: &'static [u8; 16] = unsafe {
        /* SAFETY: both arrays remain live until the two program refs have been
        compared and neither ref escapes this proof harness. */
        core::mem::transmute(&right_bytes)
    };
    let facts = ProgramImageFacts { role_count: 2 };
    let left = program(left_static, facts, identity_columns());
    let right = program(right_static, facts, identity_columns());
    let expected = left_bytes == right_bytes;

    kani::cover!(expected);
    kani::cover!(!expected);
    assert!(left.same_image(&right) == expected);
}

#[kani::proof]
fn compiled_program_image_identity_is_exact_over_facts_columns_and_blob() {
    let facts = ProgramImageFacts { role_count: 2 };
    let canonical = program(&CANONICAL_IMAGE, facts, route_columns());
    let same_image_at_another_address = program(&SAME_IMAGE, facts, route_columns());
    let different_facts = program(
        &SAME_IMAGE,
        ProgramImageFacts { role_count: 3 },
        route_columns(),
    );
    let different_columns = program(&SAME_IMAGE, facts, alternate_columns());
    let different_final_byte = program(&LAST_BYTE_DIFFERENT, facts, route_columns());

    assert!(canonical.columns.blob_len() == different_columns.columns.blob_len());
    kani::cover!(canonical.same_image(&same_image_at_another_address));
    kani::cover!(!canonical.same_image(&different_facts));
    kani::cover!(!canonical.same_image(&different_columns));
    kani::cover!(!canonical.same_image(&different_final_byte));
    assert!(canonical.same_image(&canonical));
    assert!(canonical.same_image(&same_image_at_another_address));
    assert!(same_image_at_another_address.same_image(&canonical));
    assert!(!canonical.same_image(&different_facts));
    assert!(!canonical.same_image(&different_columns));
    assert!(!canonical.same_image(&different_final_byte));
}

#[kani::proof]
fn program_atom_row_decoding_accepts_exact_domain() {
    let eff_idx: u16 = kani::any();
    let from: u8 = kani::any();
    let to: u8 = kani::any();
    let label: u8 = kani::any();
    let payload_schema: u32 = kani::any();
    let origin: u8 = kani::any();
    let lane: u8 = kani::any();
    let role_count: u8 = kani::any();

    let expected = (eff_idx as usize) < crate::eff::meta::MAX_EFF_NODES
        && role_count != 0
        && role_count <= crate::g::ROLE_DOMAIN_SIZE
        && from < role_count
        && to < role_count
        && origin <= 1;
    let decoded = ProgramAtomRow::decode(
        eff_idx,
        PackedProgramAtomFields {
            from,
            to,
            label,
            payload_schema,
            origin,
            lane,
        },
        role_count,
    );

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.eff_idx == eff_idx);
        assert!(row.atom.from == from);
        assert!(row.atom.to == to);
        assert!(row.atom.label == label);
        assert!(row.atom.payload_schema == payload_schema);
        assert!(row.atom.origin.packed_bits() == origin);
        assert!(row.atom.lane == lane);
    }
}

#[kani::proof]
fn compiled_program_atom_blob_decoding_preserves_every_schema_bit() {
    let payload_schema: u32 = kani::any();
    let bytes = [
        0,
        0,
        0,
        1,
        9,
        payload_schema as u8,
        (payload_schema >> 8) as u8,
        (payload_schema >> 16) as u8,
        (payload_schema >> 24) as u8,
        0,
        7,
    ];
    let bytes: &'static [u8; 11] = unsafe {
        /* SAFETY: the forged atom bytes remain live until the program ref has
        decoded the row and the ref does not escape this proof harness. */
        core::mem::transmute(&bytes)
    };
    let row = program(
        bytes,
        ProgramImageFacts { role_count: 2 },
        ProgramImageColumns::new(1, 0, 0),
    )
    .atom_at(0)
    .expect("valid symbolic atom");

    assert!(row.payload_schema == payload_schema);
}
