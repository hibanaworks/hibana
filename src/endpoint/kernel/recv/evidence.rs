use super::lane_port;
use crate::global::typestate::{RecvMeta, StateIndex};

pub(crate) struct MatchedRecvFrame<'a> {
    pub(super) desc: RecvDescriptor,
    pub(super) frame: lane_port::ReceivedFrame<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::endpoint::kernel::recv) struct RecvCandidate {
    pub(in crate::endpoint::kernel::recv) desc: RecvDescriptor,
    // One synchronous kernel observation. A recognized early frame is retained
    // until its prefix completes; only an enabled candidate can be consumed.
    pub(in crate::endpoint::kernel::recv) admission:
        Result<(), crate::global::typestate::EventAdmissionError>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct RecvDescriptor {
    pub(in crate::endpoint::kernel) meta: RecvMeta,
    pub(in crate::endpoint::kernel) cursor_index: StateIndex,
}
