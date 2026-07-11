use super::{CompiledProgramRef, ProgramAtomRow};
use crate::global::compiled::images::image::columns::{
    ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};

const fn encoded_atom(eff_idx: u16, from: u8, to: u8, label: u8, origin: u8, lane: u8) -> [u8; 7] {
    [
        eff_idx as u8,
        (eff_idx >> 8) as u8,
        from,
        to,
        label,
        origin,
        lane,
    ]
}

fn forged_program_ref(bytes: &'static [u8; 7], role_count: u8) -> CompiledProgramRef {
    CompiledProgramRef::compact(ProgramImageFacts { role_count }, atom_columns(), bytes)
}

fn atom_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(1, 0, 0)
}

fn route_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 1, 0)
}

fn alternate_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 0, 1)
}

#[test]
fn compiled_program_column_range_rejects_stride_multiplication_overflow() {
    assert!(
        std::panic::catch_unwind(|| ProgramColumnRange::new(0, 2, usize::MAX)).is_err(),
        "packed program column construction must reject byte-length overflow"
    );
}

static VALID: [u8; 7] = encoded_atom(2, 0, 1, 9, 1, u8::MAX);
static VALID_COPY: [u8; 7] = encoded_atom(2, 0, 1, 9, 1, u8::MAX);
static LAST_BYTE_DIFFERENT: [u8; 7] = encoded_atom(2, 0, 1, 9, 1, 0);
static SAME_COLUMN_BYTES: [u8; 5] = [0; 5];
static EFF_INDEX_OUT_OF_RANGE: [u8; 7] =
    encoded_atom(crate::eff::meta::MAX_EFF_NODES as u16, 0, 1, 9, 1, 0);
static FROM_OUT_OF_RANGE: [u8; 7] = encoded_atom(2, 2, 1, 9, 1, 0);
static TO_OUT_OF_RANGE: [u8; 7] = encoded_atom(2, 0, 2, 9, 1, 0);
static ORIGIN_OUT_OF_RANGE: [u8; 7] = encoded_atom(2, 0, 1, 9, 2, 0);

#[test]
fn compiled_program_atom_descriptor_decodes_canonical_row() {
    let atom = forged_program_ref(&VALID, 2).atom_at(2).expect("atom row");
    assert_eq!(atom.from, 0);
    assert_eq!(atom.to, 1);
    assert_eq!(atom.label, 9);
    assert_eq!(atom.origin.packed_bits(), 1);
    assert_eq!(atom.lane, u8::MAX);
}

#[test]
fn compiled_program_image_identity_is_exact_over_facts_columns_and_blob() {
    let canonical = forged_program_ref(&VALID, 2);
    let same_image_at_another_address = forged_program_ref(&VALID_COPY, 2);
    let different_facts = forged_program_ref(&VALID_COPY, 3);
    let canonical_columns = CompiledProgramRef::compact(
        ProgramImageFacts { role_count: 2 },
        route_columns(),
        &SAME_COLUMN_BYTES,
    );
    let different_columns = CompiledProgramRef::compact(
        ProgramImageFacts { role_count: 2 },
        alternate_columns(),
        &SAME_COLUMN_BYTES,
    );
    let different_final_byte = forged_program_ref(&LAST_BYTE_DIFFERENT, 2);

    assert_eq!(
        canonical_columns.columns.blob_len(),
        different_columns.columns.blob_len()
    );
    assert!(canonical.same_image(&canonical));
    assert!(canonical.same_image(&same_image_at_another_address));
    assert!(same_image_at_another_address.same_image(&canonical));
    assert!(!canonical.same_image(&different_facts));
    assert!(!canonical_columns.same_image(&different_columns));
    assert!(!canonical.same_image(&different_final_byte));
}

#[test]
fn compiled_program_atom_decoder_rejects_exact_invalid_boundaries() {
    assert!(ProgramAtomRow::decode(0, 0, 0, 0, 0, 0, 1).is_some());
    assert!(
        ProgramAtomRow::decode(crate::eff::meta::MAX_EFF_NODES as u16, 0, 0, 0, 0, 0, 1,).is_none()
    );
    assert!(ProgramAtomRow::decode(0, 0, 0, 0, 2, 0, 1).is_none());
    assert!(ProgramAtomRow::decode(0, 1, 0, 0, 0, 0, 1).is_none());
    assert!(ProgramAtomRow::decode(0, 0, 1, 0, 0, 0, 1).is_none());
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_effect_index_out_of_range() {
    let _ = forged_program_ref(&EFF_INDEX_OUT_OF_RANGE, 2).atom_at(0);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_zero_role_count() {
    let _ = forged_program_ref(&VALID, 0).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_role_count_out_of_domain() {
    let _ = forged_program_ref(&VALID, crate::g::ROLE_DOMAIN_SIZE + 1).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_from_role_out_of_range() {
    let _ = forged_program_ref(&FROM_OUT_OF_RANGE, 2).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_to_role_out_of_range() {
    let _ = forged_program_ref(&TO_OUT_OF_RANGE, 2).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_origin_out_of_range() {
    let _ = forged_program_ref(&ORIGIN_OUT_OF_RANGE, 2).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_effect_index_query_out_of_range() {
    let _ = forged_program_ref(&VALID, 2).atom_at(crate::eff::meta::MAX_EFF_NODES);
}
