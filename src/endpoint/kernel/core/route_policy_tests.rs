use super::*;
#[test]
fn route_policy_input_arg0_defaults_to_zero() {
    assert_eq!(
        route_policy_input_arg0(crate::transport::context::PolicyInput::ZERO),
        0
    );
}

#[test]
fn route_policy_input_arg0_reads_arg0() {
    assert_eq!(
        route_policy_input_arg0(crate::transport::context::PolicyInput::from_primary(
            0xABCD_1234
        )),
        0xABCD_1234
    );
}

#[test]
fn route_policy_enforces_scope_match_before_route_handle() {
    let scope = ScopeId::generic(12);
    let err = validate_route_decision_scope(scope, ScopeId::generic(13))
        .expect_err("scope mismatch must fail");
    assert!(matches!(err, SendError::PhaseInvariant));
}

#[test]
fn route_policy_rejects_empty_scope() {
    let err = validate_route_decision_scope(ScopeId::none(), ScopeId::none())
        .expect_err("route scope is required");
    assert!(matches!(err, SendError::PhaseInvariant));
}

#[test]
fn route_policy_allows_static_route_scope_without_policy_scope() {
    let scope = ScopeId::generic(18);
    validate_route_decision_scope(scope, ScopeId::none())
        .expect("static route scope should remain valid without policy scope");
    validate_route_decision_scope(scope, scope)
        .expect("dynamic route scope should match policy scope");
}
