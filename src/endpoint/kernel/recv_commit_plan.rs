use super::{
    core::{OperationKernel, PreparedCommitDelta, PreparedViewCommit},
    lane_port,
    offer::BranchMeta,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    observe::ids,
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

pub(super) enum RecvCommitPayload<'r> {
    Wire(lane_port::ReceivedFrame<'r>),
    NonWire(Payload<'r>),
}

pub(super) struct RecvCommitPlan<'r> {
    kind: RecvCommitPlanKind,
    delta: PreparedCommitDelta,
    payload: RecvCommitPayload<'r>,
}

enum RecvCommitPlanKind {
    Direct,
    Branch { branch: BranchMeta },
}

impl<'r> RecvCommitPayload<'r> {
    #[inline]
    pub(super) const fn wire(frame: lane_port::ReceivedFrame<'r>) -> Self {
        Self::Wire(frame)
    }

    #[inline]
    pub(super) const fn non_wire(payload: Payload<'r>) -> Self {
        Self::NonWire(payload)
    }

    fn validate<F, E>(&self, validate: F) -> Result<(), E>
    where
        F: FnOnce(Payload<'r>) -> Result<(), E>,
    {
        match self {
            Self::Wire(frame) => frame.validated_payload(validate).map(|_| ()),
            Self::NonWire(payload) => validate(*payload),
        }
    }

    fn discard_uncommitted(self) {
        match self {
            Self::Wire(frame) => frame.discard_uncommitted(),
            Self::NonWire(_) => {}
        }
    }

    fn into_payload(self) -> Payload<'r> {
        match self {
            Self::Wire(frame) => frame.into_payload(),
            Self::NonWire(payload) => payload,
        }
    }
}

impl<'r> RecvCommitPlan<'r> {
    #[inline]
    pub(super) const fn direct(
        delta: PreparedCommitDelta,
        frame: lane_port::ReceivedFrame<'r>,
    ) -> Self {
        Self {
            kind: RecvCommitPlanKind::Direct,
            delta,
            payload: RecvCommitPayload::Wire(frame),
        }
    }

    #[inline]
    pub(super) const fn branch(
        branch: BranchMeta,
        delta: PreparedCommitDelta,
        payload: RecvCommitPayload<'r>,
    ) -> Self {
        Self {
            kind: RecvCommitPlanKind::Branch { branch },
            delta,
            payload,
        }
    }
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(super) fn publish_recv_commit_plan<F>(
        &mut self,
        plan: RecvCommitPlan<'r>,
        validate: F,
    ) -> RecvResult<crate::endpoint::transfer::RecvOutput<'r>>
    where
        F: FnOnce(Payload<'r>) -> Result<(), CodecError>,
    {
        let mut issue = None;
        let validation = plan.payload.validate(|payload| {
            if let Some(event) = plan.delta.event() {
                issue = self.prepare_received_view(event.eff_index(), payload)?;
            }
            if issue.is_some() {
                Ok(())
            } else {
                validate(payload).map_err(RecvError::Codec)
            }
        });
        self.reconcile_retirements();
        if let Some(kind) = self.session_fault() {
            plan.payload.discard_uncommitted();
            return Err(RecvError::SessionFault(kind));
        }
        if let Err(err) = validation {
            plan.payload.discard_uncommitted();
            return Err(err);
        }

        let RecvCommitPlan {
            kind,
            delta,
            payload,
        } = plan;
        let payload = payload.into_payload();
        let (committed, owner) =
            self.commit_prepared_delta(delta, issue.map(PreparedViewCommit::Issue));
        let output = match owner {
            Some(owner) => crate::endpoint::transfer::RecvOutput::Endpoint(owner),
            None => crate::endpoint::transfer::RecvOutput::Payload(payload),
        };
        if let RecvCommitPlanKind::Branch { branch } = kind {
            self.publish_branch_preview_commit_plan(branch, &committed);
        }
        if let Some(event) = committed.event() {
            let id = event.event_id(ids::ENDPOINT_RECV, ids::ENDPOINT_SESSION);
            self.emit_endpoint_event(id, event, 2);
        }
        Ok(output)
    }
}
