//! Receive-frame receipt authority for a lane port.
//!
//! This module owns the one-shot proof that a transport frame has been
//! received from a specific port/Rx handle and must be committed, requeued, or
//! explicitly discarded.

use core::cell::Cell;

use super::Port;
use crate::{
    control::cap::mint::EpochTable,
    control::types::Lane,
    transport::{Transport, wire::Payload},
};

const RECEIVED_FRAME_CONTRACT: &str =
    "received transport frames must be committed, explicitly requeued, or explicitly discarded";

pub(super) struct RecvFrameReceiptState {
    outstanding: Cell<bool>,
}

struct PortRecvFrameReceipt {
    port_key: *const (),
    state: *const RecvFrameReceiptState,
}

/// Transport frame received from a lane port.
///
/// The payload is accompanied by a one-shot receipt. Endpoint code must choose
/// exactly one terminal action: commit the frame into a payload, requeue it on
/// the same port/Rx handle, or explicitly discard it.
/// If the transport rejects requeue, the receipt is resolved as an explicit
/// discard and the caller must treat the returned error as terminal for that
/// frame.
///
/// Invariant: received transport frames must be committed, explicitly requeued, or explicitly discarded.
pub(crate) struct ReceivedFrame<'r> {
    payload: Payload<'r>,
    lane: Lane,
    receipt: Option<PortRecvFrameReceipt>,
}

impl RecvFrameReceiptState {
    #[inline]
    pub(super) const fn new() -> Self {
        Self {
            outstanding: Cell::new(false),
        }
    }

    #[inline]
    fn issue(&self, port_key: *const ()) -> PortRecvFrameReceipt {
        assert!(
            !self.outstanding.replace(true),
            "transport receive frame polled while previous frame receipt is unresolved",
        );
        PortRecvFrameReceipt {
            port_key,
            state: core::ptr::from_ref(self),
        }
    }

    #[inline]
    fn resolve(&self) {
        assert!(
            self.outstanding.get(),
            "transport receive frame receipt is no longer current",
        );
        self.outstanding.set(false);
    }

    #[inline]
    fn assert_current(&self) {
        assert!(
            self.outstanding.get(),
            "transport receive frame receipt is no longer current",
        );
    }
}

impl PortRecvFrameReceipt {
    #[inline]
    fn assert_matches<'r, T, E>(&self, lane: Lane, port: &Port<'r, T, E>)
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        assert_eq!(
            lane,
            port.lane(),
            "received transport frame requeued on a different lane",
        );
        assert_eq!(
            self.port_key,
            Port::port_key(port),
            "received transport frame requeued on a different endpoint port",
        );
        assert_eq!(
            self.state,
            core::ptr::from_ref(&port.recv_frame_receipt),
            "received transport frame requeued on a different Rx handle",
        );
        // SAFETY: the receipt stores a pointer to this port's receipt state.
        // `assert_matches` has just proven both the port identity and the
        // state pointer identity before reading the state.
        unsafe { &*self.state }.assert_current();
    }
}

impl<'r> ReceivedFrame<'r> {
    #[inline]
    pub(crate) fn from_port<T, E>(port: &Port<'r, T, E>, payload: Payload<'r>) -> Self
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        Self {
            payload,
            lane: port.lane(),
            receipt: Some(port.recv_frame_receipt.issue(Port::port_key(port))),
        }
    }

    #[inline]
    pub(crate) const fn lane_idx(&self) -> usize {
        self.lane.raw() as usize
    }

    #[inline]
    pub(crate) const fn lane_wire(&self) -> u8 {
        self.lane.as_wire()
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.payload.as_bytes().is_empty()
    }

    #[inline]
    pub(crate) fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'r>, E>
    where
        F: FnOnce(Payload<'r>) -> Result<(), E>,
    {
        validate(self.payload)?;
        Ok(self.payload)
    }

    #[inline]
    pub(crate) fn into_payload(mut self) -> Payload<'r> {
        self.consume_receipt();
        self.payload
    }

    #[inline]
    pub(crate) fn discard_uncommitted(mut self) {
        self.consume_receipt();
    }

    #[inline]
    pub(crate) fn requeue_on<T, E>(
        mut self,
        port: &Port<'r, T, E>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.assert_matches_port(port);
        let transport = port.transport();
        let rx_ptr = port.rx_ptr();
        let result = unsafe {
            // SAFETY: the frame receipt was issued by this exact port/Rx handle
            // and `assert_matches_port` above proved the lane, port identity,
            // receipt-state pointer, and outstanding receipt state before
            // requeueing.
            transport.requeue(&mut *rx_ptr).map_err(Into::into)
        };
        match result {
            Ok(()) => {
                self.consume_receipt();
                Ok(())
            }
            Err(err) => {
                self.discard_after_failed_requeue();
                Err(err)
            }
        }
    }

    #[inline]
    fn discard_after_failed_requeue(&mut self) {
        self.consume_receipt();
    }

    #[inline]
    fn consume_receipt(&mut self) {
        if let Some(receipt) = self.receipt.take() {
            // SAFETY: receipt construction stores the address of the port-local
            // receipt state, and the state admits only one outstanding frame.
            unsafe { &*receipt.state }.resolve();
        }
    }

    #[inline]
    pub(crate) fn assert_matches_port<T, E>(&self, port: &Port<'r, T, E>)
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        if let Some(receipt) = self.receipt.as_ref() {
            receipt.assert_matches(self.lane, port);
        }
    }
}

impl Drop for ReceivedFrame<'_> {
    fn drop(&mut self) {
        assert!(
            self.receipt.is_none(),
            "{}: received transport frame dropped without explicit commit, requeue, or discard",
            RECEIVED_FRAME_CONTRACT,
        );
    }
}
