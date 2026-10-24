use crate::{
    eff::{EffIndex, EventOrigin},
    global::{compiled::images::EventSemanticKind, const_dsl::ScopeId},
};

use super::{RouteChoiceMark, StateIndex};

/// The logical view and message family requested by one endpoint operation.
#[derive(Clone, Copy)]
pub(crate) struct MessageSelector {
    pub(crate) role: u8,
    pub(crate) label: u8,
    pub(crate) payload_kind: crate::eff::EventPayload,
}

impl MessageSelector {
    pub(crate) const fn new(role: u8, label: u8, payload_kind: crate::eff::EventPayload) -> Self {
        Self {
            role,
            label,
            payload_kind,
        }
    }
}

/// Metadata for a send transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SendMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) peer: u8,
    pub(crate) label: u8,
    pub(crate) payload_kind: crate::eff::EventPayload,
    pub(crate) frame_label: u16,
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
    pub(crate) body_reentry: ScopeId,
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
            body_reentry: ScopeId::none(),
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
    pub(crate) payload_kind: crate::eff::EventPayload,
    pub(crate) frame_label: u16,
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
    pub(crate) payload_kind: crate::eff::EventPayload,
    pub(crate) frame_label: u16,
    pub(crate) semantic: EventSemanticKind,
    pub(crate) origin: EventOrigin,
    pub(crate) next: usize,
    pub(crate) scope: ScopeId,
    pub(crate) route_scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

impl From<LocalMeta> for EventCommitMeta {
    #[inline(always)]
    fn from(meta: LocalMeta) -> Self {
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

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}
