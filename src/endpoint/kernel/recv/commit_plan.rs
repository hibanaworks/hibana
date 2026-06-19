use super::{RecvDescriptor, lane_port};
use crate::{
    endpoint::kernel::core::PreparedCommitDelta,
    endpoint::{RecvError, RecvResult},
    transport::wire::{CodecError, Payload},
};

pub(super) struct RecvCommitPlan<'a> {
    pub(super) desc: RecvDescriptor,
    pub(super) frame: lane_port::ReceivedFrame<'a>,
    pub(super) delta: PreparedCommitDelta,
}

impl<'a> RecvCommitPlan<'a> {
    pub(super) fn new(
        desc: RecvDescriptor,
        frame: lane_port::ReceivedFrame<'a>,
        delta: PreparedCommitDelta,
        validate: for<'p> fn(Payload<'p>) -> Result<(), CodecError>,
    ) -> RecvResult<Self> {
        if let Err(err) = frame.validated_payload(validate) {
            frame.discard_uncommitted();
            return Err(RecvError::Codec(err));
        }
        Ok(Self { desc, frame, delta })
    }
}
