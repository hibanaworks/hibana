use core::mem::size_of;
use std::vec::Vec;

use super::*;
use crate::control::cap::mint::LocalControlKind;
use crate::observe::ids;
use crate::test_support::snapshot_control::{SNAPSHOT_CONTROL_LOGICAL, SnapshotControl};

const fn control_op_is_idempotent(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::TopologyAck | ControlOp::StateSnapshot | ControlOp::Fence | ControlOp::AbortAck
    )
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
    assert!(!control_op_is_terminal(ControlOp::Fence));

    assert!(control_op_modifies_history(ControlOp::StateSnapshot));
    assert!(control_op_modifies_history(ControlOp::StateRestore));
    assert!(control_op_modifies_history(ControlOp::TxAbort));
    assert!(!control_op_modifies_history(ControlOp::TxCommit));
}

#[test]
fn fence_performs_no_state_mutation() {
    assert!(!control_op_requires_gen_bump(ControlOp::Fence));
    assert!(!control_op_is_terminal(ControlOp::Fence));
    assert!(!control_op_modifies_history(ControlOp::Fence));
}

#[test]
fn pure_program_has_no_effect_resources_or_control_scopes() {
    let program = crate::g::send::<0, 1, crate::g::Msg<1, ()>>();
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = projected.role_image_ref().program;
    let effects = program.effect_envelope();
    assert_eq!(effects.resources().count(), 0);
    assert_eq!(effects.control_scopes().count(), 0);
}

#[test]
fn self_send_control_atom_projects_to_resource_descriptor() {
    let program = crate::g::send::<
        0,
        0,
        crate::g::ControlMsg<{ SNAPSHOT_CONTROL_LOGICAL }, SnapshotControl>,
    >();
    let projected: crate::integration::program::RoleProgram<0> =
        crate::integration::program::project(&program);
    let program = projected.role_image_ref().program;
    let effects = program.effect_envelope();
    let resources: Vec<_> = effects.resources().collect();
    assert_eq!(resources.len(), 1);
    assert_eq!(
        resources[0].tag(),
        <SnapshotControl as LocalControlKind>::TAG
    );
    assert_eq!(resources[0].control.op(), ControlOp::StateSnapshot);
}
