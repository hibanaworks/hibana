use crate::{
    control::cap::mint::CapShot,
    eff::EffIndex,
    global::{
        compiled::images::ControlSemanticKind,
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
    pub semantic: ControlSemanticKind,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub shot: Option<CapShot>,
    policy: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

impl SendMeta {
    pub(crate) const fn new(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        semantic: ControlSemanticKind,
        is_control: bool,
        next: usize,
        scope: ScopeId,
        route_arm: Option<u8>,
        shot: Option<CapShot>,
        policy: ResolverMode,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            peer,
            label,
            frame_label,
            resource,
            semantic,
            is_control,
            next,
            scope,
            route_arm,
            shot,
            policy,
            lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn policy(&self) -> ResolverMode {
        self.policy
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
    pub semantic: ControlSemanticKind,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    /// Whether this recv is a choice determinant (first recv of a route arm).
    pub is_choice_determinant: bool,
    pub shot: Option<CapShot>,
    pub policy: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

/// Metadata for a local action derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalMeta {
    pub eff_index: EffIndex,
    pub label: u8,
    pub frame_label: u8,
    pub resource: Option<u8>,
    pub semantic: ControlSemanticKind,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub shot: Option<CapShot>,
    pub policy: ResolverMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

pub(crate) const fn as_state_index(idx: usize) -> StateIndex {
    StateIndex::from_usize(idx)
}

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}
