use super::{compact_cursor_position, local_event_bit};

#[kani::proof]
fn compact_cursor_position_covers_the_full_u16_value_domain() {
    let position: usize = kani::any();
    let encoded = compact_cursor_position(position);

    assert_eq!(encoded.is_some(), position <= u16::MAX as usize);
    if let Some(encoded) = encoded {
        assert_eq!(usize::from(encoded), position);
    }
}

#[kani::proof]
fn local_event_bit_exists_exactly_inside_the_local_step_domain() {
    let step_idx: usize = kani::any();
    let local_step_count: usize = kani::any();
    let located = local_event_bit(step_idx, local_step_count);

    assert_eq!(located.is_some(), step_idx < local_step_count);
    if let Some((word_idx, bit)) = located {
        assert_eq!(word_idx, step_idx / u32::BITS as usize);
        assert_eq!(bit, 1u32 << (step_idx % u32::BITS as usize));
        assert_ne!(bit, 0);
    }
}

/// The actual cursor mutation functions record every mark and clear, even
/// when the bit already has the requested value. No send/publication double
/// supplies these counts. Descriptor setup is concrete; both target events
/// and both mutation kinds are symbolic for each of the two writes.
#[kani::proof]
#[kani::unwind(20)]
fn actual_completion_writes_are_observed_even_when_the_bits_repeat() {
    use super::{EventCursor, EventCursorState};
    use crate::{g, global::compiled::images::RoleDescriptorRef};
    use core::mem::MaybeUninit;

    let program = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::send::<0, 1, g::Msg<2, ()>>(),
    );
    let projected = crate::runtime::program::project::<0>(&program);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 1];
    let mut labels = [0u16; 1];
    let mut completed = [0u32; 1];
    // SAFETY: the concrete two-step descriptor requires one lane/word. All
    // backing allocations stay stationary and disjoint for the whole proof.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
    }
    // SAFETY: the preceding production initializer initialized every field.
    let cursor = unsafe { cursor.assume_init_mut() };
    assert_eq!(cursor.completion_mutations(), 0);
    let mut expected = 0u32;
    for count in 0..2 {
        let event = usize::from(kani::any::<u8>() % 2);
        let bit = 1u32 << event;
        if kani::any::<bool>() {
            cursor.mark_local_event_done(event);
            expected |= bit;
        } else {
            cursor.clear_local_event_done(event);
            expected &= !bit;
        }
        assert_eq!(cursor.completed_event_words(), &[expected]);
        assert_eq!(cursor.completion_mutations(), count + 1);
    }
    kani::cover!(expected == 0 && cursor.completion_mutations() == 2);
}
