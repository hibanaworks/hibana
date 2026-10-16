use super::{
    super::CompiledProgramRef,
    fixtures::{COLUMNS, OWNERSHIP, PROGRAM, SOURCE, SUMMARY, encoded_bytes},
};
use crate::global::{
    compiled::images::{ProgramImageBytes, ProgramImagePlan},
    const_dsl::{ScopeId, ScopeKind},
};

#[test]
fn owned_image_uses_the_existing_eight_byte_columns_and_exact_tail() {
    assert_eq!(core::mem::size_of_val(&COLUMNS), 8);
    assert_eq!(PROGRAM.actor_binding_at(0), Some([0, 1]));
    assert_eq!(PROGRAM.actor_binding_at(1), Some([0, 2]));
    assert_eq!(PROGRAM.actor_binding_at(2), Some([256, 3]));
    assert_eq!(
        PROGRAM.actor_decision_at(0),
        Some((ScopeId::new(ScopeKind::Route, 0), [1, 2]))
    );
    for role in 0..=u8::MAX {
        assert_eq!(
            PROGRAM.initial_actor_for(role),
            if role == 255 { Some(0) } else { None }
        );
    }
    assert!(ProgramImageBytes::<8>::from_source_if_fits(&SOURCE, COLUMNS).is_none());
    assert_eq!(OWNERSHIP.initial.end_offset(2), COLUMNS.blob_len());
}

#[test]
#[should_panic]
fn owned_image_rejects_an_ordinary_header_without_actor_facts() {
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_program(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<128> =
        ProgramImageBytes::from_image(SOURCE.eff_list(), PLAN.columns());
    let _ = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
}

#[test]
#[should_panic]
fn owned_image_rejects_a_cyclic_decision_reference() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let left = OWNERSHIP.decisions.offset as usize + 2;
        bytes[left] = 0;
        bytes[left + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_the_reserved_reference() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let receiver = OWNERSHIP.bindings.offset as usize + 2;
        bytes[receiver] = 255;
        bytes[receiver + 1] = 255;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_an_unmapped_choice_scope() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[OWNERSHIP.decisions.offset as usize] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_an_initial_actor_different_from_the_first_sender() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[OWNERSHIP.initial.offset as usize + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_scope_counts_outside_its_storage() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let header = COLUMNS.ownership_header().offset as usize;
        bytes[header] = 255;
        bytes[header + 1] = 255;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_decision_counts_outside_its_storage() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[COLUMNS.ownership_header().offset as usize + 2] = 2;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}
