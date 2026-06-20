//! Offer branch commit metadata.

use super::super::authority::RouteArmToken;
use super::super::decision_state::SelectedRouteCommitRowsRef;
use super::profile::OfferScopeProfile;
use crate::eff::{EffIndex, EventOrigin};
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{RecvMeta, StateIndex};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchCommitPlan {
    meta: Option<RecvMeta>,
    route_seed_rows: SelectedRouteCommitRowsRef,
}

impl BranchCommitPlan {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn new(
        meta: Option<RecvMeta>,
        route_seed_rows: SelectedRouteCommitRowsRef,
    ) -> Self {
        Self {
            meta,
            route_seed_rows,
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn meta(&self) -> Option<RecvMeta> {
        self.meta
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn route_seed_rows(&self) -> SelectedRouteCommitRowsRef {
        self.route_seed_rows
    }
}

/// Branch metadata carried from `offer()` to the selected branch first step.
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
    /// EffIndex of the previewed resident branch step. Ignored by terminal arms.
    pub(crate) eff_index: EffIndex,
    /// Event label for the previewed branch step.
    pub(crate) label: u8,
    /// Origin of the previewed branch step.
    pub(crate) origin: EventOrigin,
    /// Transport/ingress discriminator expected for this branch.
    pub(crate) frame_label: u8,
    /// Branch first-step category.
    pub(crate) kind: BranchKind,
    /// Route-shape profile captured during offer materialization.
    pub(in crate::endpoint::kernel) profile: OfferScopeProfile,
    /// Route arm authority used when commit emits route-arm selection events.
    pub(in crate::endpoint::kernel) route_token: RouteArmToken,
    /// Evidence controlling whether branch commit emits a route-decision event.
    /// Passive payload/frame-label evidence is demux evidence, not authority.
    pub(in crate::endpoint::kernel) route_arm_selection_commit_evidence:
        super::resolve_types::RouteArmCommitEvidence,
}

/// Branch first-step taxonomy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/ingress.
    WireRecv,
    /// Endpoint-local action that does not go on wire.
    /// Branch recv validates Hibana's canonical empty payload; route commit uses
    /// meta fields directly.
    LocalAction,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver continues on the same borrowed endpoint with `send()`.
    ArmSendHint,
    /// Terminal arm without a resident receive/send event.
    /// Branch recv validates Hibana's canonical empty payload and advances to scope end.
    TerminalArm,
}
