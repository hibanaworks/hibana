use super::{Payload, lane_port};

pub(super) enum BranchRecvCommitPayload<'r> {
    Wire(lane_port::ReceivedFrame<'r>),
    NonWire(Payload<'r>),
}

impl<'r> BranchRecvCommitPayload<'r> {
    pub(super) fn into_payload(self) -> Payload<'r> {
        match self {
            Self::Wire(frame) => frame.into_payload(),
            Self::NonWire(payload) => payload,
        }
    }

    pub(super) fn discard_uncommitted(self) {
        match self {
            Self::Wire(frame) => frame.discard_uncommitted(),
            Self::NonWire(_) => {}
        }
    }
}
