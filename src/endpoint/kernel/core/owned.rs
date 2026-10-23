use super::{
    CommitEventRow, CursorInvariantError, OperationKernel, PublicActiveOp, PublicOpEdge,
    PublicOpLease, ResidentActor, ResidentView, Transport,
};
use crate::eff::{EffIndex, EventPayload};
use crate::endpoint::{
    RecvError, RecvResult, SendError, SendResult, owner::EndpointOwner, transfer::EndpointTransfer,
};
use crate::global::typestate::EventAdmissionError;
use core::ptr::NonNull;

pub(in crate::endpoint::kernel) struct PreparedViewIssue {
    ptr: NonNull<crate::endpoint::carrier::KernelEndpointHeader>,
    generation: u32,
}

/// Affine evidence consumed by the same commit that advances actor progress.
/// Ordinary messages and route-only commits supply no view transition.
pub(in crate::endpoint::kernel) enum PreparedViewCommit {
    Retire(EndpointTransfer),
    Issue(PreparedViewIssue),
}

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    /// A private payload can supply the entry owner for its own prepared
    /// iteration. An independent sibling does not borrow that payload.
    fn independent_pending_entry_transfer(
        &self,
        role: u8,
        scope: crate::global::const_dsl::ScopeId,
        event: CommitEventRow,
    ) -> bool {
        let current = event.eff_index().dense_ordinal();
        let atom = self.context.program.event_atom_at(current);
        if atom.from == role || atom.to == role || atom.payload == EventPayload::Endpoint(role) {
            return false;
        }
        for view in self.views.as_slice() {
            for operation in view.operations() {
                if core::ptr::eq(operation.state.get(), self.operation) {
                    continue;
                }
                // SAFETY: the actor's common kernel borrow excludes mutation
                // of the other resident operations. The current mutable cell
                // was excluded before creating this shared reference.
                let operation = unsafe { &*operation.state.get() };
                let super::SendState::Sending { pending } = &operation.public_send_state else {
                    continue;
                };
                let Some(plan) = pending.commit_plan.as_ref() else {
                    continue;
                };
                if plan.proof.progress.delta.body_reentry() != scope {
                    continue;
                }
                let (Some(transfer), Some(pending_event)) =
                    (plan.proof.transfer, plan.proof.progress.delta.event())
                else {
                    continue;
                };
                let Some(payload_view) = self.views.get(role) else {
                    continue;
                };
                if transfer.ptr != NonNull::from(&payload_view.public_header)
                    || !transfer.handle.matches_header(&payload_view.public_header)
                {
                    continue;
                }
                let pending = pending_event.eff_index().dense_ordinal();
                for marker in 0..self.context.program.source_scope_marker_count() {
                    if let Some((start, split, end)) =
                        self.context.program.source_parallel_at(marker)
                        && ((start <= current
                            && current < split
                            && split <= pending
                            && pending < end)
                            || (start <= pending
                                && pending < split
                                && split <= current
                                && current < end))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A completed visit does not restore an Endpoint transferred by a later
    /// continuation. Entry owners are immutable source-prefix facts; live
    /// issuance is read from the existing affine headers, not a holder table.
    pub(in crate::endpoint::kernel) fn preflight_roll_entry_views(
        &self,
        event: Option<CommitEventRow>,
    ) -> Result<Option<crate::global::const_dsl::ScopeId>, EventAdmissionError> {
        let Some(scope) = self.prepared_body_reentry_scope(event) else {
            return Ok(None);
        };
        if !self.progress.cursor.roll_reentry_epochs_available(scope) {
            return Err(EventAdmissionError::Invariant);
        }
        // SAFETY: the current kernel retains this pinned actor allocation.
        let actor = unsafe {
            self.public_header
                .actor()
                .cast::<ResidentActor<'r, T>>()
                .as_ref()
        };
        let sent = event.and_then(|event| {
            let atom = self
                .context
                .program
                .event_atom_at(event.eff_index().dense_ordinal());
            match atom.payload {
                EventPayload::Endpoint(role) if atom.from == self.public_header.role() => {
                    Some(role)
                }
                EventPayload::Wire(_) | EventPayload::Endpoint(_) => None,
            }
        });
        let rows = self
            .progress
            .cursor
            .roll_entry_views(scope)
            .ok_or(EventAdmissionError::Invariant)?;
        let entry = event.and_then(|event| {
            self.progress
                .cursor
                .node_index_for_relocatable_step(event.progress_step())
        });
        let mut verdict = Ok(Some(scope));
        for (role, owner, receipts) in rows {
            // A retained independent child keeps its current visit and owner.
            // Only the operations actually restarted need their entry owners.
            if entry.is_some_and(|entry| {
                self.progress
                    .cursor
                    .roll_reentry_preserves_view(scope, role, entry)
            }) {
                continue;
            }
            let view = actor.views.get(role);
            if let Some(view) = view {
                // SAFETY: the actor operation lease excludes another kernel;
                // the payload view is distinct from this borrowed carrier.
                let operation = if core::ptr::eq(&view.public_header, self.public_header) {
                    self.operation.public_active_op
                } else {
                    unsafe { &*view.primary_operation().state.get() }.public_active_op
                };
                if operation.check_payload_borrow(role, sent).is_err()
                    && !event.is_some_and(|event| {
                        self.independent_pending_entry_transfer(role, scope, event)
                    })
                {
                    verdict = Err(EventAdmissionError::OtherViewPending);
                    continue;
                }
            }
            let available = match self
                .context
                .program
                .selected_actor(owner, &mut |scope| self.selected_arm_for_scope(scope))
            {
                Some(expected) if expected != self.progress.cursor.role() => {
                    view.is_none_or(|view| view.public_header.generation() == 0)
                }
                Some(_) => {
                    if view.is_some_and(|view| view.public_header.generation() != 0) {
                        true
                    } else {
                        match self.progress.cursor.resource_receipt_wait(
                            receipts,
                            (self.public_header.role(), sent),
                            &mut |scope| self.selected_arm_for_scope(scope),
                        ) {
                            EventAdmissionError::OtherViewPending => {
                                verdict = Err(EventAdmissionError::OtherViewPending);
                                continue;
                            }
                            error @ (EventAdmissionError::Invariant
                            | EventAdmissionError::PredecessorPending) => return Err(error),
                        }
                    }
                }
                None => false,
            };
            if !available {
                return Err(EventAdmissionError::Invariant);
            }
        }
        verdict
    }

    fn outgoing_transfer_view(
        &self,
        transfer: EndpointTransfer,
        role: u8,
    ) -> SendResult<&ResidentView<'r, T>> {
        // SAFETY: the owned send future retains the value behind this tag.
        let header = unsafe { transfer.ptr.as_ref() };
        if header.actor() != self.public_header.actor()
            || core::ptr::eq(header, self.public_header)
            || header.role() != role
            || !transfer.handle.matches_header(header)
        {
            return Err(SendError::PhaseInvariant);
        }
        // Recover the enclosing view using the actor's original allocation
        // pointer, rather than expanding the narrow header reborrow.
        let actor = unsafe { header.actor().cast::<ResidentActor<'r, T>>().as_ref() };
        actor.views.get(role).ok_or(SendError::PhaseInvariant)
    }

    pub(in crate::endpoint::kernel::core) fn reserve_transfer_out(
        &self,
        transfer: EndpointTransfer,
        role: u8,
    ) -> SendResult<()> {
        let view = self.outgoing_transfer_view(transfer, role)?;
        // SAFETY: the common actor lease protects the distinct payload view;
        // its Endpoint value is exclusively owned by this send future.
        if view.operations().any(|operation| unsafe {
            (&*operation.state.get()).public_active_op != PublicActiveOp::Idle
        }) {
            return Err(SendError::PhaseInvariant);
        }
        let operation = unsafe { &mut *view.primary_operation().state.get() };
        let borrowed = operation
            .public_active_op
            .transition(PublicOpEdge::BeginSendPayload);
        if borrowed.lease() != PublicOpLease::Held {
            crate::invariant();
        }
        operation.public_active_op = borrowed.phase();
        Ok(())
    }

    pub(in crate::endpoint::kernel::core) fn preflight_transfer_out(
        &self,
        transfer: EndpointTransfer,
        role: u8,
    ) -> SendResult<()> {
        let view = self.outgoing_transfer_view(transfer, role)?;
        // SAFETY: the same actor lease excludes concurrent payload mutation.
        if unsafe { &*view.primary_operation().state.get() }.public_active_op
            != PublicActiveOp::SendPayload
        {
            return Err(SendError::PhaseInvariant);
        }
        Ok(())
    }

    pub(in crate::endpoint::kernel) fn prepare_received_view(
        &self,
        eff: EffIndex,
        payload: crate::transport::wire::Payload<'_>,
    ) -> RecvResult<Option<PreparedViewIssue>> {
        let EventPayload::Endpoint(role) = self
            .context
            .program
            .event_atom_at(eff.dense_ordinal())
            .payload
        else {
            return Ok(None);
        };
        let envelope = crate::endpoint::transfer::wire_envelope(role, eff.dense_ordinal() as u16);
        if payload.as_bytes() != envelope {
            return Err(RecvError::PhaseInvariant);
        }
        // SAFETY: the current kernel retains this pinned actor allocation.
        let actor = unsafe {
            self.public_header
                .actor()
                .cast::<ResidentActor<'r, T>>()
                .as_ref()
        };
        let view = actor.views.get(role).ok_or(RecvError::PhaseInvariant)?;
        if view.public_header.generation() != 0 {
            return Err(RecvError::PhaseInvariant);
        }
        let generation =
            crate::endpoint::carrier::next_view_generation(self.context.view_generation.get())
                .ok_or(RecvError::PhaseInvariant)?;
        Ok(Some(PreparedViewIssue {
            ptr: NonNull::from(&view.public_header),
            generation,
        }))
    }

    fn preflight_view_commit(
        &self,
        event: Option<CommitEventRow>,
        view: &Option<PreparedViewCommit>,
    ) -> Result<u16, CursorInvariantError> {
        let atom = event.map(|event| {
            self.context
                .program
                .event_atom_at(event.eff_index().dense_ordinal())
        });
        let invalid = CursorInvariantError::INVARIANT;
        match (atom, view) {
            (None, None)
            | (
                Some(crate::eff::EffAtom {
                    payload: EventPayload::Wire(_),
                    ..
                }),
                None,
            ) => Ok(self.context.live_views.get()),
            (Some(atom), Some(PreparedViewCommit::Retire(transfer))) => {
                let EventPayload::Endpoint(role) = atom.payload else {
                    return Err(invalid);
                };
                if atom.from != self.public_header.role() {
                    return Err(invalid);
                }
                self.preflight_transfer_out(*transfer, role)
                    .map_err(|_| invalid)?;
                self.context.live_views.get().checked_sub(1).ok_or(invalid)
            }
            (Some(atom), Some(PreparedViewCommit::Issue(issue))) => {
                let EventPayload::Endpoint(role) = atom.payload else {
                    return Err(invalid);
                };
                // SAFETY: only prepare_received_view constructs this evidence;
                // its dormant record is retained by the same pinned actor.
                let header = unsafe { issue.ptr.as_ref() };
                if atom.to != self.public_header.role()
                    || header.actor() != self.public_header.actor()
                    || header.role() != role
                    || header.generation() != 0
                    || crate::endpoint::carrier::next_view_generation(
                        self.context.view_generation.get(),
                    ) != Some(issue.generation)
                {
                    return Err(invalid);
                }
                self.context.live_views.get().checked_add(1).ok_or(invalid)
            }
            _ => Err(invalid),
        }
    }

    pub(super) fn commit_prepared_view(
        &mut self,
        event: Option<CommitEventRow>,
        view: Option<PreparedViewCommit>,
    ) -> Option<EndpointOwner> {
        let Ok(live_views) = self.preflight_view_commit(event, &view) else {
            crate::invariant();
        };
        match view {
            None => None,
            Some(PreparedViewCommit::Retire(transfer)) => {
                let header = unsafe { transfer.ptr.as_ref() };
                let view = self
                    .outgoing_transfer_view(transfer, header.role())
                    .unwrap_or_else(|_| crate::invariant());
                // SAFETY: preflight proved the unique payload borrow, and the
                // actor lease keeps its release and retirement in one commit.
                let operation = unsafe { &mut *view.primary_operation().state.get() };
                let released = operation
                    .public_active_op
                    .transition(PublicOpEdge::FinishSendPayload);
                if released.lease() != PublicOpLease::Held {
                    crate::invariant();
                }
                operation.public_active_op = released.phase();
                // SAFETY: preflight rechecked the unique future-owned view.
                header.retire_generation();
                self.context.live_views.set(live_views);
                // Ports and storage belong to the actor/run and survive return.
                None
            }
            Some(PreparedViewCommit::Issue(issue)) => {
                // No callback or fallible check separates these writes from the
                // actor progress publication in commit_prepared_delta.
                unsafe { issue.ptr.as_ref() }.issue_received(issue.generation);
                self.context.view_generation.set(issue.generation);
                self.context.live_views.set(live_views);
                Some(EndpointOwner::new(
                    issue.ptr,
                    crate::endpoint::carrier::EndpointHandle::new(
                        self.context.public_generation.get(),
                        issue.generation,
                    ),
                ))
            }
        }
    }
}
