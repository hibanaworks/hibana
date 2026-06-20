use super::{
    core::{CursorEndpoint, PreparedCommitDelta},
    lane_port,
    offer::BranchMeta,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    observe::ids,
    transport::{
        Transport,
        trace::TapFrameMeta,
        wire::{CodecError, Payload},
    },
};

#[derive(Clone, Copy)]
pub(super) struct EndpointRxEventPlan {
    lane: u8,
    label: u8,
    event_id: u16,
}

pub(super) enum RecvCommitPayload<'r> {
    Wire(lane_port::ReceivedFrame<'r>),
    NonWire(Payload<'r>),
}

pub(super) struct RecvCommitPlan<'r> {
    kind: RecvCommitPlanKind,
    event: EndpointRxEventPlan,
    delta: PreparedCommitDelta,
    payload: RecvCommitPayload<'r>,
}

enum RecvCommitPlanKind {
    Direct,
    Branch { branch: BranchMeta },
}

impl EndpointRxEventPlan {
    #[inline]
    pub(super) const fn direct(lane: u8, label: u8) -> Self {
        Self {
            lane,
            label,
            event_id: ids::ENDPOINT_RECV,
        }
    }

    #[inline]
    pub(super) const fn branch(lane: u8, label: u8, origin: crate::eff::EventOrigin) -> Self {
        Self {
            lane,
            label,
            event_id: if origin.is_session() {
                ids::ENDPOINT_SESSION
            } else {
                ids::ENDPOINT_RECV
            },
        }
    }
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

    fn validate<F>(&self, validate: F) -> Result<(), CodecError>
    where
        F: FnOnce(Payload<'r>) -> Result<(), CodecError>,
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
        event: EndpointRxEventPlan,
        delta: PreparedCommitDelta,
        frame: lane_port::ReceivedFrame<'r>,
    ) -> Self {
        Self {
            kind: RecvCommitPlanKind::Direct,
            event,
            delta,
            payload: RecvCommitPayload::Wire(frame),
        }
    }

    #[inline]
    pub(super) const fn branch(
        branch: BranchMeta,
        event: EndpointRxEventPlan,
        delta: PreparedCommitDelta,
        payload: RecvCommitPayload<'r>,
    ) -> Self {
        Self {
            kind: RecvCommitPlanKind::Branch { branch },
            event,
            delta,
            payload,
        }
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn publish_recv_commit_plan<F>(
        &mut self,
        plan: RecvCommitPlan<'r>,
        validate: F,
    ) -> RecvResult<Payload<'r>>
    where
        F: FnOnce(Payload<'r>) -> Result<(), CodecError>,
    {
        if let Err(err) = plan.payload.validate(validate) {
            plan.payload.discard_uncommitted();
            return Err(RecvError::Codec(err));
        }

        let RecvCommitPlan {
            kind,
            event,
            delta,
            payload,
        } = plan;
        match kind {
            RecvCommitPlanKind::Direct => {
                let endpoint_meta =
                    TapFrameMeta::new(self.sid.raw(), event.lane, ROLE, event.label);
                self.emit_endpoint_event(event.event_id, endpoint_meta, event.lane);
                self.commit_prepared_delta(delta);
            }
            RecvCommitPlanKind::Branch { branch } => {
                self.commit_prepared_delta(delta);
                self.publish_branch_preview_commit_plan(branch);
                let endpoint_meta =
                    TapFrameMeta::new(self.sid.raw(), event.lane, ROLE, event.label);
                self.emit_endpoint_event(event.event_id, endpoint_meta, event.lane);
            }
        }
        Ok(payload.into_payload())
    }
}
