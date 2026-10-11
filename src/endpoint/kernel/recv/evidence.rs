use super::lane_port;
use crate::global::typestate::{RecvMeta, StateIndex};

pub(crate) struct MatchedRecvFrame<'a> {
    pub(super) desc: RecvDescriptor,
    pub(super) frame: lane_port::ReceivedFrame<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::endpoint::kernel::recv) struct RecvCandidate {
    pub(in crate::endpoint::kernel::recv) desc: RecvDescriptor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::endpoint::kernel::recv) struct RecvDescriptor {
    pub(in crate::endpoint::kernel::recv) meta: RecvMeta,
    pub(in crate::endpoint::kernel::recv) cursor_index: StateIndex,
}
