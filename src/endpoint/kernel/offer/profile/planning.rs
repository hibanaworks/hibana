use super::{
    OfferControllerSkipEvidence, OfferControllerSkipReadiness, OfferPassiveEvidence,
    OfferPassiveReadiness, OfferScopeProfile,
};

impl OfferScopeProfile {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn controller_skip_readiness(
        self,
        evidence: OfferControllerSkipEvidence,
    ) -> OfferControllerSkipReadiness {
        match self {
            Self::ControllerStatic | Self::ControllerDynamic
                if evidence.materialization_pending() =>
            {
                OfferControllerSkipReadiness::BlockedByMaterialization
            }
            Self::ControllerStatic | Self::ControllerDynamic
                if evidence.non_entry_cursor_ready() =>
            {
                OfferControllerSkipReadiness::Ready
            }
            Self::ControllerStatic
            | Self::ControllerDynamic
            | Self::PassiveStatic
            | Self::PassiveDynamic => OfferControllerSkipReadiness::NeedsTransport,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn passive_readiness(
        self,
        evidence: OfferPassiveEvidence,
    ) -> OfferPassiveReadiness {
        match self {
            Self::PassiveStatic if evidence.has_ready_signal() => {
                OfferPassiveReadiness::ReadyArmOrFrameHint
            }
            Self::PassiveDynamic if evidence.has_ready_signal() => {
                OfferPassiveReadiness::ReadyArmOrFrameHint
            }
            Self::PassiveDynamic if evidence.dynamic_scope_without_recv() => {
                OfferPassiveReadiness::DynamicScopeWithoutRecv
            }
            Self::PassiveDynamic if evidence.ack_materializable() => {
                OfferPassiveReadiness::DynamicAckMaterializable
            }
            Self::ControllerStatic
            | Self::ControllerDynamic
            | Self::PassiveStatic
            | Self::PassiveDynamic => OfferPassiveReadiness::NeedsTransport,
        }
    }
}
