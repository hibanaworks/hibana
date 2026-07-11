use super::{CompiledProgramRef, ProgramAtomRow};
use crate::global::compiled::images::image::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
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
    let columns = ProgramImageColumns {
        atoms: ProgramColumnRange::new(0, 1, PROGRAM_IMAGE_ATOM_STRIDE),
        route_resolvers: ProgramColumnRange::new(
            PROGRAM_IMAGE_ATOM_STRIDE,
            0,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        ),
    };
    CompiledProgramRef::compact(ProgramImageFacts { role_count }, columns, bytes)
}

static VALID: [u8; 7] = encoded_atom(2, 0, 1, 9, 1, u8::MAX);
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
