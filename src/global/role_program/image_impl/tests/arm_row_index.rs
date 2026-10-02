use super::super::metadata::{passive_parent_rows_are_coherent, route_scopes_are_sorted};
use super::*;
use std::{
    boxed::Box,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn projected_image() -> &'static RoleImageRef {
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

fn copy_bytes(rows: &RoleImageRef) -> [u8; 2048] {
    let mut bytes = [0; 2048];
    assert!(rows.columns.blob_len() <= bytes.len());
    for (idx, byte) in bytes.iter_mut().enumerate().take(rows.columns.blob_len()) {
        *byte = rows.blob.byte_at(idx);
    }
    bytes
}

fn outcome<T>(f: impl FnOnce() -> T) -> Result<T, ()> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| ())
}

fn compare_view(view: RoleLaneImage<'_>, slot: usize, arm: u8) {
    assert_eq!(
        outcome(|| view.certified_route_arm_event_row_by_slot(slot, arm).raw()),
        outcome(|| view.route_arm_event_row_by_slot(slot, arm).raw()),
    );
    assert_eq!(
        outcome(|| view.passive_arm_child_ordinal_with_certificate(slot, arm, true)),
        outcome(|| view.passive_arm_child_ordinal_by_slot(slot, arm)),
    );
}

#[test]
fn certified_arm_rows_match_original_with_full_query_guards() {
    let rows = projected_image();
    assert!(rows.has_passive_parent_index());
    for slot in (0..rows.columns.route_scopes.len as usize + 2).chain([usize::MAX]) {
        for arm in [0, 1, 2, u8::MAX] {
            compare_view(rows.lanes(), slot, arm);
            assert_eq!(
                outcome(|| rows.route_arm_event_row_by_slot(slot, arm).raw()),
                outcome(|| rows.lanes().route_arm_event_row_by_slot(slot, arm).raw()),
            );
            assert_eq!(
                outcome(|| rows.passive_arm_child_ordinal_by_slot(slot, arm)),
                outcome(|| rows.lanes().passive_arm_child_ordinal_by_slot(slot, arm)),
            );
        }
    }
}

#[test]
fn certified_arm_byte_mutations_match_exact_original_rows() {
    let rows = projected_image();
    let bytes = copy_bytes(rows);
    let columns = rows.columns;
    let mut certified = 0;
    let mut rejected = 0;
    for offset in columns.route_arms.offset as usize
        ..columns.route_arms.end_offset(ROLE_IMAGE_ROUTE_ARM_STRIDE)
    {
        for raw in [0, 1, 7, 0x7f, 0x80, 0xff] {
            let mut changed = bytes;
            changed[offset] = raw;
            if !route_scopes_are_sorted(&changed, columns.route_scopes)
                || !passive_parent_rows_are_coherent(&changed, columns)
            {
                rejected += 1;
                continue;
            }
            certified += 1;
            let immutable = Box::leak(Box::new(changed));
            let view = RoleLaneImage::new(
                &rows.columns,
                BlobPtr::from_array(immutable, columns.blob_len()),
            );
            for slot in 0..columns.route_scopes.len as usize {
                for arm in 0..2 {
                    compare_view(view, slot, arm);
                }
            }
        }
    }
    assert!(certified > 0 && rejected > 0);
}

#[test]
fn malformed_prefix_requires_fallback_and_later_damage_keeps_early_success() {
    let rows = projected_image();
    let mut bytes = copy_bytes(rows);
    let start = rows.columns.route_arms.offset as usize;
    // Corrupt only the reserved byte of row1: row0 must still return normally.
    bytes[start + ROLE_IMAGE_ROUTE_ARM_STRIDE + 7] = 1;
    assert!(!passive_parent_rows_are_coherent(&bytes, rows.columns));
    let immutable = Box::leak(Box::new(bytes));
    let view = RoleLaneImage::new(
        &rows.columns,
        BlobPtr::from_array(immutable, rows.columns.blob_len()),
    );
    assert!(outcome(|| view.route_arm_event_row_by_slot(0, 0)).is_ok());
    assert!(outcome(|| view.passive_arm_child_ordinal_by_slot(0, 0)).is_ok());
    assert_eq!(
        outcome(|| view.passive_arm_child_ordinal_with_certificate(0, 0, false)),
        outcome(|| view.passive_arm_child_ordinal_by_slot(0, 0)),
    );
    assert!(outcome(|| view.route_arm_event_row_by_slot(1, 0)).is_err());
    assert!(outcome(|| view.passive_arm_child_ordinal_with_certificate(1, 0, false)).is_err());
    // This deliberate guard-dropping mutant exposes the skipped predecessor.
    assert!(outcome(|| view.certified_route_arm_event_row_by_slot(1, 0)).is_ok());
}

#[test]
fn constructor_rejected_certificate_keeps_original_accessors() {
    let original = projected_image();
    let mut bytes = copy_bytes(original);
    let last = original.columns.route_scopes.len as usize - 1;
    let owner = original.columns.route_scope_conflicts.offset as usize + last * 2;
    bytes[owner..owner + 2].copy_from_slice(&0x8000u16.to_le_bytes());
    let immutable = Box::leak(Box::new(bytes));
    let rows = RoleImageRef::new(
        original.program,
        original.role,
        original.facts,
        original.columns,
        immutable,
    );
    assert!(!rows.has_passive_parent_index());
    for slot in 0..rows.columns.route_scopes.len as usize {
        for arm in 0..2 {
            assert_eq!(
                outcome(|| rows.route_arm_event_row_by_slot(slot, arm).raw()),
                outcome(|| rows.lanes().route_arm_event_row_by_slot(slot, arm).raw()),
            );
            assert_eq!(
                outcome(|| rows.passive_arm_child_ordinal_by_slot(slot, arm)),
                outcome(|| rows.lanes().passive_arm_child_ordinal_by_slot(slot, arm)),
            );
        }
    }
}
