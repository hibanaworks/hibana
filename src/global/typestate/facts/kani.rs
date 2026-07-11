use super::{LocalNode, PackedEventConflict};

#[kani::proof]
fn packed_event_conflict_decoding_accepts_exact_domain() {
    let raw: u16 = kani::any();
    let decoded = PackedEventConflict::decode_raw(raw);
    let valid = raw <= 0x3fff || raw == 0x6000 || raw == 0xffff;

    assert!(decoded.is_some() == valid);
    if let Some(conflict) = decoded {
        assert!(conflict.raw() == raw);
        match (raw <= 0x3fff, conflict.decoded_route()) {
            (true, Some((ordinal, arm))) => {
                assert!(ordinal == (raw >> 1) & 0x0fff);
                assert!(arm == (raw & 1) as u8);
            }
            (false, None) => {}
            _ => assert!(false),
        }
    }
}

#[kani::proof]
fn optional_route_arm_decoding_accepts_exact_domain() {
    let raw: u8 = kani::any();
    let decoded = LocalNode::decode_optional_route_arm_raw(raw);

    match (raw, decoded) {
        (0, Some(Some(0))) | (1, Some(Some(1))) | (255, Some(None)) => {}
        (2..=254, None) => {}
        _ => assert!(false),
    }
}
