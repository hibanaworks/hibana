//! Typed offer route-shape profile and ingress planning.

use super::{FrameHintIngestion, OfferScopeSelection, RouteArmToken, ingress::OfferIngressMode};

mod evidence;
mod planning;

pub(super) use self::evidence::{
    OfferArmRecvEvidence, OfferControllerCursorArm, OfferControllerLocalEvidence,
    OfferMaterializationReadiness, OfferPassiveAckEvidence, OfferPassiveEvidence,
    OfferPassiveReadySignal, OfferPassiveRecvEvidence,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::endpoint::kernel) enum OfferScopeProfile {
    ControllerIntrinsic,
    ControllerDynamic,
    PassiveIntrinsic,
    PassiveDynamic,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OfferAuthorityPath {
    ControllerResolver,
    PassiveEvidence,
    LocalSources,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) enum OfferEntryPosition {
    RouteEntry,
    AfterRouteEntry,
}

impl OfferEntryPosition {
    #[inline]
    pub(in crate::endpoint::kernel) const fn is_route_entry(self) -> bool {
        matches!(self, Self::RouteEntry)
    }
}

#[derive(Clone, Copy)]
pub(super) enum OfferCursorReadiness {
    Recv,
    NonRecv,
}

#[derive(Clone, Copy)]
pub(super) enum OfferEarlyDecisionReadiness {
    Unavailable,
    AvailableWithRecv,
    AvailableWithoutRecv,
}

impl OfferEarlyDecisionReadiness {
    #[inline]
    const fn available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }

    #[inline]
    const fn arm_has_no_recv(self) -> bool {
        matches!(self, Self::AvailableWithoutRecv)
    }
}

#[derive(Clone, Copy)]
pub(super) enum OfferControllerLocalReadiness {
    Ready,
    BlockedByMaterialization,
    NeedsTransport,
}

#[derive(Clone, Copy)]
pub(super) enum OfferPassiveReadiness {
    ReadyArmOrFrameHint,
    DynamicScopeWithoutRecv,
    DynamicAckMaterializable,
    NeedsTransport,
}

#[derive(Clone, Copy)]
pub(super) struct OfferRouteShape {
    pub(super) profile: OfferScopeProfile,
    pub(super) entry: OfferEntryPosition,
    pub(super) cursor: OfferCursorReadiness,
    pub(super) early_decision: OfferEarlyDecisionReadiness,
    pub(super) controller: OfferControllerLocalReadiness,
    pub(super) passive: OfferPassiveReadiness,
}

impl OfferRouteShape {
    #[inline]
    pub(super) const fn ingress_mode(self) -> OfferIngressMode {
        match self.profile {
            OfferScopeProfile::ControllerIntrinsic | OfferScopeProfile::ControllerDynamic => {
                self.controller_ingress_mode()
            }
            OfferScopeProfile::PassiveIntrinsic | OfferScopeProfile::PassiveDynamic => {
                self.passive_ingress_mode()
            }
        }
    }

    #[inline]
    const fn controller_ingress_mode(self) -> OfferIngressMode {
        if self.controller_resolved_without_frame() || self.early_decision.arm_has_no_recv() {
            return OfferIngressMode::ResolvedWithoutFrame;
        }
        OfferIngressMode::TransportFrame
    }

    #[inline]
    const fn passive_ingress_mode(self) -> OfferIngressMode {
        if self.passive_resolved_without_frame() || self.early_decision.arm_has_no_recv() {
            return OfferIngressMode::ResolvedWithoutFrame;
        }
        OfferIngressMode::TransportFrame
    }

    #[inline]
    const fn controller_resolved_without_frame(self) -> bool {
        if matches!(
            self.controller,
            OfferControllerLocalReadiness::BlockedByMaterialization
        ) {
            return false;
        }
        match (self.entry, self.cursor) {
            (OfferEntryPosition::RouteEntry, _) => {
                self.profile.is_dynamic()
                    || matches!(self.controller, OfferControllerLocalReadiness::Ready)
                    || self.early_decision.available()
            }
            (OfferEntryPosition::AfterRouteEntry, OfferCursorReadiness::NonRecv) => true,
            (OfferEntryPosition::AfterRouteEntry, OfferCursorReadiness::Recv) => false,
        }
    }

    #[inline]
    const fn passive_resolved_without_frame(self) -> bool {
        matches!(
            self.passive,
            OfferPassiveReadiness::ReadyArmOrFrameHint
                | OfferPassiveReadiness::DynamicScopeWithoutRecv
                | OfferPassiveReadiness::DynamicAckMaterializable
        )
    }
}

impl OfferScopeProfile {
    #[inline]
    pub(super) const fn is_controller(self) -> bool {
        matches!(self, Self::ControllerIntrinsic | Self::ControllerDynamic)
    }

    #[inline]
    pub(super) const fn is_passive(self) -> bool {
        !self.is_controller()
    }

    #[inline]
    pub(super) const fn is_dynamic(self) -> bool {
        matches!(self, Self::ControllerDynamic | Self::PassiveDynamic)
    }

    #[inline]
    pub(super) const fn frame_hint_ingestion(self) -> FrameHintIngestion {
        if self.is_dynamic() {
            FrameHintIngestion::Dynamic
        } else {
            FrameHintIngestion::Scope
        }
    }

    #[inline]
    pub(super) const fn authority_path_after_ack_miss(self) -> OfferAuthorityPath {
        match self {
            Self::ControllerDynamic => OfferAuthorityPath::ControllerResolver,
            Self::PassiveIntrinsic | Self::PassiveDynamic => OfferAuthorityPath::PassiveEvidence,
            Self::ControllerIntrinsic => OfferAuthorityPath::LocalSources,
        }
    }

    #[inline]
    pub(super) const fn transport_marks_ready_from_source(self, token: RouteArmToken) -> bool {
        match self {
            Self::PassiveDynamic => token.is_ack() || token.is_poll(),
            Self::ControllerDynamic => token.is_resolver() || token.is_poll(),
            Self::ControllerIntrinsic | Self::PassiveIntrinsic => false,
        }
    }

    #[inline]
    pub(super) const fn keeps_current_scope_for_unready_resolver(
        self,
        selection: OfferScopeSelection,
        token: RouteArmToken,
    ) -> bool {
        matches!(self, Self::ControllerDynamic)
            && !selection.entry_position.is_route_entry()
            && token.is_resolver()
    }

    #[inline]
    pub(super) const fn intrinsic_passive_progress_after_defer(self) -> bool {
        matches!(self, Self::PassiveIntrinsic)
    }

    #[inline]
    pub(super) const fn poll_wire_commit_requires_event(self) -> bool {
        self.is_dynamic()
    }

    #[inline]
    pub(super) const fn poll_wire_commit_requires_intrinsic_observation(self) -> bool {
        !self.is_dynamic()
    }

    #[inline]
    pub(super) const fn publishes_controller_ack_decision(self) -> bool {
        self.is_controller()
    }
}
