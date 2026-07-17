use super::{InboundFrameKey, LocalNode, MAX_STATES, PackedEventConflict, StateIndex};

#[test]
fn state_index_uses_the_exact_present_identity_domain() {
    assert_eq!(
        StateIndex::from_usize(MAX_STATES - 1).as_usize(),
        MAX_STATES - 1
    );
    assert!(!StateIndex::from_usize(MAX_STATES - 1).is_absent());
    assert_eq!(
        StateIndex::checked_from_usize(MAX_STATES - 1)
            .expect("last present state")
            .as_usize(),
        MAX_STATES - 1
    );
    assert!(StateIndex::checked_from_usize(MAX_STATES).is_none());
}

#[test]
#[should_panic]
fn state_index_rejects_the_reserved_absent_identity() {
    let _ = StateIndex::from_usize(MAX_STATES);
}

#[test]
fn inbound_frame_key_is_exact_three_byte_identity() {
    let key = InboundFrameKey::new(253, 254, 255);

    assert_eq!(core::mem::size_of::<InboundFrameKey>(), 3);
    assert_eq!(key.source_role, 253);
    assert_eq!(key.lane, 254);
    assert_eq!(key.frame_label, 255);
    assert!(key.matches_parts(253, 254, 255));
    assert!(!key.matches_parts(252, 254, 255));
    assert!(!key.matches_parts(253, 253, 255));
    assert!(!key.matches_parts(253, 254, 254));
}

#[test]
fn packed_conflict_and_optional_arm_decoders_accept_exact_domains() {
    for raw in 0..=u16::MAX {
        let route_ordinal = (raw >> 1) & 0x1fff;
        let route = raw < 0x8000;
        let expected = route || raw == 0xc000 || raw == 0xffff;
        let decoded = PackedEventConflict::decode_raw(raw);
        assert_eq!(decoded.is_some(), expected);
        if route {
            assert_eq!(
                decoded.and_then(PackedEventConflict::decoded_route),
                Some((route_ordinal, (raw & 1) as u8))
            );
        } else if expected {
            assert_eq!(decoded.and_then(PackedEventConflict::decoded_route), None);
        }
    }

    for raw in 0..=u8::MAX {
        let expected = match raw {
            0 => Some(Some(0)),
            1 => Some(Some(1)),
            2..=254 => None,
            255 => Some(None),
        };
        assert_eq!(LocalNode::decode_optional_route_arm_raw(raw), expected);
    }
}

#[test]
#[should_panic]
fn packed_conflict_rejects_reserved_high_bit() {
    let _ = PackedEventConflict::from_raw(0x8001);
}

#[test]
#[should_panic]
fn optional_route_arm_rejects_invalid_compact_value() {
    let _ = LocalNode::decode_route_arm(2);
}
