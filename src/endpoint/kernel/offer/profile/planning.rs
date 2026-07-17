use super::{
    OfferControllerLocalEvidence, OfferControllerLocalReadiness, OfferPassiveEvidence,
    OfferPassiveReadiness, OfferScopeProfile,
};

impl OfferScopeProfile {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn controller_local_readiness(
        self,
        evidence: OfferControllerLocalEvidence,
    ) -> OfferControllerLocalReadiness {
        match self {
            Self::ControllerIntrinsic | Self::ControllerDynamic
                if evidence.materialization_pending() =>
            {
                OfferControllerLocalReadiness::BlockedByMaterialization
            }
            Self::ControllerIntrinsic | Self::ControllerDynamic
                if evidence.non_entry_cursor_ready() =>
            {
                OfferControllerLocalReadiness::Ready
            }
            Self::ControllerIntrinsic
            | Self::ControllerDynamic
            | Self::PassiveIntrinsic
            | Self::PassiveDynamic => OfferControllerLocalReadiness::NeedsTransport,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn passive_readiness(
        self,
        evidence: OfferPassiveEvidence,
    ) -> OfferPassiveReadiness {
        match self {
            Self::PassiveIntrinsic if evidence.has_ready_signal() => {
                OfferPassiveReadiness::ReadyArm
            }
            Self::PassiveDynamic if evidence.has_ready_signal() => OfferPassiveReadiness::ReadyArm,
            Self::PassiveDynamic if evidence.dynamic_scope_without_recv() => {
                OfferPassiveReadiness::DynamicScopeWithoutRecv
            }
            Self::ControllerIntrinsic
            | Self::ControllerDynamic
            | Self::PassiveIntrinsic
            | Self::PassiveDynamic => OfferPassiveReadiness::NeedsTransport,
        }
    }
}
