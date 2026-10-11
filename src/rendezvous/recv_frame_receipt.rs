use core::{cell::Cell, ptr::NonNull};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecvFrameReceiptPhase {
    Idle,
    Outstanding,
}

pub(in crate::rendezvous) struct RecvFrameReceiptState {
    phase: Cell<RecvFrameReceiptPhase>,
}

#[derive(Clone, Copy)]
struct RecvFrameReceiptOwner {
    port_key: NonNull<()>,
    state: NonNull<RecvFrameReceiptState>,
}

pub(in crate::rendezvous) struct PortRecvFrameReceipt {
    owner: Option<RecvFrameReceiptOwner>,
}

impl RecvFrameReceiptState {
    #[inline]
    pub(in crate::rendezvous) const fn new() -> Self {
        Self {
            phase: Cell::new(RecvFrameReceiptPhase::Idle),
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn issue(&self, port_key: NonNull<()>) -> PortRecvFrameReceipt {
        if self.phase.replace(RecvFrameReceiptPhase::Outstanding) != RecvFrameReceiptPhase::Idle {
            crate::invariant();
        }
        PortRecvFrameReceipt {
            owner: Some(RecvFrameReceiptOwner {
                port_key,
                state: NonNull::from(self),
            }),
        }
    }

    #[inline]
    fn resolve(&self) {
        if self.phase.replace(RecvFrameReceiptPhase::Idle) != RecvFrameReceiptPhase::Outstanding {
            crate::invariant();
        }
    }

    #[inline]
    fn assert_current(&self) {
        if self.phase.get() != RecvFrameReceiptPhase::Outstanding {
            crate::invariant();
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn has_outstanding(&self) -> bool {
        self.phase.get() == RecvFrameReceiptPhase::Outstanding
    }
}

impl PortRecvFrameReceipt {
    #[inline]
    pub(in crate::rendezvous) const fn is_current(&self) -> bool {
        self.owner.is_some()
    }

    #[inline]
    pub(in crate::rendezvous) fn resolve(&mut self) {
        let owner = match self.owner.take() {
            Some(owner) => owner,
            None => crate::invariant(),
        };
        // SAFETY: receipt construction stores a non-null pointer to the
        // port-local state. Taking the owner makes resolution one-shot.
        unsafe { owner.state.as_ref() }.resolve();
    }

    #[inline]
    pub(in crate::rendezvous) fn assert_matches(
        &self,
        port_key: NonNull<()>,
        receipt_state: NonNull<RecvFrameReceiptState>,
    ) {
        let owner = match self.owner {
            Some(owner) => owner,
            None => crate::invariant(),
        };
        if owner.port_key != port_key {
            crate::invariant();
        }
        if owner.state != receipt_state {
            crate::invariant();
        }
        // SAFETY: the receipt stores a pointer to this port's receipt state.
        // `assert_matches` has just proven both the port identity and the
        // state pointer identity before reading the state.
        unsafe { owner.state.as_ref() }.assert_current();
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
