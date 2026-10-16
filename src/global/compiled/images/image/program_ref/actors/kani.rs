use super::super::CompiledProgramRef;
use super::fixtures::{COLUMNS, OWNERSHIP, PROGRAM, encoded_bytes};
use crate::global::const_dsl::{ScopeId, ScopeKind};

#[kani::proof]
#[kani::unwind(4)]
fn selected_actor_preserves_identity_and_requires_the_actual_choice() {
    let actor: u8 = kani::any();
    let right: bool = kani::any();
    assert_eq!(
        PROGRAM.selected_actor(u16::from(actor), &mut |_| None),
        Some(actor)
    );
    assert_eq!(
        PROGRAM.selected_actor(256, &mut |scope| {
            assert_eq!(scope, ScopeId::route(0));
            Some(u8::from(right))
        }),
        Some(if right { 2 } else { 1 })
    );
    assert_eq!(PROGRAM.selected_actor(256, &mut |_| None), None);
}

#[kani::proof]
#[kani::unwind(4)]
fn initial_issuance_lookup_preserves_the_complete_role_domain() {
    let role: u8 = kani::any();
    assert_eq!(
        PROGRAM.initial_actor_for(role),
        if role == 255 { Some(0) } else { None }
    );
}

#[kani::proof]
fn actor_row_lookups_reject_every_out_of_range_index() {
    let index: usize = kani::any();
    let expected = match index {
        0 => Some([0, 1]),
        1 => Some([0, 2]),
        2 => Some([256, 3]),
        _ => None,
    };
    assert_eq!(PROGRAM.actor_binding_at(index), expected);
    assert_eq!(
        PROGRAM.actor_decision_at(index),
        if index == 0 {
            Some((ScopeId::new(ScopeKind::Route, 0), [1, 2]))
        } else {
            None
        }
    );
}

#[kani::proof]
#[kani::should_panic]
// Atom decoding reads all nine bytes before it can reject the decision.
#[kani::unwind(16)]
fn owned_image_construction_rejects_a_self_referencing_decision() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let left = OWNERSHIP.decisions.offset as usize + 2;
        bytes[left] = 0;
        bytes[left + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}
