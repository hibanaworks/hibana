use crate::{
    eff::{EffIndex, EventOrigin},
    global::{compiled::images::EventSemanticKind, const_dsl::ScopeId},
};

use super::{RouteChoiceMark, StateIndex};

/// Complete descriptor-visible identity of an inbound transport operation.
///
/// Session and target role are fixed by the attached endpoint. These three
/// fields are the remaining wire facts needed to select exactly one receive
/// occurrence without conflating messages from different peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InboundFrameKey {
    pub(crate) source_role: u8,
    pub(crate) lane: u8,
    pub(crate) frame_label: u8,
}

impl InboundFrameKey {
    #[inline(always)]
    pub(crate) const fn new(source_role: u8, lane: u8, frame_label: u8) -> Self {
        Self {
            source_role,
            lane,
            frame_label,
        }
    }

    #[inline(always)]
    pub(crate) const fn matches_recv(self, meta: RecvMeta) -> bool {
        self.source_role == meta.peer
            && self.lane == meta.lane
            && self.frame_label == meta.frame_label
    }
}

/// Metadata for a send transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SendMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) peer: u8,
    pub(crate) label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) frame_label: u8,
    pub(crate) semantic: EventSemanticKind,
    pub(crate) origin: EventOrigin,
    pub(crate) next: usize,
    pub(crate) scope: ScopeId,
    pub(crate) route_scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    pub(crate) selected_route_arm: Option<u8>,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventCommitMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) label: u8,
    pub(crate) origin: EventOrigin,
    pub(crate) scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    pub(crate) lane: u8,
}

impl EventCommitMeta {
    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        label: u8,
        origin: EventOrigin,
        scope: ScopeId,
        route_arm: Option<u8>,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            label,
            origin,
            scope,
            route_arm,
            lane,
        }
    }
}

impl From<SendMeta> for EventCommitMeta {
    #[inline(always)]
    fn from(meta: SendMeta) -> Self {
        Self::new(
            meta.eff_index,
            meta.label,
            meta.origin,
            meta.scope,
            meta.route_arm,
            meta.lane,
        )
    }
}

/// Metadata for a receive transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecvMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) peer: u8,
    pub(crate) label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) frame_label: u8,
    pub(crate) semantic: EventSemanticKind,
    pub(crate) origin: EventOrigin,
    pub(crate) next: usize,
    pub(crate) scope: ScopeId,
    pub(crate) route_scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    /// Route-choice role of this recv.
    pub(crate) choice: RouteChoiceMark,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

impl From<RecvMeta> for EventCommitMeta {
    #[inline(always)]
    fn from(meta: RecvMeta) -> Self {
        Self::new(
            meta.eff_index,
            meta.label,
            meta.origin,
            meta.scope,
            meta.route_arm,
            meta.lane,
        )
    }
}

/// Metadata for a local action derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) frame_label: u8,
    pub(crate) semantic: EventSemanticKind,
    pub(crate) origin: EventOrigin,
    pub(crate) next: usize,
    pub(crate) scope: ScopeId,
    pub(crate) route_scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}
