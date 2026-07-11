use super::decode_binary_route_arm_index;

#[test]
fn resident_route_arm_index_decoder_accepts_exact_binary_domain() {
    assert_eq!(decode_binary_route_arm_index(0), Some(0));
    assert_eq!(decode_binary_route_arm_index(1), Some(1));
    for raw in 2..=u8::MAX {
        assert_eq!(decode_binary_route_arm_index(raw), None);
    }
}
