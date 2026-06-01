use super::super::ingress::OfferIngressMode;
use super::super::{
    OfferScopeProfile,
    profile::{
        OfferBindingProbeMode, OfferControllerReadiness, OfferCursorReadiness,
        OfferEarlyDecisionReadiness, OfferEntryPosition, OfferPassiveReadiness, OfferRouteShape,
    },
};

#[derive(Clone, Copy)]
pub(super) struct OfferIngressEvidence {
    pub(super) profile: OfferScopeProfile,
    pub(super) entry: OfferEntryPosition,
    pub(super) cursor: OfferCursorReadiness,
    pub(super) early_decision: OfferEarlyDecisionReadiness,
    pub(super) controller: OfferControllerReadiness,
    pub(super) passive: OfferPassiveReadiness,
    pub(super) binding_probe: OfferBindingProbeMode,
}

impl OfferIngressEvidence {
    #[inline]
    pub(super) const fn profile(self) -> OfferScopeProfile {
        self.profile
    }

    #[inline]
    pub(super) fn ingress_mode(self) -> OfferIngressMode {
        OfferRouteShape {
            profile: self.profile,
            entry: self.entry,
            cursor: self.cursor,
            early_decision: self.early_decision,
            controller: self.controller,
            passive: self.passive,
            binding_probe: self.binding_probe,
        }
        .ingress_mode()
    }
}
