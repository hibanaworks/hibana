//! Observation primitives (tap ring and association snapshots).
//!
//! The current implementation maintains a fixed-length dual-ring buffer of
//! 20-byte tap events. Each entry captures coarse timing plus up to three
//! contextual arguments. This is intentionally small so that the ring fits
//! comfortably in cache or DMA-able memory regions.
//!
//! # Dual-Ring Architecture
//!
//! Events are routed to separate rings based on ID range:
//! - **User Ring** (`0x0000..0x00FF`): application/custom events
//! - **Infra Ring** (`0x0100..0xFFFF`): System events (ENDPOINT_SEND, etc.)
//!
//! This separation prevents Observer Effect feedback loops where streaming
//! infrastructure events would otherwise flood the ring.
//!
use core::{cell::Cell, marker::PhantomData};

use crate::{
    observe::ids,
    runtime_core::consts::{RING_BUFFER_SIZE, RING_EVENTS},
};

pub use crate::observe::event::{Evidence, TapEvent};

/// Single-producer event ring buffer storage suited for DMA/SHM environments.
struct RingBuffer<'a> {
    head: Cell<usize>,
    storage: *mut TapEvent,
    _marker: PhantomData<&'a mut [TapEvent]>,
    _no_send_sync: PhantomData<*mut ()>,
}

pub(crate) fn emit(ring: &TapRing<'_>, event: TapEvent) {
    ring.push(event);
}

impl<'a> RingBuffer<'a> {
    fn new(storage: &'a mut [TapEvent]) -> Self {
        assert!(storage.len() >= RING_BUFFER_SIZE);
        Self {
            head: Cell::new(0),
            storage: storage.as_mut_ptr(),
            _marker: PhantomData,
            _no_send_sync: PhantomData,
        }
    }

    /// Append an observation.
    fn push(&self, event: TapEvent) {
        let head = self.head.get();
        let idx = head % RING_BUFFER_SIZE;
        self.head.set(head.wrapping_add(1));
        /* SAFETY: `idx` is bounded by `RING_BUFFER_SIZE`, and `storage` was
         * derived from a mutable slice with at least that many `TapEvent`
         * slots. `TapEvent` has no drop glue, so overwriting the ring slot is
         * sound; `RingBuffer` is single-producer and not `Sync`.
         */
        unsafe {
            self.storage.add(idx).write(event);
        }
    }

    fn port(&self) -> RingPort<'_> {
        RingPort {
            head: &self.head,
            storage: self.storage.cast_const(),
            cursor: self.head.get(),
            _marker: PhantomData,
        }
    }
}

struct RingPort<'a> {
    head: &'a Cell<usize>,
    storage: *const TapEvent,
    cursor: usize,
    _marker: PhantomData<&'a [TapEvent]>,
}

impl RingPort<'_> {
    #[inline]
    fn normalize_cursor(&mut self, head: usize) {
        if head.wrapping_sub(self.cursor) > RING_BUFFER_SIZE {
            self.cursor = head.wrapping_sub(RING_BUFFER_SIZE);
        }
    }

    #[inline]
    fn peek(&mut self) -> Option<TapEvent> {
        let head = self.head.get();
        self.normalize_cursor(head);
        if self.cursor == head {
            return None;
        }
        let index = self.cursor % RING_BUFFER_SIZE;
        Some(unsafe {
            // SAFETY: `index` is bounded by `RING_BUFFER_SIZE`, and `storage`
            // is the ring storage pointer created from the rendezvous-owned
            // tap buffer.
            core::ptr::read_volatile(self.storage.add(index))
        })
    }

    #[inline]
    fn advance(&mut self) {
        self.cursor = self.cursor.wrapping_add(1);
    }

    fn next(&mut self) -> Option<TapEvent> {
        let event = self.peek()?;
        self.advance();
        Some(event)
    }
}

pub struct TapPort<'a> {
    user: RingPort<'a>,
    infra: RingPort<'a>,
}

impl Iterator for TapPort<'_> {
    type Item = TapEvent;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match (self.user.peek(), self.infra.peek()) {
            (Some(user), Some(infra)) => {
                if tap_event_precedes(user, infra) {
                    self.user.advance();
                    Some(user)
                } else {
                    self.infra.advance();
                    Some(infra)
                }
            }
            (Some(_user), None) => self.user.next(),
            (None, Some(_infra)) => self.infra.next(),
            (None, None) => None,
        }
    }
}

#[inline(always)]
const fn tap_event_precedes(left: TapEvent, right: TapEvent) -> bool {
    if left.ts != right.ts {
        left.ts < right.ts
    } else {
        left.id <= right.id
    }
}

/// Dual-ring buffer separating User (Application) and Infra (System) events.
pub(crate) struct TapRing<'a> {
    user: RingBuffer<'a>,
    infra: RingBuffer<'a>,
}

impl<'a> TapRing<'a> {
    pub(crate) fn from_storage(storage: &'a mut [TapEvent; RING_EVENTS]) -> Self {
        let (user_slice, infra_slice) = storage.split_at_mut(RING_BUFFER_SIZE);
        Self {
            user: RingBuffer::new(user_slice),
            infra: RingBuffer::new(infra_slice),
        }
    }

    /// Append an observation (routing to appropriate ring).
    ///
    /// Events are routed based on ID range:
    /// - `id < USER_EVENT_RANGE_END` (0x0100): User Ring (application/custom events)
    /// - `id >= USER_EVENT_RANGE_END`: Infra Ring (system events)
    pub(crate) fn push(&self, event: TapEvent) {
        if event.id < ids::USER_EVENT_RANGE_END {
            self.user.push(event);
        } else {
            self.infra.push(event);
        }
    }

    pub(crate) fn port(&self) -> TapPort<'_> {
        TapPort {
            user: self.user.port(),
            infra: self.infra.port(),
        }
    }
}

// Canonical tap identifiers are generated at build time (see [`crate::observe::ids`]).
//
// # Event ID Ranges (Dual-Ring Routing)
//
// - `0x0000..0x00FF`: **User Ring** — application/custom events
// - `0x0100..0x013F`: Coordination events
// - `0x0200..0x020F`: Endpoint core events
// - `0x0210..0x021F`: Lane lifecycle
// - `0x0220..0x022F`: Route decision (ROUTE_ARM_SELECTION)
// - `0x0230..0x023F`: Decision-resolver staging (DECISION_PICK)
// - `0x0400..0x041F`: Resolver VM events (ABORT, ANNOT, TRAP, EFFECT, COMMIT)
// - `0x02FF`: Misuse detection (MISUSE_RECVGUARD_DROP)
