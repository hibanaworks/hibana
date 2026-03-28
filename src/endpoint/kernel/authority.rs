//! Authority-path helpers for resolver/ack/poll decisions.

#[cfg(test)]
use crate::control::cap::resource_kinds::RouteDecisionHandle;
use crate::{
    endpoint::{SendError, SendResult},
    epf::{AbortInfo, Action},
    global::const_dsl::ScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoutePolicyDecision {
    RouteArm(u8),
    DelegateResolver,
    Abort(u16),
    Defer { retry_hint: u8, source: DeferSource },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferSource {
    Epf,
    Resolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferReason {
    Unsupported = 1,
    NoEvidence = 2,
}

#[inline]
pub(super) fn route_policy_input_arg0(input: &[u32; 4]) -> u32 {
    input[0]
}

#[inline]
pub(super) fn route_policy_decision_from_action(
    action: Action,
    policy_id: u16,
) -> RoutePolicyDecision {
    match action {
        Action::Route { arm } if arm <= 1 => RoutePolicyDecision::RouteArm(arm),
        Action::Route { .. } => RoutePolicyDecision::Abort(policy_id),
        Action::Abort(AbortInfo { reason, .. }) => RoutePolicyDecision::Abort(reason),
        Action::Defer { retry_hint } => RoutePolicyDecision::Defer {
            retry_hint,
            source: DeferSource::Epf,
        },
        Action::Proceed | Action::Tap { .. } => RoutePolicyDecision::DelegateResolver,
    }
}

#[inline]
pub(super) fn validate_route_decision_scope(
    scope: ScopeId,
    policy_scope: ScopeId,
) -> SendResult<()> {
    if scope.is_none() {
        return Err(SendError::PhaseInvariant);
    }
    if !policy_scope.is_none() && scope != policy_scope {
        return Err(SendError::PhaseInvariant);
    }
    Ok(())
}

#[inline]
#[cfg(test)]
pub(super) fn resolve_route_decision_handle_with_policy<F>(
    scope: ScopeId,
    policy_scope: ScopeId,
    policy_decision: RoutePolicyDecision,
    delegate_resolver: F,
) -> SendResult<RouteDecisionHandle>
where
    F: FnOnce() -> SendResult<RouteDecisionHandle>,
{
    validate_route_decision_scope(scope, policy_scope)?;
    match policy_decision {
        RoutePolicyDecision::RouteArm(arm) => Ok(RouteDecisionHandle { scope, arm }),
        RoutePolicyDecision::Abort(reason) => Err(SendError::PolicyAbort { reason }),
        RoutePolicyDecision::Defer { .. } => delegate_resolver(),
        RoutePolicyDecision::DelegateResolver => delegate_resolver(),
    }
}
