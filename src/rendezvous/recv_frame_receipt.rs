use core::cell::Cell;

pub(in crate::rendezvous) struct RecvFrameReceiptState {
    outstanding: Cell<bool>,
}

pub(in crate::rendezvous) struct PortRecvFrameReceipt {
    port_key: *const (),
    state: *const RecvFrameReceiptState,
}

impl RecvFrameReceiptState {
    #[inline]
    pub(in crate::rendezvous) const fn new() -> Self {
        Self {
            outstanding: Cell::new(false),
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn issue(&self, port_key: *const ()) -> PortRecvFrameReceipt {
        if self.outstanding.replace(true) {
            crate::invariant();
        }
        PortRecvFrameReceipt {
            port_key,
            state: core::ptr::from_ref(self),
        }
    }

    #[inline]
    fn resolve(&self) {
        if !self.outstanding.get() {
            crate::invariant();
        }
        self.outstanding.set(false);
    }

    #[inline]
    fn assert_current(&self) {
        if !self.outstanding.get() {
            crate::invariant();
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn has_outstanding(&self) -> bool {
        self.outstanding.get()
    }
}

impl PortRecvFrameReceipt {
    #[inline]
    pub(in crate::rendezvous) const fn is_current(&self) -> bool {
        !self.state.is_null()
    }

    #[inline]
    pub(in crate::rendezvous) fn resolve(&mut self) {
        if !self.state.is_null() {
            // SAFETY: receipt construction stores a valid pointer to the
            // port-local receipt state, and clearing `state` ensures one-shot
            // resolution.
            unsafe { &*self.state }.resolve();
            self.port_key = core::ptr::null();
            self.state = core::ptr::null();
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn assert_matches(
        &self,
        port_key: *const (),
        receipt_state: *const RecvFrameReceiptState,
    ) {
        if self.state.is_null() {
            return;
        }
        if self.port_key != port_key {
            crate::invariant();
        }
        if self.state != receipt_state {
            crate::invariant();
        }
        // SAFETY: the receipt stores a pointer to this port's receipt state.
        // `assert_matches` has just proven both the port identity and the
        // state pointer identity before reading the state.
        unsafe { &*self.state }.assert_current();
    }
}
