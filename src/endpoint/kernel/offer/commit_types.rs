//! Offer branch commit and decode metadata.

use super::super::authority::RouteArmToken;
use super::super::core::BranchPreviewView;
use super::profile::OfferScopeProfile;
use super::resolve_types::RouteArmCommitEvidence;
use crate::eff::EffIndex;
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{RecvMeta, StateIndex};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchCommitPlan {
    pub(in crate::endpoint::kernel) preview: BranchPreviewView,
    pub(in crate::endpoint::kernel) meta: Option<RecvMeta>,
    pub(in crate::endpoint::kernel) route_seed_len: usize,
}

impl BranchCommitPlan {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn meta(&self) -> Option<RecvMeta> {
        self.meta
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn route_seed_len(&self) -> usize {
        self.route_seed_len
    }
}

/// Branch metadata carried from `offer()` to `decode()`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BranchMeta {
    /// The scope this branch belongs to.
    pub(crate) scope_id: ScopeId,
    /// The selected arm (0, 1, ...).
    pub(crate) selected_arm: u8,
    /// Wire lane for this branch.
    pub(crate) lane_wire: u8,
    /// Exact typestate node selected by `offer()` for this branch.
    pub(crate) cursor_index: StateIndex,
    /// EffIndex of the previewed resident branch step. Ignored by empty arms.
    pub(crate) eff_index: EffIndex,
    /// Event label for the previewed branch step.
    pub(crate) label: u8,
    /// Whether the previewed branch step is a control event.
    pub(crate) is_control: bool,
    /// Transport/ingress discriminator expected for this branch.
    pub(crate) frame_label: u8,
    /// Branch dispatch category for decode() dispatch.
    pub(crate) kind: BranchKind,
    /// Route-shape profile captured during offer materialization.
    pub(in crate::endpoint::kernel) profile: OfferScopeProfile,
    /// Route arm authority used when commit emits route-arm selection events.
    pub(in crate::endpoint::kernel) route_token: RouteArmToken,
    /// Evidence controlling whether branch commit emits a route-decision event.
    /// Passive payload/frame-label evidence is demux evidence, not authority.
    pub(in crate::endpoint::kernel) route_arm_selection_commit_evidence: RouteArmCommitEvidence,
}

/// Branch type taxonomy for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/ingress.
    WireRecv,
    /// Synthetic endpoint-local control that does not go on wire.
    /// Decode from zero buffer; route commit uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should continue on the same borrowed endpoint with `flow().send()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}
