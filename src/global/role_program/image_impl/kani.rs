use super::{
    decode_binary_route_arm_index,
    event_rows::decode_resident_event_header,
    lane_image::{decode_resident_roll_scope, decode_resident_route_scope},
};

#[kani::proof]
fn resident_route_arm_index_decoding_accepts_exact_binary_domain() {
    let raw: u8 = kani::any();
    let decoded = decode_binary_route_arm_index(raw);

    assert!(decoded.is_some() == (raw <= 1));
    if let Some(index) = decoded {
        assert!(index == raw as usize);
    }
}

#[kani::proof]
fn resident_route_scope_decoding_accepts_exact_domain() {
    let raw: u16 = kani::any();
    let decoded = decode_resident_route_scope(raw);
    let expected = (raw as usize) < crate::eff::meta::MAX_EFF_NODES;

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.raw() == raw);
        assert!(scope.local_ordinal() == raw);
    }
}

#[kani::proof]
fn resident_roll_scope_decoding_accepts_exact_domain() {
    let raw_ordinal: u16 = kani::any();
    let decoded = decode_resident_roll_scope(raw_ordinal);
    let expected = (raw_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES;

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.local_ordinal() == raw_ordinal);
    }
}

#[kani::proof]
fn resident_event_header_decoding_accepts_exact_domain() {
    let eff_index: u16 = kani::any();
    let scope_raw: u16 = kani::any();
    let flags: u8 = kani::any();
    let scope_ordinal = scope_raw & 0x1fff;
    let scope_kind = (scope_raw >> 13) & 0b11;
    let scope_valid = scope_raw == u16::MAX
        || ((scope_raw & 0x8000) == 0
            && scope_kind <= 2
            && (scope_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES);
    let expected =
        (eff_index as usize) < crate::eff::meta::MAX_EFF_NODES && flags <= 1 && scope_valid;
    let decoded = decode_resident_event_header(eff_index, scope_raw, flags);

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.raw() == scope_raw);
    }
}
