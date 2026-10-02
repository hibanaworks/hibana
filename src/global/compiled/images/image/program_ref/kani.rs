use super::{CompiledProgramRef, decode_program_atom};
use crate::global::compiled::images::image::blob_storage::{
    DescriptorScopeEvent, ProgramImageBytes, erase_scope_event, scope_marker_identity_tag,
};
use crate::global::compiled::images::image::columns::{
    COMPACT_DESCRIPTOR_BYTE_CAPACITY, PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY,
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
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
static DENSE_ATOM_ROWS: [u8; 27] = atom_rows();

fn route_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 0, 27, 0)
}

fn identity_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 0, 16, 0)
}

fn alternate_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 1, 19, 0)
}

fn program<const N: usize>(
    bytes: &'static [u8; N],
    facts: ProgramImageFacts,
    columns: ProgramImageColumns,
) -> CompiledProgramRef {
    CompiledProgramRef::compact(facts, columns, bytes)
}

const fn atom_rows() -> [u8; 27] {
    let mut bytes = [0u8; 27];
    let mut row = 0;
    while row < 3 {
        bytes[row * PROGRAM_IMAGE_ATOM_STRIDE + 2] = row as u8;
        row += 1;
    }
    bytes
}

#[kani::proof]
fn program_image_columns_are_canonical_for_exact_count_domain() {
    let atom_len: usize = kani::any();
    let route_resolver_len: usize = kani::any();
    let route_participant_len: usize = kani::any();
    let scope_marker_len: usize = kani::any();
    let counts_fit = atom_len <= PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY
        && route_resolver_len <= COMPACT_DESCRIPTOR_BYTE_CAPACITY
        && route_participant_len <= COMPACT_DESCRIPTOR_BYTE_CAPACITY
        && scope_marker_len <= COMPACT_DESCRIPTOR_BYTE_CAPACITY;
    let valid = counts_fit
        && atom_len * PROGRAM_IMAGE_ATOM_STRIDE
            + route_resolver_len * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE
            + route_participant_len * PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE
            + scope_marker_len * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE
            <= COMPACT_DESCRIPTOR_BYTE_CAPACITY;
    let source_resolver_len: usize = kani::any();
    let checked = ProgramImageColumns::try_new(
        atom_len,
        route_resolver_len,
        route_participant_len,
        scope_marker_len,
    );

    assert_eq!(checked.is_some(), valid);
    kani::cover!(valid);
    kani::cover!(!valid && counts_fit);
    kani::cover!(!valid && atom_len > PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY);
    kani::cover!(!valid && route_resolver_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY);
    kani::cover!(!valid && route_participant_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY);
    kani::cover!(!valid && scope_marker_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY);
    if let Some(columns) = checked {
        let atom_bytes = atom_len * PROGRAM_IMAGE_ATOM_STRIDE;
        let route_resolver_bytes = route_resolver_len * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
        let route_participant_bytes =
            route_participant_len * PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE;
        let scope_marker_bytes = scope_marker_len * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
        let route_resolver_offset = atom_bytes;
        let route_participant_offset = route_resolver_offset + route_resolver_bytes;
        let scope_marker_offset = route_participant_offset + route_participant_bytes;
        let blob_len = scope_marker_offset + scope_marker_bytes;

        assert!(columns.atoms().offset == 0);
        assert!(usize::from(columns.atoms().len) == atom_len);
        assert!(columns.route_resolvers().offset as usize == route_resolver_offset);
        assert!(usize::from(columns.route_resolvers().len) == route_resolver_len);
        assert!(columns.route_participants().offset as usize == route_participant_offset);
        assert!(usize::from(columns.route_participants().len) == route_participant_len);
        assert!(columns.scope_markers().offset as usize == scope_marker_offset);
        assert!(usize::from(columns.scope_markers().len) == scope_marker_len);
        assert!(columns.blob_len() == blob_len);

        kani::cover!(source_resolver_len <= route_resolver_len);
        kani::cover!(source_resolver_len > route_resolver_len);
        assert_eq!(
            columns.covers_source_counts(atom_len, scope_marker_len, source_resolver_len,),
            source_resolver_len <= route_resolver_len,
        );
        assert!(
            !columns.covers_source_counts(atom_len + 1, scope_marker_len, source_resolver_len,)
        );
        assert!(
            !columns.covers_source_counts(atom_len, scope_marker_len + 1, source_resolver_len,)
        );
        assert!(!columns.covers_source_counts(atom_len, scope_marker_len, route_resolver_len + 1,));
    }
}

#[kani::proof]
#[kani::should_panic]
fn program_image_columns_reject_first_total_byte_overflow() {
    let route_resolver_len = usize::from(u16::MAX) / PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE + 1;
    let _ = ProgramImageColumns::new(0, route_resolver_len, 0, 0);
}

#[kani::proof]
fn program_image_fit_probe_rejects_undersized_storage() {
    let source = EffList::<1>::new();
    let columns = ProgramImageColumns::new(1, 0, 0, 0);
    assert!(ProgramImageBytes::<8>::from_image_if_fits(&source, columns).is_none());
}

#[kani::proof]
#[kani::should_panic]
fn program_image_constructor_rejects_undersized_storage() {
    let source = EffList::<1>::new();
    let columns = ProgramImageColumns::new(1, 0, 0, 0);
    let _ = ProgramImageBytes::<8>::from_image(&source, columns);
}

#[kani::proof]
fn descriptor_scope_marker_tag_is_exact_and_injective() {
    let left_event: DescriptorScopeEvent = kani::any();
    let right_event: DescriptorScopeEvent = kani::any();
    let left_reentry: ReentryMark = kani::any();
    let right_reentry: ReentryMark = kani::any();
    let left = scope_marker_identity_tag(left_event, left_reentry);
    let right = scope_marker_identity_tag(right_event, right_reentry);

    kani::cover!(left_event == right_event && left_reentry == right_reentry);
    kani::cover!(left_event != right_event);
    kani::cover!(left_event == right_event && left_reentry != right_reentry);
    assert!((left == right) == (left_event == right_event && left_reentry == right_reentry));
}

#[kani::proof]
fn proof_only_scope_entry_metadata_is_erased_from_descriptor_tags() {
    let reentry: ReentryMark = kani::any();
    let expected = scope_marker_identity_tag(erase_scope_event(ScopeEvent::roll_enter()), reentry);

    assert!(
        scope_marker_identity_tag(erase_scope_event(ScopeEvent::route_enter(2)), reentry)
            == expected
    );
    assert!(
        scope_marker_identity_tag(
            erase_scope_event(ScopeEvent::route_arm_continuation()),
            reentry,
        ) == expected
    );
    assert!(
        scope_marker_identity_tag(erase_scope_event(ScopeEvent::parallel_enter(1)), reentry,)
            == expected
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
        8 => 9,
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
    let facts = ProgramImageFacts { max_role: 1 };
    let left = program(left_static, facts, identity_columns());
    let right = program(right_static, facts, identity_columns());
    let expected = left_bytes == right_bytes;

    kani::cover!(expected);
    kani::cover!(!expected);
    assert!(left.same_image(&right) == expected);
}

#[kani::proof]
fn compiled_program_image_identity_is_exact_over_facts_columns_and_blob() {
    let facts = ProgramImageFacts { max_role: 1 };
    let canonical = program(&CANONICAL_IMAGE, facts, route_columns());
    let same_image_at_another_address = program(&SAME_IMAGE, facts, route_columns());
    let different_facts = program(
        &SAME_IMAGE,
        ProgramImageFacts { max_role: 2 },
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
    let bytes: [u8; 9] = kani::any();
    let max_role: u8 = kani::any();
    let expected = bytes[0] <= max_role && bytes[1] <= max_role && bytes[7] <= 1;
    let decoded = decode_program_atom(bytes, max_role);
    assert!(decoded.is_some() == expected);
    if let Some(atom) = decoded {
        assert!(atom.from == bytes[0]);
        assert!(atom.to == bytes[1]);
        assert!(atom.label == bytes[2]);
        assert!(
            atom.payload_schema == u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]])
        );
        assert!(atom.origin.packed_bits() == bytes[7]);
        assert!(atom.lane == bytes[8]);
    }
}

#[kani::proof]
fn compiled_program_atom_lookup_is_exact_for_dense_rows() {
    let image = program(
        &DENSE_ATOM_ROWS,
        ProgramImageFacts { max_role: 0 },
        ProgramImageColumns::new(3, 0, 0, 0),
    );
    let query: u16 = kani::any();
    let index = usize::from(query) % crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY;
    assert!(
        image.atom_at(index).map(|atom| atom.label)
            == if index < 3 { Some(index as u8) } else { None }
    );
    kani::cover!(index < 3);
    kani::cover!(index >= 3);
}

#[kani::proof]
#[kani::should_panic]
fn compiled_program_atom_constructor_rejects_invalid_roles() {
    static INVALID: [u8; 9] = [1, 0, 0, 0, 0, 0, 0, 0, 0];
    let _ = program(
        &INVALID,
        ProgramImageFacts { max_role: 0 },
        ProgramImageColumns::new(1, 0, 0, 0),
    );
}

#[kani::proof]
fn compiled_program_atom_blob_decoding_preserves_every_schema_bit() {
    let payload_schema: u32 = kani::any();
    let bytes = [
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
    let bytes: &'static [u8; 9] = unsafe {
        /* SAFETY: the forged atom bytes remain live until the program ref has
        decoded the row and the ref does not escape this proof harness. */
        core::mem::transmute(&bytes)
    };
    let row = program(
        bytes,
        ProgramImageFacts { max_role: 1 },
        ProgramImageColumns::new(1, 0, 0, 0),
    )
    .atom_at(0)
    .expect("valid symbolic atom");

    assert!(row.payload_schema == payload_schema);
}
