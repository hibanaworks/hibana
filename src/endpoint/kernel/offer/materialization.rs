//! Exact event admission and callback-free route-branch publication for `offer()`.

use core::task::Poll;

use super::{
    AdmittedOfferEvent, BranchKind, BranchMeta, CachedRecvMeta, OfferScopeProfile,
    OfferScopeSelection, OfferStagedIngress, OperationKernel, ResolvedRouteArm,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::{EnabledEventCommit, EventAdmissionError, EventArmView},
    transport::Transport,
};

use crate::endpoint::kernel::{
    core::{MaterializedRouteBranch, OfferedFrame},
    lane_port,
};

/// A final, same-phase event grant and its affine frame. This value never
/// enters a pending operation or public branch: publication consumes it before
/// another callback or progress mutation can invalidate the admission.
pub(super) struct ResolvedOfferEvent<'r> {
    branch: MaterializedRouteBranch<'r>,
    enabled: Option<EnabledEventCommit>,
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(super) fn prepare_offer_event(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        profile: OfferScopeProfile,
        ingress: &mut OfferStagedIngress<'r>,
    ) -> Poll<RecvResult<Option<ResolvedOfferEvent<'r>>>> {
        let ResolvedRouteArm {
            route_token,
            selected_arm,
            route_arm_selection_commit_evidence,
            outbound,
        } = resolved;
        let (preview_meta, branch_kind, mut enabled) = if let Some(event) = outbound {
            (event.metadata, event.kind, Some(event.enabled))
        } else {
            let preview_meta = match self.preview_selected_arm_meta(selection, selected_arm) {
                Ok(meta) => meta,
                Err(err) => {
                    ingress.discard_terminal();
                    return Poll::Ready(Err(err));
                }
            };
            let branch_kind = self.materialized_branch_kind(&preview_meta);
            (preview_meta, branch_kind, None)
        };
        if preview_meta.is_empty() {
            ingress.discard_terminal();
            return Poll::Ready(Err(RecvError::PhaseInvariant));
        }

        let mut frame = None;
        if let Some(payload) = ingress.take_transport() {
            // Acceptance emits a tap callback, and requeue invokes transport.
            // Only immutable intent crosses either boundary.
            enabled = None;
            frame = match self.resolve_materialized_transport(branch_kind, preview_meta, payload) {
                Ok(frame) => frame,
                Err(error) => return Poll::Ready(Err(error)),
            };
            self.reconcile_retirements();
            if let Some(kind) = self.session_fault() {
                if let Some(frame) = frame {
                    frame.discard_uncommitted();
                }
                return Poll::Ready(Err(RecvError::SessionFault(kind)));
            }
        }
        if enabled.is_none() && branch_kind != BranchKind::TerminalArm {
            match self.admit_offer_event(selection, selected_arm, preview_meta, branch_kind) {
                Ok(event) => enabled = Some(event.enabled),
                Err(error) => {
                    if let Some(frame) = frame {
                        let lane = frame.lane_idx();
                        let requeue = frame.requeue_on(self.port_for_lane(lane));
                        self.reconcile_retirements();
                        if let Some(kind) = self.session_fault() {
                            return Poll::Ready(Err(RecvError::SessionFault(kind)));
                        }
                        if let Err(error) = requeue {
                            return Poll::Ready(Err(RecvError::Transport(error)));
                        }
                    }
                    return match error {
                        EventAdmissionError::Invariant => Poll::Ready(Ok(None)),
                        EventAdmissionError::OtherViewPending
                        | EventAdmissionError::PredecessorPending
                        | EventAdmissionError::PayloadPredecessorPending => Poll::Pending,
                    };
                }
            }
        }
        let branch_meta = BranchMeta {
            scope_id: selection.scope_id,
            selected_arm,
            lane_wire: preview_meta.lane,
            cursor_index: preview_meta.cursor_index,
            eff_index: preview_meta.eff_index,
            label: preview_meta.label,
            origin: preview_meta.origin,
            frame_label: preview_meta.eff_index.dense_ordinal() as u16,
            kind: branch_kind,
            profile,
            route_token,
            route_arm_selection_commit_evidence,
        };
        Poll::Ready(Ok(Some(ResolvedOfferEvent {
            branch: MaterializedRouteBranch {
                offered_frame: frame.map(OfferedFrame::new),
                branch_meta,
            },
            enabled,
        })))
    }

    /// Publication performs no protocol search, callback, or progress update.
    /// The public branch stores intent; its eventual send/recv rechecks the
    /// canonical semantic state independently before committing.
    pub(super) fn produce_branch(&mut self, plan: ResolvedOfferEvent<'r>) -> u8 {
        let ResolvedOfferEvent { branch, enabled } = plan;
        if match enabled {
            Some(enabled) => enabled.event_index() != branch.branch_meta.cursor_index,
            None => branch.branch_meta.kind != BranchKind::TerminalArm,
        } || self.operation.public_route_branch.is_some()
        {
            if let Some(frame) = branch.offered_frame {
                frame.discard_terminal();
            }
            crate::invariant();
        }
        let label = branch.branch_meta.label;
        self.operation.public_route_branch = Some(branch);
        label
    }

    #[inline(never)]
    fn admit_offer_event(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
        metadata: CachedRecvMeta,
        kind: BranchKind,
    ) -> Result<AdmittedOfferEvent, EventAdmissionError> {
        let index = metadata.cursor_index.as_usize();
        let row = self
            .progress
            .cursor
            .event_row_at(index)
            .ok_or(EventAdmissionError::Invariant)?;
        if row.logical_role() != self.public_header.role()
            || !self.permits_event(index)
            || !self.owns_lane(usize::from(metadata.lane))
        {
            return Err(EventAdmissionError::Invariant);
        }
        let mut previews = self.preview_queries();
        let enabled =
            self.event_enabled(index, row.commit_meta(), &mut |scope, view| match view {
                EventArmView::Committed => self.selected_arm_for_scope(scope),
                EventArmView::Preview if scope == selection.scope_id => Some(selected_arm),
                EventArmView::Preview => previews(scope)
                    .or_else(|| self.selected_arm_for_recv_event(row.conflict(), scope, view)),
            })?;
        Ok(AdmittedOfferEvent {
            metadata,
            enabled,
            kind,
        })
    }

    fn materialized_branch_kind(&self, metadata: &CachedRecvMeta) -> BranchKind {
        let cursor_index = metadata.cursor_index.as_usize();
        if metadata.is_recv_step() {
            BranchKind::WireRecv
        } else if self.progress.cursor.is_send_at(cursor_index) {
            BranchKind::ArmSend
        } else if self.progress.cursor.is_local_action_at(cursor_index) {
            BranchKind::LocalAction
        } else {
            BranchKind::TerminalArm
        }
    }

    #[inline(never)]
    fn resolve_materialized_transport(
        &mut self,
        branch_kind: BranchKind,
        metadata: CachedRecvMeta,
        payload: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<Option<lane_port::ReceivedFrame<'r>>> {
        let frame_label = metadata.eff_index.dense_ordinal() as u16;
        let observed_frame_label = payload.observed_frame_label_raw();
        if branch_kind == BranchKind::WireRecv && payload.lane_wire() == metadata.lane {
            return self
                .accept_materialized_transport_frame(
                    payload.lane_idx(),
                    metadata.lane,
                    metadata.peer,
                    frame_label,
                    payload,
                )
                .map(Some);
        }
        if branch_kind == BranchKind::WireRecv && observed_frame_label != frame_label {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload)?;
        Ok(None)
    }
}
