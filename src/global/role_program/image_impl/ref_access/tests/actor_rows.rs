use super::*;
use crate::Endpoint;
use crate::global::role_program::RoleLocalDirection;

// Check the event validator alone, so an unrelated index or layout rejection
// cannot conceal an accepted missing, duplicated, reordered or wrong-side row.
fn accepts(original: &RoleImageRef, selected: &[(usize, RoleLocalDirection)]) -> bool {
    let mut bytes = copy(original);
    let mut columns = *original.columns;
    let mut facts = original.facts;
    let stride = crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE;
    columns.events.offset = original.columns.blob_len() as u32;
    columns.lanes.offset = columns.events.offset + (selected.len() * stride) as u32;
    for (target, &(source, direction)) in selected.iter().enumerate() {
        for byte in 0..stride {
            bytes[columns.events.offset as usize + target * stride + byte] = original
                .blob
                .byte_at(original.columns.events.offset as usize + source * stride + byte);
        }
        let flags = &mut bytes[columns.events.offset as usize + target * stride + 8];
        *flags = (*flags & 1) | ((direction as u8) << 1);
        bytes[columns.lanes.offset as usize + target] = original
            .blob
            .byte_at(original.columns.lanes.offset as usize + source);
    }
    columns.events.len = selected.len() as u16;
    columns.lanes.len = selected.len() as u16;
    facts.words[RuntimeRoleFacts::LOCAL_STEP_COUNT] = selected.len() as u16;
    // SAFETY: neither reference leaves this call. Both backing values remain
    // stationary and alive until the checked image and the unwind have ended.
    let (columns, bytes) = unsafe {
        (
            &*core::ptr::from_ref(&columns),
            &*core::ptr::from_ref(&bytes),
        )
    };
    let image = RoleImageRef {
        program: original.program,
        role: original.role,
        facts,
        columns,
        blob: BlobPtr::from_array(bytes, bytes.len()),
        first_active_lane: original.first_active_lane,
        route_index_start: original.route_index_start,
        lookup_indexes: original.lookup_indexes,
    };
    catch_unwind(|| validation::ValidationRows::<32>::new(&image)).is_ok()
}

fn check_mutations(image: &RoleImageRef, expected: &[RoleLocalDirection]) {
    let original: std::vec::Vec<_> = expected.iter().copied().enumerate().collect();
    assert!(accepts(image, &original));
    for row in 0..original.len() {
        let mut missing = original.clone();
        missing.remove(row);
        assert!(!accepts(image, &missing), "missing row {row}");
        let mut duplicate = original.clone();
        duplicate.insert(row, original[row]);
        assert!(!accepts(image, &duplicate), "duplicated row {row}");
        for direction in [
            RoleLocalDirection::Local,
            RoleLocalDirection::Send,
            RoleLocalDirection::Recv,
        ] {
            if direction != original[row].1 {
                let mut wrong_side = original.clone();
                wrong_side[row].1 = direction;
                assert!(!accepts(image, &wrong_side), "wrong direction at row {row}");
            }
        }
        if row != 0 {
            let mut swapped = original.clone();
            swapped.swap(row - 1, row);
            assert!(!accepts(image, &swapped), "reordered row {row}");
        }
    }
}

#[test]
fn independent_actor_check_rejects_missing_extra_and_collapsed_operations() {
    use RoleLocalDirection::{Local, Recv, Send};
    type Loan = Msg<1, Endpoint<'static, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<2, 2, Msg<2, ()>>(),
            g::seq(g::send::<2, 1, Msg<3, u32>>(), g::send::<1, 0, Loan>()),
        ),
    );
    check_mutations(
        project::<1>(&global).role_image_ref(),
        &[Recv, Local, Send, Recv, Send],
    );
    check_mutations(project::<0>(&global).role_image_ref(), &[Send, Recv]);
}

#[test]
fn independent_actor_check_keeps_every_conditional_holder_and_both_facets() {
    use RoleLocalDirection::{Recv, Send};
    type Left = Msg<1, Endpoint<'static, 255>>;
    type Right = Msg<2, Endpoint<'static, 255>>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<0, 1, Left>(), g::send::<0, 2, Msg<3, ()>>()),
            g::seq(g::send::<0, 2, Right>(), g::send::<0, 1, Msg<4, ()>>()),
        )
        .resolve::<701>(),
        g::send::<255, 3, Msg<5, u32>>(),
    );
    check_mutations(
        project::<0>(&global).role_image_ref(),
        &[Send, Send, Send, Send],
    );
    check_mutations(project::<1>(&global).role_image_ref(), &[Recv, Recv, Send]);
    check_mutations(project::<2>(&global).role_image_ref(), &[Recv, Recv, Send]);
    check_mutations(project::<3>(&global).role_image_ref(), &[Recv]);
    assert!(accepts(project::<254>(&global).role_image_ref(), &[]));
}
