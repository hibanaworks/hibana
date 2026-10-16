//! Lane-bound transport handles. Shared workspace and registry metadata remain
//! in the pinned rendezvous owner.

use super::core::Rendezvous;
use crate::{observe::core::TapRing, session::types::Lane, transport::Transport};
use core::{cell::UnsafeCell, marker::PhantomData, ptr::NonNull};

mod recv_frame;
use self::recv_frame::RecvFrameReceiptState;
pub(crate) use self::recv_frame::{
    FrameMismatch, FrameObservation, PreambleFrame, PreambleObservation, ReceivedFrame,
    transport_frame_tap_event,
};

/// Pinned affine Tx/Rx handles for one logical view and lane.
/// Mutable transport state stays here; the port owns no protocol progress.
pub(crate) struct Port<'r, T: Transport> {
    rendezvous: &'r Rendezvous<'r, 'r, T>,
    tx: UnsafeCell<T::Tx<'r>>,
    rx: UnsafeCell<T::Rx<'r>>,
    pub(crate) lane: Lane,
    _no_send_sync: PhantomData<*mut ()>,
    recv_frame_receipt: RecvFrameReceiptState,
}

pub(crate) struct PortInit<'r, T: Transport> {
    pub(crate) rendezvous: &'r Rendezvous<'r, 'r, T>,
    pub(crate) lane: Lane,
    pub(crate) tx: T::Tx<'r>,
    pub(crate) rx: T::Rx<'r>,
}

impl<'r, T: Transport + 'r> Port<'r, T> {
    pub(crate) fn new(init: PortInit<'r, T>) -> Self {
        let PortInit {
            rendezvous,
            lane,
            tx,
            rx,
        } = init;
        Self {
            rendezvous,
            tx: UnsafeCell::new(tx),
            rx: UnsafeCell::new(rx),
            lane,
            _no_send_sync: PhantomData,
            recv_frame_receipt: RecvFrameReceiptState::new(),
        }
    }

    #[inline]
    fn port_key(port: &Self) -> NonNull<()> {
        NonNull::from(port).cast()
    }

    #[inline]
    pub(crate) fn rendezvous(&self) -> &'r Rendezvous<'r, 'r, T> {
        self.rendezvous
    }

    pub(crate) fn transport(&self) -> &'r T {
        self.rendezvous.transport()
    }

    #[inline]
    pub(crate) fn has_unresolved_recv_frame(&self) -> bool {
        self.recv_frame_receipt.has_outstanding()
    }

    #[inline]
    pub(crate) fn tx_ptr(&self) -> *mut T::Tx<'r> {
        self.tx.get()
    }

    #[inline]
    pub(crate) fn rx_ptr(&self) -> *mut T::Rx<'r> {
        self.rx.get()
    }

    #[inline]
    pub(crate) fn tap(&self) -> &TapRing<'r> {
        self.rendezvous.tap()
    }

    #[inline]
    pub(crate) fn lane(&self) -> Lane {
        self.lane
    }
}
