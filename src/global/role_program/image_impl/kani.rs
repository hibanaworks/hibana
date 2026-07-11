use super::decode_binary_route_arm_index;

#[kani::proof]
fn resident_route_arm_index_decoding_accepts_exact_binary_domain() {
    let raw: u8 = kani::any();
    let decoded = decode_binary_route_arm_index(raw);

    assert!(decoded.is_some() == (raw <= 1));
    if let Some(index) = decoded {
        assert!(index == raw as usize);
    }
}
