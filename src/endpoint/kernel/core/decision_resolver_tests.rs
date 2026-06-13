use super::*;
#[test]
fn empty_resolver_audit_input_is_explicit() {
    assert_eq!(resolver_audit::EMPTY_RESOLVER_INPUT_ARG0, 0);
    assert_eq!(resolver_audit::EMPTY_RESOLVER_INPUT_WORDS, [0, 0, 0, 0]);
    assert_eq!(resolver_audit::hash_empty_resolver_input(), 0x4B95_F515);
}
