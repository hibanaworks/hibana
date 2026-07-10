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
    kani::assume(mask & !0b11 == 0);

    match (mask, Arm::from_single_ready_mask(mask)) {
        (0, None) | (3, None) => {}
        (1, Some(arm)) => assert!(arm == Arm::LEFT),
        (2, Some(arm)) => assert!(arm == Arm::RIGHT),
        _ => assert!(false),
    }
}
