use super::Arm;

#[kani::proof]
fn route_arm_decoding_accepts_exact_binary_domain() {
    let raw: u8 = kani::any();
    let decoded = Arm::decode_raw(raw);

    assert!(decoded.is_some() == (raw <= 1));
    if let Some(arm) = decoded {
        assert!(arm.as_u8() == raw);
    }
}

#[kani::proof]
fn single_ready_mask_decoding_is_exact() {
    let mask: u8 = kani::any();
    let expected = match mask {
        0 => Some(None),
        1 => Some(Some(Arm::LEFT)),
        2 => Some(Some(Arm::RIGHT)),
        3..=u8::MAX => None,
    };

    assert!(Arm::decode_single_ready_mask(mask) == expected);
    if let Some(expected) = expected {
        assert!(Arm::from_single_ready_mask(mask) == expected);
    }
}
