//! Typed offer route-shape profile and ingress planning.

use super::{OfferScopeSelection, RouteDecisionSource, ingress::OfferIngressMode};

mod evidence;
mod planning;

pub(super) use self::evidence::{
    OfferArmRecvEvidence, OfferControllerCursorArm, OfferControllerSkipEvidence,
    OfferMaterializationReadiness, OfferPassiveAckEvidence, OfferPassiveEvidence,
    OfferPassiveReadySignal, OfferPassiveRecvEvidence,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::endpoint::kernel) enum OfferScopeProfile {
    ControllerStatic,
    ControllerDynamic,
    PassiveStatic,
    PassiveDynamic,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OfferAuthorityRole {
    Controller,
    Passive,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OfferRouteScopeKind {
    Static,
    Dynamic,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OfferAuthorityPath {
    ControllerResolver,
    PassiveEvidence,
    LocalSources,
}

#[derive(Clone, Copy)]
pub(super) enum OfferEntryPosition {
    RouteEntry,
    AfterRouteEntry,
}

impl OfferEntryPosition {
    #[inline]
    pub(super) const fn from_route_entry(at_route_entry: bool) -> Self {
        if at_route_entry {
            Self::RouteEntry
        } else {
            Self::AfterRouteEntry
        }
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
pub(super) enum OfferControllerSkipReadiness {
    Ready,
    BlockedByMaterialization,
    NeedsTransport,
}

#[derive(Clone, Copy)]
pub(super) struct OfferControllerReadiness {
    pub(super) skip: OfferControllerSkipReadiness,
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
    pub(super) controller: OfferControllerReadiness,
    pub(super) passive: OfferPassiveReadiness,
}

impl OfferRouteShape {
    #[inline]
    pub(super) const fn ingress_mode(self) -> OfferIngressMode {
        match self.profile {
            OfferScopeProfile::ControllerStatic | OfferScopeProfile::ControllerDynamic => {
                self.controller_ingress_mode()
            }
            OfferScopeProfile::PassiveStatic | OfferScopeProfile::PassiveDynamic => {
                self.passive_ingress_mode()
            }
        }
    }

    #[inline]
    const fn controller_ingress_mode(self) -> OfferIngressMode {
        if self.controller_can_skip_recv() || self.early_decision.arm_has_no_recv() {
            return OfferIngressMode::Skip;
        }
        OfferIngressMode::TransportOnly
    }

    #[inline]
    const fn passive_ingress_mode(self) -> OfferIngressMode {
        if self.passive_can_skip_recv() || self.early_decision.arm_has_no_recv() {
            return OfferIngressMode::Skip;
        }
        OfferIngressMode::TransportOnly
    }

    #[inline]
    const fn controller_can_skip_recv(self) -> bool {
        if matches!(
            self.controller.skip,
            OfferControllerSkipReadiness::BlockedByMaterialization
        ) {
            return false;
        }
        match (self.entry, self.cursor) {
            (OfferEntryPosition::RouteEntry, _) => {
                self.profile.is_dynamic()
                    || matches!(self.controller.skip, OfferControllerSkipReadiness::Ready)
                    || self.early_decision.available()
            }
            (OfferEntryPosition::AfterRouteEntry, OfferCursorReadiness::NonRecv) => true,
            (OfferEntryPosition::AfterRouteEntry, OfferCursorReadiness::Recv) => false,
        }
    }

    #[inline]
    const fn passive_can_skip_recv(self) -> bool {
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
    pub(super) const fn from_evidence(role: OfferAuthorityRole, kind: OfferRouteScopeKind) -> Self {
        match (role, kind) {
            (OfferAuthorityRole::Controller, OfferRouteScopeKind::Dynamic) => {
                Self::ControllerDynamic
            }
            (OfferAuthorityRole::Controller, OfferRouteScopeKind::Static) => Self::ControllerStatic,
            (OfferAuthorityRole::Passive, OfferRouteScopeKind::Dynamic) => Self::PassiveDynamic,
            (OfferAuthorityRole::Passive, OfferRouteScopeKind::Static) => Self::PassiveStatic,
        }
    }

    #[inline]
    pub(super) const fn is_controller(self) -> bool {
        matches!(self, Self::ControllerStatic | Self::ControllerDynamic)
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
    pub(super) const fn suppresses_scope_frame_hint(self) -> bool {
        self.is_dynamic()
    }

    #[inline]
    pub(super) const fn authority_path_after_ack_miss(self) -> OfferAuthorityPath {
        match self {
            Self::ControllerDynamic => OfferAuthorityPath::ControllerResolver,
            Self::PassiveStatic | Self::PassiveDynamic => OfferAuthorityPath::PassiveEvidence,
            Self::ControllerStatic => OfferAuthorityPath::LocalSources,
        }
    }

    #[inline]
    pub(super) const fn transport_marks_ready_from_source(
        self,
        source: RouteDecisionSource,
    ) -> bool {
        match self {
            Self::PassiveDynamic => {
                matches!(source, RouteDecisionSource::Ack | RouteDecisionSource::Poll)
            }
            Self::ControllerDynamic => {
                matches!(
                    source,
                    RouteDecisionSource::Resolver | RouteDecisionSource::Poll
                )
            }
            Self::ControllerStatic | Self::PassiveStatic => false,
        }
    }

    #[inline]
    pub(super) const fn keeps_current_scope_for_unready_resolver(
        self,
        selection: OfferScopeSelection,
        source: RouteDecisionSource,
    ) -> bool {
        matches!(self, Self::ControllerDynamic)
            && !selection.at_route_offer_entry
            && matches!(source, RouteDecisionSource::Resolver)
    }

    #[inline]
    pub(super) const fn static_passive_progress_after_defer(self) -> bool {
        matches!(self, Self::PassiveStatic)
    }

    #[inline]
    pub(super) const fn poll_wire_commit_requires_event(self) -> bool {
        self.is_dynamic()
    }

    #[inline]
    pub(super) const fn poll_wire_commit_requires_static_observation(self) -> bool {
        !self.is_dynamic()
    }

    #[inline]
    pub(super) const fn publishes_recvless_parent_route_decision(self) -> bool {
        self.is_passive()
    }

    #[inline]
    pub(super) const fn publishes_controller_ack_decision(self) -> bool {
        self.is_controller()
    }
}
