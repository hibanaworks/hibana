use super::{CompiledProgramRef, PackedProgramAtomFields, ProgramAtomRow};
use crate::eff::{EffAtom, EventOrigin};
use crate::global::{
    compiled::images::image::{
        blob_storage::ProgramImageBytes,
        columns::{
            PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY, PROGRAM_IMAGE_ATOM_STRIDE, ProgramColumnRange,
            ProgramImageColumns, ProgramImageFacts,
        },
    },
    const_dsl::EffList,
};

const ATOM_ONLY_IMAGE_BYTES: usize =
    PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY * PROGRAM_IMAGE_ATOM_STRIDE;

const fn maximum_atom_only_source() -> EffList<PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY> {
    let mut source = EffList::new();
    let atom = EffAtom {
        from: 0,
        to: 1,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    };
    let mut event = 0usize;
    while event < PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY {
        source.push_event_mut(atom);
        event += 1;
    }
    source
}

static MAXIMUM_ATOM_ONLY_SOURCE: EffList<PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY> =
    maximum_atom_only_source();
static MAXIMUM_ATOM_ONLY_IMAGE: ProgramImageBytes<ATOM_ONLY_IMAGE_BYTES> =
    ProgramImageBytes::from_image(
        &MAXIMUM_ATOM_ONLY_SOURCE,
        ProgramImageColumns::new(PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY, 0, 0, 0),
    );

const fn encoded_atom(
    eff_idx: u16,
    from: u8,
    to: u8,
    label: u8,
    payload_schema: u32,
    origin: u8,
    lane: u8,
) -> [u8; 11] {
    [
        eff_idx as u8,
        (eff_idx >> 8) as u8,
        from,
        to,
        label,
        payload_schema as u8,
        (payload_schema >> 8) as u8,
        (payload_schema >> 16) as u8,
        (payload_schema >> 24) as u8,
        origin,
        lane,
    ]
}

const fn encoded_atom_rows(eff_indices: [u16; 3]) -> [u8; 33] {
    let mut bytes = [0u8; 33];
    let mut row = 0usize;
    while row < eff_indices.len() {
        let encoded = encoded_atom(eff_indices[row], 0, 0, row as u8, 0, 0, 0);
        let mut offset = 0usize;
        while offset < encoded.len() {
            bytes[row * PROGRAM_IMAGE_ATOM_STRIDE + offset] = encoded[offset];
            offset += 1;
        }
        row += 1;
    }
    bytes
}

fn forged_program_ref(bytes: &'static [u8; 11], max_role: u8) -> CompiledProgramRef {
    CompiledProgramRef::compact(ProgramImageFacts { max_role }, atom_columns(), bytes)
}

fn atom_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(1, 0, 0, 0)
}

fn route_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(0, 1, 3, 0)
}

fn alternate_columns() -> ProgramImageColumns {
    ProgramImageColumns::new(1, 0, 0, 0)
}

#[test]
fn compiled_program_column_range_rejects_stride_multiplication_overflow() {
    assert!(
        std::panic::catch_unwind(|| ProgramColumnRange::new(0, 2, usize::MAX)).is_err(),
        "packed program column construction must reject byte-length overflow"
    );
}

#[test]
fn atom_only_source_and_image_reach_the_compact_byte_ceiling() {
    assert_eq!(MAXIMUM_ATOM_ONLY_SOURCE.len(), 5_957);
    assert_eq!(ATOM_ONLY_IMAGE_BYTES, 65_527);
    assert_eq!(
        core::mem::size_of_val(&MAXIMUM_ATOM_ONLY_IMAGE),
        ATOM_ONLY_IMAGE_BYTES
    );
}

#[test]
fn program_image_fit_probe_rejects_undersized_storage() {
    let source = EffList::<1>::new();
    let columns = ProgramImageColumns::new(1, 0, 0, 0);
    assert!(ProgramImageBytes::<10>::from_image_if_fits(&source, columns).is_none());
}

#[test]
#[should_panic]
fn program_image_constructor_rejects_undersized_storage() {
    let source = EffList::<1>::new();
    let columns = ProgramImageColumns::new(1, 0, 0, 0);
    let _ = ProgramImageBytes::<10>::from_image(&source, columns);
}

const VALID_SCHEMA: u32 = 0x7856_3412;
static VALID: [u8; 11] = encoded_atom(2, 0, 1, 9, VALID_SCHEMA, 1, u8::MAX);
static VALID_COPY: [u8; 11] = encoded_atom(2, 0, 1, 9, VALID_SCHEMA, 1, u8::MAX);
static SCHEMA_DIFFERENT: [u8; 11] = encoded_atom(2, 0, 1, 9, 0x7856_3413, 1, u8::MAX);
static LAST_BYTE_DIFFERENT: [u8; 11] = encoded_atom(2, 0, 1, 9, VALID_SCHEMA, 1, 0);
static SAME_COLUMN_BYTES: [u8; 11] = [0; 11];
static EFF_INDEX_OUT_OF_RANGE: [u8; 11] = encoded_atom(u16::MAX, 0, 1, 9, VALID_SCHEMA, 1, 0);
static FROM_OUT_OF_RANGE: [u8; 11] = encoded_atom(2, 2, 1, 9, VALID_SCHEMA, 1, 0);
static TO_OUT_OF_RANGE: [u8; 11] = encoded_atom(2, 0, 2, 9, VALID_SCHEMA, 1, 0);
static ORIGIN_OUT_OF_RANGE: [u8; 11] = encoded_atom(2, 0, 1, 9, VALID_SCHEMA, 2, 0);

#[test]
fn compiled_program_atom_descriptor_decodes_canonical_row() {
    let atom = forged_program_ref(&VALID, 1).atom_at(2).expect("atom row");
    assert_eq!(atom.from, 0);
    assert_eq!(atom.to, 1);
    assert_eq!(atom.label, 9);
    assert_eq!(atom.payload_schema, VALID_SCHEMA);
    assert_eq!(atom.origin.packed_bits(), 1);
    assert_eq!(atom.lane, u8::MAX);
}

#[test]
fn compiled_program_atom_lookup_is_exact_for_sparse_sorted_rows() {
    static SORTED: [u8; 33] = encoded_atom_rows([1, 257, 4096]);
    let image = CompiledProgramRef::compact(
        ProgramImageFacts { max_role: 0 },
        ProgramImageColumns::new(3, 0, 0, 0),
        &SORTED,
    );

    assert_eq!(image.atom_at(0), None);
    assert_eq!(image.atom_at(1).map(|atom| atom.label), Some(0));
    assert_eq!(image.atom_at(256), None);
    assert_eq!(image.atom_at(257).map(|atom| atom.label), Some(1));
    assert_eq!(image.atom_at(4095), None);
    assert_eq!(image.atom_at(4096).map(|atom| atom.label), Some(2));
    assert_eq!(image.atom_at(4097), None);
}

#[test]
#[should_panic]
fn compiled_program_descriptor_rejects_noncanonical_atom_order() {
    static UNSORTED: [u8; 33] = encoded_atom_rows([1, 4096, 257]);
    let _ = CompiledProgramRef::compact(
        ProgramImageFacts { max_role: 0 },
        ProgramImageColumns::new(3, 0, 0, 0),
        &UNSORTED,
    );
}

#[test]
fn compiled_program_image_identity_is_exact_over_facts_columns_and_blob() {
    let canonical = forged_program_ref(&VALID, 1);
    let same_image_at_another_address = forged_program_ref(&VALID_COPY, 1);
    let different_facts = forged_program_ref(&VALID_COPY, 2);
    let canonical_columns = CompiledProgramRef::compact(
        ProgramImageFacts { max_role: 1 },
        route_columns(),
        &SAME_COLUMN_BYTES,
    );
    let different_columns = CompiledProgramRef::compact(
        ProgramImageFacts { max_role: 1 },
        alternate_columns(),
        &SAME_COLUMN_BYTES,
    );
    let different_schema = forged_program_ref(&SCHEMA_DIFFERENT, 1);
    let different_final_byte = forged_program_ref(&LAST_BYTE_DIFFERENT, 1);

    assert_eq!(
        canonical_columns.columns.blob_len(),
        different_columns.columns.blob_len()
    );
    assert!(canonical.same_image(&canonical));
    assert!(canonical.same_image(&same_image_at_another_address));
    assert!(same_image_at_another_address.same_image(&canonical));
    assert!(!canonical.same_image(&different_facts));
    assert!(!canonical_columns.same_image(&different_columns));
    assert!(!canonical.same_image(&different_schema));
    assert!(!canonical.same_image(&different_final_byte));
}

#[test]
fn compiled_program_atom_decoder_rejects_exact_invalid_boundaries() {
    let fields = |from, to, origin| PackedProgramAtomFields {
        from,
        to,
        label: 0,
        payload_schema: 0,
        origin,
        lane: 0,
    };
    assert!(ProgramAtomRow::decode(0, fields(0, 0, 0), 0).is_some());
    assert!(ProgramAtomRow::decode(u16::MAX, fields(0, 0, 0), 0,).is_none());
    assert!(ProgramAtomRow::decode(0, fields(0, 0, 2), 0).is_none());
    assert!(ProgramAtomRow::decode(0, fields(1, 0, 0), 0).is_none());
    assert!(ProgramAtomRow::decode(0, fields(0, 1, 0), 0).is_none());
    assert!(ProgramAtomRow::decode(0, fields(u8::MAX, u8::MAX, 0), u8::MAX).is_some());
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_effect_index_out_of_range() {
    let _ = forged_program_ref(&EFF_INDEX_OUT_OF_RANGE, 1).atom_at(0);
}

#[test]
fn compiled_program_atom_descriptor_accepts_full_u8_role_domain() {
    static HIGH: [u8; 11] = encoded_atom(2, u8::MAX, u8::MAX, 9, VALID_SCHEMA, 1, 0);
    let atom = forged_program_ref(&HIGH, u8::MAX)
        .atom_at(2)
        .expect("role 255 atom");
    assert_eq!((atom.from, atom.to), (u8::MAX, u8::MAX));
    assert_eq!(forged_program_ref(&HIGH, u8::MAX).role_count(), 256);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_from_role_out_of_range() {
    let _ = forged_program_ref(&FROM_OUT_OF_RANGE, 1).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_to_role_out_of_range() {
    let _ = forged_program_ref(&TO_OUT_OF_RANGE, 1).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_origin_out_of_range() {
    let _ = forged_program_ref(&ORIGIN_OUT_OF_RANGE, 1).atom_at(2);
}

#[test]
#[should_panic]
fn compiled_program_atom_descriptor_rejects_effect_index_query_out_of_range() {
    let _ =
        forged_program_ref(&VALID, 1).atom_at(crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY);
}
