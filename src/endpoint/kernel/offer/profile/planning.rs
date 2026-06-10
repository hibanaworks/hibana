use super::{
    OfferControllerSkipEvidence, OfferControllerSkipReadiness, OfferPassiveEvidence,
    OfferPassiveReadiness, OfferScopeProfile,
};

#[derive(Clone, Copy)]
enum OfferControllerSkipPlan {
    NonEntryCursorReady,
    BlockedByMaterialization,
    NeedsTransport,
}

impl OfferControllerSkipPlan {
    #[inline]
    const fn readiness(self) -> OfferControllerSkipReadiness {
        match self {
            Self::NonEntryCursorReady => OfferControllerSkipReadiness::Ready,
            Self::BlockedByMaterialization => {
                OfferControllerSkipReadiness::BlockedByMaterialization
            }
            Self::NeedsTransport => OfferControllerSkipReadiness::NeedsTransport,
        }
    }
}

#[derive(Clone, Copy)]
enum OfferPassivePlan {
    ReadyArmOrFrameHint,
    DynamicScopeWithoutRecv,
    DynamicAckMaterializable,
    NeedsTransport,
}

impl OfferPassivePlan {
    #[inline]
    const fn readiness(self) -> OfferPassiveReadiness {
        match self {
            Self::ReadyArmOrFrameHint => OfferPassiveReadiness::ReadyArmOrFrameHint,
            Self::DynamicScopeWithoutRecv => OfferPassiveReadiness::DynamicScopeWithoutRecv,
            Self::DynamicAckMaterializable => OfferPassiveReadiness::DynamicAckMaterializable,
            Self::NeedsTransport => OfferPassiveReadiness::NeedsTransport,
        }
    }
}

impl OfferScopeProfile {
    #[inline]
    const fn controller_skip_plan(
        self,
        evidence: OfferControllerSkipEvidence,
    ) -> OfferControllerSkipPlan {
        match self {
            Self::ControllerStatic | Self::ControllerDynamic
                if evidence.materialization_pending() =>
            {
                OfferControllerSkipPlan::BlockedByMaterialization
            }
            Self::ControllerStatic | Self::ControllerDynamic
                if evidence.non_entry_cursor_ready() =>
            {
                OfferControllerSkipPlan::NonEntryCursorReady
            }
            Self::ControllerStatic
            | Self::ControllerDynamic
            | Self::PassiveStatic
            | Self::PassiveDynamic => OfferControllerSkipPlan::NeedsTransport,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn controller_skip_readiness(
        self,
        evidence: OfferControllerSkipEvidence,
    ) -> OfferControllerSkipReadiness {
        self.controller_skip_plan(evidence).readiness()
    }

    #[inline]
    const fn passive_plan(self, evidence: OfferPassiveEvidence) -> OfferPassivePlan {
        match self {
            Self::PassiveStatic if evidence.has_ready_signal() => {
                OfferPassivePlan::ReadyArmOrFrameHint
            }
            Self::PassiveDynamic if evidence.has_ready_signal() => {
                OfferPassivePlan::ReadyArmOrFrameHint
            }
            Self::PassiveDynamic if evidence.dynamic_scope_without_recv() => {
                OfferPassivePlan::DynamicScopeWithoutRecv
            }
            Self::PassiveDynamic if evidence.ack_materializable() => {
                OfferPassivePlan::DynamicAckMaterializable
            }
            Self::ControllerStatic
            | Self::ControllerDynamic
            | Self::PassiveStatic
            | Self::PassiveDynamic => OfferPassivePlan::NeedsTransport,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn passive_readiness(
        self,
        evidence: OfferPassiveEvidence,
    ) -> OfferPassiveReadiness {
        self.passive_plan(evidence).readiness()
    }
}
