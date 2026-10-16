use super::{OperationKernel, PublicActiveOp, ResidentActor, Transport};
use crate::eff::{EffIndex, EventPayload};
use crate::endpoint::{
    RecvError, RecvResult, SendError, SendResult, owner::EndpointOwner, transfer::EndpointTransfer,
};
use core::ptr::NonNull;

pub(in crate::endpoint::kernel) struct PreparedViewIssue {
    ptr: NonNull<crate::endpoint::carrier::KernelEndpointHeader>,
    generation: u32,
}

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    pub(in crate::endpoint::kernel::core) fn preflight_transfer_out(
        &self,
        transfer: EndpointTransfer,
        role: u8,
    ) -> SendResult<()> {
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
        let view = actor.views.get(role).ok_or(SendError::PhaseInvariant)?;
        // The common progress borrow excludes another kernel. Moving the
        // Endpoint excludes a public operation/preview borrow of this view.
        if unsafe { &*view.io.get() }.public_active_op != PublicActiveOp::Idle {
            return Err(SendError::PhaseInvariant);
        }
        Ok(())
    }

    pub(in crate::endpoint::kernel::core) fn commit_transfer_out(
        &mut self,
        transfer: EndpointTransfer,
    ) {
        // SAFETY: send preflight proved a live same-actor view; the future
        // keeps its unique owner until this callback-free commit completes.
        let header = unsafe { transfer.ptr.as_ref() };
        if !transfer.handle.matches_header(header) {
            crate::invariant();
        }
        header.retire_generation();
        self.context.live_views.set(crate::invariant_some(
            self.context.live_views.get().checked_sub(1),
        ));
        // Ports and backing storage belong to the actor/run and survive return.
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
        let ordinal = (eff.dense_ordinal() as u16).to_be_bytes();
        if payload.as_bytes() != [role, ordinal[0], ordinal[1]] {
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

    pub(in crate::endpoint::kernel) fn commit_received_view(
        &mut self,
        issue: PreparedViewIssue,
    ) -> EndpointOwner {
        // SAFETY: preflight checked this dormant same-actor record. No callback
        // occurs between this publication and the prepared cursor commit.
        let header = unsafe { issue.ptr.as_ref() };
        header.issue_received(issue.generation);
        self.context.view_generation.set(issue.generation);
        self.context.live_views.set(crate::invariant_some(
            self.context.live_views.get().checked_add(1),
        ));
        EndpointOwner::new(
            issue.ptr,
            crate::endpoint::carrier::EndpointHandle::new(
                self.context.public_generation.get(),
                issue.generation,
            ),
        )
    }
}
