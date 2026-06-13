use crate::{
    eff::EffIndex,
    global::{
        compiled::images::EventSemanticKind,
        const_dsl::{ResolverMode, ScopeId},
    },
};

use super::StateIndex;

/// Metadata for a send transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendMeta {
    pub eff_index: EffIndex,
    pub peer: u8,
    pub label: u8,
    pub frame_label: u8,
    pub resource: Option<u8>,
    pub semantic: EventSemanticKind,
    pub is_internal: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub(crate) resolver: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

impl SendMeta {
    #[inline(always)]
    pub(crate) const fn resolver(&self) -> ResolverMode {
        self.resolver
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventCommitMeta {
    pub(crate) eff_index: EffIndex,
    pub(crate) label: u8,
    pub(crate) is_internal: bool,
    pub(crate) scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    pub(crate) lane: u8,
}

impl EventCommitMeta {
    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        label: u8,
        is_internal: bool,
        scope: ScopeId,
        route_arm: Option<u8>,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            label,
            is_internal,
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
            meta.is_internal,
            meta.scope,
            meta.route_arm,
            meta.lane,
        )
    }
}

/// Metadata for a receive transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecvMeta {
    pub eff_index: EffIndex,
    pub peer: u8,
    pub label: u8,
    pub frame_label: u8,
    pub resource: Option<u8>,
    pub semantic: EventSemanticKind,
    pub is_internal: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    /// Whether this recv is a choice determinant (first recv of a route arm).
    pub is_choice_determinant: bool,
    pub resolver: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

impl From<RecvMeta> for EventCommitMeta {
    #[inline(always)]
    fn from(meta: RecvMeta) -> Self {
        Self::new(
            meta.eff_index,
            meta.label,
            meta.is_internal,
            meta.scope,
            meta.route_arm,
            meta.lane,
        )
    }
}

/// Metadata for a local action derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalMeta {
    pub eff_index: EffIndex,
    pub label: u8,
    pub frame_label: u8,
    pub resource: Option<u8>,
    pub semantic: EventSemanticKind,
    pub is_internal: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub resolver: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}
