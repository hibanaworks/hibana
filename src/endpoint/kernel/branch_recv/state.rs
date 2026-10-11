use crate::{endpoint::kernel::lane_port, global::typestate::RecvMeta};

pub(crate) struct BranchRecvState {
    prepared_meta: Option<RecvMeta>,
    pending_recv: lane_port::PendingRecv,
}

impl BranchRecvState {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(crate) const fn armed() -> Self {
        Self {
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(crate) fn prepared_meta(&self) -> Option<RecvMeta> {
        self.prepared_meta
    }

    #[inline]
    pub(crate) fn set_prepared_meta(&mut self, prepared_meta: Option<RecvMeta>) {
        self.prepared_meta = prepared_meta;
    }

    #[inline]
    pub(crate) fn pending_recv_mut(&mut self) -> &mut lane_port::PendingRecv {
        &mut self.pending_recv
    }
}
