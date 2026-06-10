use super::*;
#[test]
fn decision_policy_input_arg0_defaults_to_zero() {
    assert_eq!(
        decision_policy_input_arg0(crate::transport::context::PolicyInput::ZERO),
        0
    );
}
