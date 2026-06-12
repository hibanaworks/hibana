use core::mem::size_of;
use std::vec::Vec;

use super::*;
use crate::control::cap::mint::LocalControlKind;
use crate::observe::ids;

const fn control_op_is_idempotent(op: ControlOp) -> bool {
    matches!(op, ControlOp::TopologyAck | ControlOp::StateSnapshot)
}

const fn control_op_requires_gen_bump(op: ControlOp) -> bool {
    matches!(op, ControlOp::TopologyCommit | ControlOp::TxCommit)
}

const fn control_op_is_terminal(op: ControlOp) -> bool {
    matches!(op, ControlOp::TxCommit | ControlOp::TxAbort)
}

const fn control_op_modifies_history(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::StateSnapshot | ControlOp::StateRestore | ControlOp::TxAbort
    )
}

#[test]
fn resource_descriptor_stays_compact() {
    assert!(
        size_of::<ResourceDescriptor>() <= 16,
        "ResourceDescriptor regressed to a wide unused-field layout: {} bytes",
        size_of::<ResourceDescriptor>()
    );
}

#[test]
fn control_op_tap_and_property_tables_match_expected_semantics() {
    assert_eq!(
        control_op_tap_event_id(ControlOp::TopologyAck),
        ids::TOPOLOGY_ACK
    );
    assert_eq!(
        control_op_tap_event_id(ControlOp::TxAbort),
        ids::POLICY_TX_ABORT
    );

    assert!(control_op_is_idempotent(ControlOp::TopologyAck));
    assert!(!control_op_is_idempotent(ControlOp::TopologyBegin));
    assert!(control_op_is_idempotent(ControlOp::StateSnapshot));
    assert!(!control_op_is_idempotent(ControlOp::StateRestore));

    assert!(control_op_requires_gen_bump(ControlOp::TopologyCommit));
    assert!(!control_op_requires_gen_bump(ControlOp::TopologyBegin));

    assert!(control_op_is_terminal(ControlOp::TxCommit));
    assert!(control_op_is_terminal(ControlOp::TxAbort));

    assert!(control_op_modifies_history(ControlOp::StateSnapshot));
    assert!(control_op_modifies_history(ControlOp::StateRestore));
    assert!(control_op_modifies_history(ControlOp::TxAbort));
    assert!(!control_op_modifies_history(ControlOp::TxCommit));
}

#[test]
fn pure_program_has_no_effect_resources_or_control_scopes() {
    let program = crate::g::send::<0, 1, crate::g::Msg<1, ()>>();
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = *projected.role_image_ref().program;
    let effects = program.effect_envelope();
    assert_eq!(effects.resources().count(), 0);
    assert_eq!(effects.control_scopes().count(), 0);
}

#[test]
fn self_send_control_atom_projects_to_resource_descriptor() {
    let program =
        crate::g::send::<0, 0, crate::g::ControlMsg<125, crate::g::control::StateSnapshot>>();
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = *projected.role_image_ref().program;
    let effects = program.effect_envelope();
    let resources: Vec<_> = effects.resources().collect();
    assert_eq!(resources.len(), 1);
    assert_eq!(
        resources[0].tag(),
        <crate::g::control::StateSnapshot as LocalControlKind>::TAG
    );
    assert_eq!(resources[0].control.op(), ControlOp::StateSnapshot);
}

#[test]
fn wire_control_atom_projects_to_resource_descriptor() {
    let program =
        crate::g::send::<0, 1, crate::g::ControlMsg<20, crate::g::control::TopologyBegin>>();
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = *projected.role_image_ref().program;
    let effects = program.effect_envelope();
    let resources: Vec<_> = effects.resources().collect();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].tag(), 0x50);
    assert_eq!(resources[0].control.op(), ControlOp::TopologyBegin);
}

#[test]
fn app_msg_and_control_msg_with_same_label_stay_separate() {
    let program = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<42, ()>>(),
        crate::g::send::<0, 1, crate::g::ControlMsg<42, crate::g::control::TopologyBegin>>(),
    );
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = *projected.role_image_ref().program;
    let app = program.atom_at(0).expect("app atom");
    let control = program.atom_at(1).expect("control atom");

    assert_eq!(app.label, 42);
    assert!(!app.is_control);
    assert_eq!(app.resource, None);

    assert_eq!(control.label, 42);
    assert!(control.is_control);
    assert_eq!(control.resource, Some(0x50));
}
