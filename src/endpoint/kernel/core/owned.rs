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
    /// A completed visit does not restore an Endpoint transferred by a later
    /// continuation. Entry owners are immutable source-prefix facts; live
    /// issuance is read from the existing affine headers, not a holder table.
    pub(in crate::endpoint::kernel) fn preflight_roll_entry_views(
        &self,
        event: Option<CommitEventRow>,
    ) -> Result<(), EventAdmissionError> {
        let Some(scope) = self.prepared_body_reentry_scope(event) else {
            return Ok(());
        };
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
        let mut verdict = Ok(());
        for (role, owner, receipts) in rows {
            let view = actor.views.get(role);
            if let Some(view) = view {
                // SAFETY: the actor operation lease excludes another kernel;
                // the payload view is distinct from this borrowed carrier.
                let operation = if core::ptr::eq(&view.public_header, self.public_header) {
                    self.io.public_active_op
                } else {
                    unsafe { &*view.io.get() }.public_active_op
                };
                if operation.check_payload_borrow(role, sent).is_err() {
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
        let io = unsafe { &mut *view.io.get() };
        if io.public_active_op != PublicActiveOp::Idle {
            return Err(SendError::PhaseInvariant);
        }
        let borrowed = io
            .public_active_op
            .transition(PublicOpEdge::BeginSendPayload);
        if borrowed.lease() != PublicOpLease::Held {
            crate::invariant();
        }
        io.public_active_op = borrowed.phase();
        Ok(())
    }

    pub(in crate::endpoint::kernel::core) fn preflight_transfer_out(
        &self,
        transfer: EndpointTransfer,
        role: u8,
    ) -> SendResult<()> {
        let view = self.outgoing_transfer_view(transfer, role)?;
        // SAFETY: the same actor lease excludes concurrent payload mutation.
        if unsafe { &*view.io.get() }.public_active_op != PublicActiveOp::SendPayload {
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
                let io = unsafe { &mut *view.io.get() };
                let released = io
                    .public_active_op
                    .transition(PublicOpEdge::FinishSendPayload);
                if released.lease() != PublicOpLease::Held {
                    crate::invariant();
                }
                io.public_active_op = released.phase();
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
