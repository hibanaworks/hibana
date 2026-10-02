use super::super::metadata::{passive_parent_rows_are_coherent, route_scopes_are_sorted};
use super::*;

#[test]
fn certified_byte_mutations_preserve_every_skipped_fact_and_owner_read() {
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::route(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
    );
    let projected: RoleProgram<1> = project(&global);
    let original = projected.role_image_ref();
    let columns = original.columns;
    let mut bytes = [0u8; 512];
    assert!(columns.blob_len() <= bytes.len());
    for (offset, byte) in bytes.iter_mut().enumerate().take(columns.blob_len()) {
        *byte = original.blob.byte_at(offset);
    }
    assert!(passive_parent_rows_are_coherent(&bytes, columns));
    let mut certified = 0usize;
    let mut rejected = 0usize;
    for (column, stride) in [
        (columns.route_scopes, ROLE_IMAGE_ROUTE_SCOPE_STRIDE),
        (columns.route_scope_conflicts, ROLE_IMAGE_CONFLICT_STRIDE),
        (columns.route_arms, ROLE_IMAGE_ROUTE_ARM_STRIDE),
    ] {
        for offset in column.offset as usize..column.end_offset(stride) {
            for raw in [0u8, 1, 7, 0x7f, 0x80, 0xff] {
                let mut changed = bytes;
                changed[offset] = raw;
                if !(route_scopes_are_sorted(&changed, columns.route_scopes)
                    && passive_parent_rows_are_coherent(&changed, columns))
                {
                    rejected += 1;
                    continue;
                }
                certified += 1;
                let immutable = std::boxed::Box::leak(std::boxed::Box::new(changed));
                let view = RoleLaneImage::new(
                    &original.columns,
                    BlobPtr::from_array(immutable, columns.blob_len()),
                );
                for slot in 0..columns.route_scopes.len as usize {
                    assert!(view.route_scope_by_slot(slot).is_some());
                    let owner = view.route_scope_conflict_by_slot(slot);
                    assert!(owner.is_none() || owner.to_conflict().is_some());
                    for arm in 0..2 {
                        if let Some(child) = view.passive_arm_child_ordinal_by_slot(slot, arm) {
                            assert!(view.route_scope_slot(ScopeId::route(child)).is_some());
                        }
                    }
                }
            }
        }
    }
    assert!(
        certified > 0 && rejected > 0,
        "mutation test must exercise both outcomes"
    );
}
