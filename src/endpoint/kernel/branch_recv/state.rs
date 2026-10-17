use crate::endpoint::kernel::lane_port;

pub(crate) struct BranchRecvState {
    pending_recv: lane_port::PendingRecv,
}

impl BranchRecvState {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(crate) const fn armed() -> Self {
        Self {
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(crate) fn pending_recv_mut(&mut self) -> &mut lane_port::PendingRecv {
        &mut self.pending_recv
    }
}
