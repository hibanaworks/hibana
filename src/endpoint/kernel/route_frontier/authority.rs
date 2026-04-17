//! Authority-path helpers for resolver/ack/poll decisions.

#[cfg(test)]
use crate::control::cap::resource_kinds::RouteDecisionHandle;
use crate::{
    endpoint::{SendError, SendResult},
    global::const_dsl::ScopeId,
    policy_runtime::{AbortInfo, Action},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDecision {
    Continue,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arm(u8);

impl Arm {
    #[inline]
    pub(super) const fn new(value: u8) -> Option<Self> {
        if value <= 1 { Some(Self(value)) } else { None }
    }

    #[inline]
    pub(super) const fn as_u8(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeHint(u8);

impl ScopeHint {
    #[inline]
    pub(super) const fn new(label: u8) -> Option<Self> {
        if label == 0 { None } else { Some(Self(label)) }
    }

    #[inline]
    pub(super) const fn label(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteDecisionSource {
    Ack,
    Resolver,
    Poll,
}

impl RouteDecisionSource {
    #[inline]
    pub(super) const fn as_tap_seq(self) -> u8 {
        match self {
            Self::Ack => 1,
            Self::Resolver => 2,
            Self::Poll => 3,
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) const fn from_tap_seq(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Ack),
            2 => Some(Self::Resolver),
            3 => Some(Self::Poll),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteDecisionToken {
    arm: Arm,
    source: RouteDecisionSource,
}

impl RouteDecisionToken {
    #[inline]
    pub(super) const fn from_ack(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Ack,
        }
    }

    #[inline]
    pub(super) const fn from_resolver(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Resolver,
        }
    }

    #[inline]
    pub(super) const fn from_poll(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Poll,
        }
    }

    #[inline]
    pub(super) const fn arm(self) -> Arm {
        self.arm
    }

    #[inline]
    pub(super) const fn source(self) -> RouteDecisionSource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteResolveStep {
    Resolved(Arm),
    Deferred { retry_hint: u8, source: DeferSource },
    Abort(u16),
}

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
    if let Some(arm) = action.route_arm() {
        return if arm <= 1 {
            RoutePolicyDecision::RouteArm(arm)
        } else {
            RoutePolicyDecision::Abort(policy_id)
        };
    }
    if let Some(AbortInfo { reason, .. }) = action.abort_info() {
        return RoutePolicyDecision::Abort(reason);
    }
    if let Some(retry_hint) = action.defer_hint() {
        return RoutePolicyDecision::Defer {
            retry_hint,
            source: DeferSource::Epf,
        };
    }
    RoutePolicyDecision::DelegateResolver
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
