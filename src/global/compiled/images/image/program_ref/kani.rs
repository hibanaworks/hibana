use super::{CompiledProgramRef, ProgramAtomRow};
use crate::global::compiled::images::image::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
};
use crate::global::role_program::ColumnRange;

static CANONICAL_IMAGE: [u8; 7] = [2, 0, 0, 1, 9, 1, u8::MAX];
static SAME_IMAGE: [u8; 7] = [2, 0, 0, 1, 9, 1, u8::MAX];
static LAST_BYTE_DIFFERENT: [u8; 7] = [2, 0, 0, 1, 9, 1, 0];

fn atom_columns() -> ProgramImageColumns {
    ProgramImageColumns {
        atoms: ProgramColumnRange::new(0, 1, PROGRAM_IMAGE_ATOM_STRIDE),
        route_resolvers: ProgramColumnRange::new(
            PROGRAM_IMAGE_ATOM_STRIDE,
            0,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        ),
    }
}

fn alternate_columns_with_same_blob_len() -> ProgramImageColumns {
    ProgramImageColumns {
        atoms: ProgramColumnRange::new(0, 0, PROGRAM_IMAGE_ATOM_STRIDE),
        route_resolvers: ProgramColumnRange::new(2, 1, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE),
    }
}

fn program(
    bytes: &'static [u8; 7],
    facts: ProgramImageFacts,
    columns: ProgramImageColumns,
) -> CompiledProgramRef {
    CompiledProgramRef::compact(facts, columns, bytes)
}

#[kani::proof]
fn packed_column_range_construction_is_exact_for_resident_stride_domain() {
    let offset: u16 = kani::any();
    let len: u16 = kani::any();
    let stride_index = kani::any::<u8>() % 8;
    let stride = match stride_index {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 5,
        4 => 6,
        5 => 7,
        6 => 8,
        7 => 10,
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
    let left_bytes: [u8; 7] = kani::any();
    let right_bytes: [u8; 7] = kani::any();
    let left_static: &'static [u8; 7] = unsafe {
        /* SAFETY: both arrays remain live until the two program refs have been
        compared and neither ref escapes this proof harness. */
        core::mem::transmute(&left_bytes)
    };
    let right_static: &'static [u8; 7] = unsafe {
        /* SAFETY: both arrays remain live until the two program refs have been
        compared and neither ref escapes this proof harness. */
        core::mem::transmute(&right_bytes)
    };
    let facts = ProgramImageFacts { role_count: 2 };
    let left = program(left_static, facts, atom_columns());
    let right = program(right_static, facts, atom_columns());
    let expected = left_bytes == right_bytes;

    kani::cover!(expected);
    kani::cover!(!expected);
    assert!(left.same_image(&right) == expected);
}

#[kani::proof]
fn compiled_program_image_identity_is_exact_over_facts_columns_and_blob() {
    let facts = ProgramImageFacts { role_count: 2 };
    let canonical = program(&CANONICAL_IMAGE, facts, atom_columns());
    let same_image_at_another_address = program(&SAME_IMAGE, facts, atom_columns());
    let different_facts = program(
        &SAME_IMAGE,
        ProgramImageFacts { role_count: 3 },
        atom_columns(),
    );
    let different_columns = program(&SAME_IMAGE, facts, alternate_columns_with_same_blob_len());
    let different_final_byte = program(&LAST_BYTE_DIFFERENT, facts, atom_columns());

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
    let origin: u8 = kani::any();
    let lane: u8 = kani::any();
    let role_count: u8 = kani::any();

    let expected = (eff_idx as usize) < crate::eff::meta::MAX_EFF_NODES
        && role_count != 0
        && role_count <= crate::g::ROLE_DOMAIN_SIZE
        && from < role_count
        && to < role_count
        && origin <= 1;
    let decoded = ProgramAtomRow::decode(eff_idx, from, to, label, origin, lane, role_count);

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.eff_idx == eff_idx);
        assert!(row.atom.from == from);
        assert!(row.atom.to == to);
        assert!(row.atom.label == label);
        assert!(row.atom.origin.packed_bits() == origin);
        assert!(row.atom.lane == lane);
    }
}
