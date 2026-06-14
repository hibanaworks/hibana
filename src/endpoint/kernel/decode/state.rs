use crate::{
    endpoint::kernel::{core::MaterializedRouteBranch, lane_port},
    global::typestate::RecvMeta,
};

pub(crate) struct DecodeState<'r> {
    pub(crate) branch: Option<MaterializedRouteBranch<'r>>,
    prepared_meta: Option<RecvMeta>,
    pending_recv: lane_port::PendingRecv,
    restore_on_drop: DecodeRestore,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DecodeRestore {
    Disarmed,
    Armed,
}

impl<'r> DecodeState<'r> {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            branch: None,
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: DecodeRestore::Disarmed,
        }
    }

    #[inline]
    pub(crate) fn new(branch: MaterializedRouteBranch<'r>) -> Self {
        Self {
            branch: Some(branch),
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: DecodeRestore::Armed,
        }
    }

    #[inline]
    pub(crate) const fn restore_on_drop(&self) -> DecodeRestore {
        self.restore_on_drop
    }

    #[inline]
    pub(crate) fn disarm_restore(&mut self) {
        self.restore_on_drop = DecodeRestore::Disarmed;
    }

    #[inline]
    pub(crate) fn branch(&self) -> Option<&MaterializedRouteBranch<'r>> {
        self.branch.as_ref()
    }

    #[inline]
    pub(crate) fn branch_mut(&mut self) -> Option<&mut MaterializedRouteBranch<'r>> {
        self.branch.as_mut()
    }

    #[inline]
    pub(crate) fn take_branch(&mut self) -> Option<MaterializedRouteBranch<'r>> {
        self.branch.take()
    }

    #[inline]
    pub(crate) fn discard_terminal(&mut self) {
        if let Some(branch) = self.branch.take() {
            branch.discard_terminal();
        }
        self.disarm_restore();
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
