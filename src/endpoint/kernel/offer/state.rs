//! Offer preview state and restore ownership.

use super::ingress::OfferFrontierFacts;
use super::{IngressEvidenceState, OfferProgressState, OfferScopeSelection, ResolvePendingState};
use crate::endpoint::kernel::lane_port;
use crate::global::typestate::InboundFrameKey;

pub(super) struct OfferStagedIngress<'a> {
    transport_payload: Option<lane_port::PreambleFrame<'a>>,
}

impl<'a> OfferStagedIngress<'a> {
    #[inline]
    pub(super) const fn empty() -> Self {
        Self {
            transport_payload: None,
        }
    }

    #[inline]
    pub(super) fn has_transport(&self) -> bool {
        self.transport_payload.is_some()
    }

    #[inline]
    pub(super) fn evidence_state(&self) -> IngressEvidenceState {
        match self.transport_payload {
            Some(_) => IngressEvidenceState::Ready,
            None => IngressEvidenceState::Absent,
        }
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.transport_payload.is_none()
    }

    #[inline]
    pub(super) fn transport_lane_wire(&self) -> Option<u8> {
        self.transport_payload
            .as_ref()
            .map(lane_port::PreambleFrame::lane_wire)
    }

    #[inline]
    pub(super) fn transport_frame_key(&self) -> Option<InboundFrameKey> {
        let frame = self.transport_payload.as_ref()?;
        if frame.is_deterministic() {
            None
        } else {
            Some(InboundFrameKey::new(
                frame.observed_source_role(),
                frame.lane_wire(),
                frame.observed_frame_label_raw(),
            ))
        }
    }

    #[inline]
    fn transport_observation(
        &self,
        session_raw: u32,
        target_role: u8,
    ) -> Option<lane_port::FrameObservation> {
        let frame = self.transport_payload.as_ref()?;
        if frame.is_deterministic() {
            None
        } else {
            Some(frame.observed_transport_frame(session_raw, frame.lane_wire(), target_role))
        }
    }

    #[inline]
    pub(super) fn stage_transport(&mut self, frame: lane_port::PreambleFrame<'a>) {
        if self.transport_payload.is_some() {
            crate::invariant();
        }
        self.transport_payload = Some(frame);
    }

    #[inline]
    pub(super) fn take_transport(&mut self) -> Option<lane_port::PreambleFrame<'a>> {
        self.transport_payload.take()
    }

    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        if let Some(payload) = self.transport_payload.take() {
            payload.discard_uncommitted();
        }
    }
}

pub(super) struct OfferCollectState<'a> {
    pub(super) facts: OfferFrontierFacts,
    pub(super) ingress: OfferStagedIngress<'a>,
}

impl OfferCollectState<'_> {
    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        self.ingress.discard_terminal();
    }
}

pub(super) struct OfferResolveState<'a> {
    pub(super) facts: OfferFrontierFacts,
    pub(super) ingress: OfferStagedIngress<'a>,
    pub(super) progress: OfferProgressState,
    pub(super) pending: ResolvePendingState,
}

impl OfferResolveState<'_> {
    #[inline]
    pub(super) const fn selection(&self) -> OfferScopeSelection {
        self.facts.selection
    }

    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        self.ingress.discard_terminal();
    }
}

pub(super) enum OfferExecution<'a> {
    Uninitialized,
    Selecting {
        frontier_visited: super::FrontierVisitSet,
    },
    Collecting {
        frontier_visited: super::FrontierVisitSet,
        stage: OfferCollectState<'a>,
    },
    Resolving {
        frontier_visited: super::FrontierVisitSet,
        stage: OfferResolveState<'a>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum OfferExecutionKind {
    Uninitialized,
    Selecting,
    Collecting,
    Resolving,
}

impl OfferExecution<'_> {
    #[inline]
    pub(super) const fn kind(&self) -> OfferExecutionKind {
        match self {
            Self::Uninitialized => OfferExecutionKind::Uninitialized,
            Self::Selecting { .. } => OfferExecutionKind::Selecting,
            Self::Collecting { .. } => OfferExecutionKind::Collecting,
            Self::Resolving { .. } => OfferExecutionKind::Resolving,
        }
    }
}

pub(in crate::endpoint::kernel) struct OfferDetachedIngress<'r> {
    pub(in crate::endpoint::kernel) carried_transport_payload: Option<lane_port::PreambleFrame<'r>>,
    pub(in crate::endpoint::kernel) stage_transport_payload: Option<lane_port::PreambleFrame<'r>>,
}

impl OfferDetachedIngress<'_> {
    #[inline]
    pub(in crate::endpoint::kernel) fn discard_terminal(&mut self) {
        if let Some(payload) = self.carried_transport_payload.take() {
            payload.discard_uncommitted();
        }
        if let Some(payload) = self.stage_transport_payload.take() {
            payload.discard_uncommitted();
        }
    }
}

pub(crate) struct OfferState<'r> {
    pub(super) carried_ingress: OfferStagedIngress<'r>,
    pub(super) execution: OfferExecution<'r>,
    pub(super) pending_recv: lane_port::PendingRecv,
}

impl<'r> OfferState<'r> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            carried_ingress: OfferStagedIngress::empty(),
            execution: OfferExecution::Uninitialized,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(super) fn take_carried_ingress(&mut self) -> OfferStagedIngress<'r> {
        core::mem::replace(&mut self.carried_ingress, OfferStagedIngress::empty())
    }

    #[inline]
    pub(super) fn carried_transport_lane_wire(&self) -> Option<u8> {
        self.carried_ingress.transport_lane_wire()
    }

    #[inline]
    pub(super) fn carried_transport_frame_key(&self) -> Option<InboundFrameKey> {
        self.carried_ingress.transport_frame_key()
    }

    #[inline]
    pub(super) fn carried_transport_observation(
        &self,
        session_raw: u32,
        target_role: u8,
    ) -> Option<lane_port::FrameObservation> {
        self.carried_ingress
            .transport_observation(session_raw, target_role)
    }

    #[inline]
    pub(super) fn carry_ingress(&mut self, ingress: OfferStagedIngress<'r>) {
        self.carried_ingress = ingress;
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn take_detached_ingress(
        &mut self,
    ) -> OfferDetachedIngress<'r> {
        let mut items = OfferDetachedIngress {
            carried_transport_payload: self.carried_ingress.take_transport(),
            stage_transport_payload: None,
        };
        match core::mem::replace(&mut self.execution, OfferExecution::Uninitialized) {
            OfferExecution::Uninitialized | OfferExecution::Selecting { .. } => return items,
            OfferExecution::Collecting { stage, .. } => {
                let mut ingress = stage.ingress;
                items.stage_transport_payload = ingress.take_transport();
            }
            OfferExecution::Resolving { stage, .. } => {
                let mut ingress = stage.ingress;
                items.stage_transport_payload = ingress.take_transport();
            }
        }
        items
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn discard_terminal(&mut self) {
        let mut detached = self.take_detached_ingress();
        detached.discard_terminal();
    }
}
