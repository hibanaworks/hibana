use super::unique_controller_role;

#[kani::proof]
fn unique_controller_role_accepts_exact_single_bit_domain() {
    let mask = kani::any::<u16>();
    let role = unique_controller_role(mask);
    let exact = mask != 0 && (mask & (mask - 1)) == 0;

    assert_eq!(role.is_some(), exact);
    if let Some(role) = role {
        assert!(role < u16::BITS as u8);
        assert_eq!(mask, 1u16 << role);
    }
}
