use super::*;
use crate::{
    g,
    global::{compiled::images::RoleDescriptorRef, typestate::cursor::EventCursorState},
    runtime::program::project,
};
use core::mem::MaybeUninit;

fn check_receive_prefix(hint: u8, received: bool, arm: bool) {
    let global = g::seq(
        g::seq(
            g::send::<0, 1, g::Msg<10, ()>>(),
            g::send::<2, 1, g::Msg<11, ()>>(),
        ),
        g::route(
            g::send::<1, 0, g::Msg<12, ()>>(),
            g::send::<1, 0, g::Msg<13, ()>>(),
        )
        .resolve::<17>(),
    );
    let program = project::<1>(&global);
    let image = program.role_image_ref();
    let descriptor = RoleDescriptorRef::from_resident(image);
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut storage = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 3];
    let mut labels = [0u16; 3];
    let mut completed = [0u32; 8];
    assert!(descriptor.logical_lane_count() <= heads.len());
    assert!(image.footprint().completion_bit_count().div_ceil(32) <= completed.len());
    // SAFETY: the checked descriptor sections fit these disjoint arrays,
    // which remain stationary until the cursor's last use.
    unsafe {
        EventCursor::init_from_compiled(
            storage.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
    }
    // SAFETY: the production initializer wrote the cursor and its state.
    let cursor = unsafe { storage.assume_init_mut() };
    cursor.mark_local_event_done(0);
    if received {
        cursor.mark_local_event_done(1);
    }
    cursor.set_index(usize::from(hint) % cursor.machine().node_len());
    let meta = cursor.try_send_meta_at(2 + usize::from(arm)).unwrap();
    let mut calls = 0;
    let result = cursor.send_preview_meta_for_contract(
        MessageSelector::new(1, meta.label, meta.payload_kind),
        None,
        &mut |_| None,
        &mut |_| {
            assert!(received, "resolver ran before its required receive");
            calls += 1;
            Some(u8::from(arm))
        },
        &mut |_| None,
        &mut |_, _, _| meta.lane,
    );
    if received {
        assert_eq!(result.unwrap().0.label, meta.label);
        assert_eq!(calls, 1);
    } else {
        assert!(matches!(result, Err(SendPreviewError::OutOfOrder)));
        assert_eq!(calls, 0);
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn lookup_hints_cannot_bypass_a_resolvers_receive_prefix() {
    for hint in 0..=u8::MAX {
        for received in [false, true] {
            for arm in [false, true] {
                check_receive_prefix(hint, received, arm);
            }
        }
    }
}
