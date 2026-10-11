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
