use super::*;
use crate::{
    g::{self, Msg},
    global::{event_program::LocalEventProgram, role_program::RoleImageRef},
    runtime::program::project,
};
use std::{boxed::Box, panic::catch_unwind};

fn fixture() -> &'static RoleImageRef {
    let global = g::seq(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::route(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<5, ()>>(), g::send::<0, 1, Msg<6, ()>>()),
    );
    project::<1, _>(&global).role_image_ref()
}

fn machine(rows: &'static RoleImageRef) -> EventCursorMachine {
    EventCursorMachine {
        role: rows.role,
        event_program: LocalEventProgram::from_rows(rows),
    }
}

fn rebuild(
    original: &'static RoleImageRef,
    edit: impl FnOnce(&mut [u8; 2048]),
) -> &'static RoleImageRef {
    let mut bytes = [0u8; 2048];
    assert!(original.columns.blob_len() <= bytes.len());
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns.blob_len())
    {
        *byte = original.blob.byte_at(offset);
    }
    edit(&mut bytes);
    let immutable = Box::leak(Box::new(bytes));
    Box::leak(Box::new(RoleImageRef::new(
        original.program,
        original.role,
        original.facts,
        original.columns,
        immutable,
    )))
}

fn write_u16(bytes: &mut [u8; 2048], offset: usize, raw: u16) {
    bytes[offset..offset + 2].copy_from_slice(&raw.to_le_bytes());
}

fn assert_same_lookup(machine: &EventCursorMachine, query: ScopeId) {
    let new = catch_unwind(|| machine.passive_child_parent_route(query));
    let old = catch_unwind(|| machine.scanned_passive_child_parent_route(query));
    assert_eq!(new.is_err(), old.is_err());
    assert_eq!(new.ok(), old.ok());
}

#[test]
fn certified_parent_index_matches_legacy_for_every_decodable_scope() {
    let rows = fixture();
    assert!(rows.has_passive_parent_index());
    let machine = machine(rows);
    for raw in 0..=u16::MAX {
        if let Some(query) = ScopeId::decode_raw(raw) {
            assert_same_lookup(&machine, query);
        }
    }
}

#[test]
fn invalid_unused_owner_disables_new_read_without_changing_harmless_miss() {
    let original = fixture();
    let last = original.columns.route_scopes.len as usize - 1;
    let query = original.route_scope_by_slot(last).unwrap();
    let rows = rebuild(original, |bytes| {
        write_u16(
            bytes,
            original.columns.route_scope_conflicts.offset as usize + last * 2,
            0x8000,
        );
    });
    assert!(rows.has_sorted_route_index());
    assert!(!rows.has_passive_parent_index());
    let machine = machine(rows);
    assert_eq!(machine.passive_child_parent_route(query), None);
    assert_same_lookup(&machine, query);
    assert!(catch_unwind(|| machine.indexed_passive_child_parent_route(query)).is_err());
}

#[test]
fn malformed_later_edge_and_duplicate_target_preserve_earlier_match() {
    let original = fixture();
    let query = original.route_scope_by_slot(1).unwrap();
    let root = original.route_scope_by_slot(0).unwrap();
    for child in [0u16, 1] {
        let rows = rebuild(original, |bytes| {
            // Parent arm0 remains valid. Arm1 is backward or duplicates arm0.
            write_u16(
                bytes,
                original.columns.route_arms.offset as usize + 8 + 4,
                child,
            );
        });
        assert!(!rows.has_passive_parent_index());
        let machine = machine(rows);
        assert_eq!(machine.passive_child_parent_route(query), Some((root, 0)));
        assert_same_lookup(&machine, query);
        assert_same_lookup(&machine, ScopeId::route(8191));
    }
}

#[test]
fn wrong_owner_arm_disables_index_and_keeps_original_failure() {
    let original = fixture();
    let query = original.route_scope_by_slot(1).unwrap();
    let rows = rebuild(original, |bytes| {
        let offset = original.columns.route_scope_conflicts.offset as usize + 2;
        let raw = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        write_u16(bytes, offset, raw ^ 1);
    });
    assert!(!rows.has_passive_parent_index());
    let machine = machine(rows);
    assert!(catch_unwind(|| machine.passive_child_parent_route(query)).is_err());
    assert_same_lookup(&machine, query);
}

#[test]
fn passive_cycle_disables_index_and_preserves_ordered_legacy_outcomes() {
    let original = fixture();
    let root = original.route_scope_by_slot(0).unwrap();
    let child = original.route_scope_by_slot(1).unwrap();
    let rows = rebuild(original, |bytes| {
        write_u16(
            bytes,
            original.columns.route_arms.offset as usize + 2 * 8 + 4,
            0,
        );
        write_u16(
            bytes,
            original.columns.route_scope_conflicts.offset as usize,
            child.raw() << 1,
        );
    });
    assert!(!rows.has_passive_parent_index());
    let machine = machine(rows);
    assert_eq!(machine.passive_child_parent_route(child), Some((root, 0)));
    assert_same_lookup(&machine, child);
    assert_same_lookup(&machine, root);
}

#[test]
fn unused_cyclic_owners_do_not_create_passive_edges() {
    let original = fixture();
    let last = original.columns.route_scopes.len as usize - 1;
    let root = original.route_scope_by_slot(0).unwrap();
    let orphan = original.route_scope_by_slot(last).unwrap();
    let rows = rebuild(original, |bytes| {
        let start = original.columns.route_scope_conflicts.offset as usize;
        write_u16(bytes, start, orphan.raw() << 1);
        write_u16(bytes, start + last * 2, root.raw() << 1);
    });
    assert!(rows.has_passive_parent_index());
    let machine = machine(rows);
    for query in [root, orphan] {
        assert_eq!(machine.passive_child_parent_route(query), None);
        assert_same_lookup(&machine, query);
    }
}
