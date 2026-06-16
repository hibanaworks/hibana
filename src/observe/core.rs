//! Observation primitives (tap ring and association snapshots).
//!
//! Tap is Hibana runtime evidence, not an application logger. The ring keeps a
//! fixed 64-event postmortem window of 16-byte records for endpoint, transport,
//! lane, route, and resolver evidence.

use core::{cell::Cell, marker::PhantomData};

use crate::runtime_core::consts::RING_EVENTS;

pub use crate::observe::event::{Evidence, TapEvent};

/// Single-producer event ring buffer storage suited for DMA/SHM environments.
struct RingBuffer<'a> {
    head: Cell<usize>,
    storage: *mut TapEvent,
    _marker: PhantomData<&'a mut [TapEvent; RING_EVENTS]>,
    _no_send_sync: PhantomData<*mut ()>,
}

pub(crate) fn emit(ring: &TapRing<'_>, event: TapEvent) {
    ring.push(event);
}

impl<'a> RingBuffer<'a> {
    fn from_ptr(storage: *mut TapEvent) -> Self {
        Self {
            head: Cell::new(0),
            storage,
            _marker: PhantomData,
            _no_send_sync: PhantomData,
        }
    }

    /// Append an observation.
    fn push(&self, event: TapEvent) {
        let head = self.head.get();
        let idx = head % RING_EVENTS;
        self.head.set(head.wrapping_add(1));
        /* SAFETY: `idx` is bounded by `RING_EVENTS`, and `storage` was
         * derived from a mutable slice with at least that many `TapEvent`
         * slots. `TapEvent` has no drop glue, so overwriting the ring slot is
         * sound; `RingBuffer` is single-producer and not `Sync`.
         */
        unsafe {
            self.storage.add(idx).write(event);
        }
    }

    fn port(&self) -> RingPort<'_> {
        let head = self.head.get();
        let cursor = head.saturating_sub(RING_EVENTS);
        RingPort {
            head: &self.head,
            storage: self.storage.cast_const(),
            cursor,
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
        if head.wrapping_sub(self.cursor) > RING_EVENTS {
            self.cursor = head.wrapping_sub(RING_EVENTS);
        }
    }

    #[inline]
    fn peek(&mut self) -> Option<TapEvent> {
        let head = self.head.get();
        self.normalize_cursor(head);
        if self.cursor == head {
            return None;
        }
        let index = self.cursor % RING_EVENTS;
        Some(unsafe {
            // SAFETY: `index` is bounded by `RING_EVENTS`, and `storage`
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
    ring: RingPort<'a>,
}

impl Iterator for TapPort<'_> {
    type Item = TapEvent;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.ring.next()
    }
}

/// Fixed-size runtime evidence ring.
pub(crate) struct TapRing<'a> {
    ring: RingBuffer<'a>,
}

impl<'a> TapRing<'a> {
    pub(crate) fn from_storage(storage: &'a mut [TapEvent; RING_EVENTS]) -> Self {
        Self {
            ring: RingBuffer::from_ptr(storage.as_mut_ptr()),
        }
    }

    /// Append runtime evidence.
    pub(crate) fn push(&self, event: TapEvent) {
        self.ring.push(event);
    }

    pub(crate) fn port(&self) -> TapPort<'_> {
        TapPort {
            ring: self.ring.port(),
        }
    }
}

// Canonical tap identifiers live in [`crate::observe::ids`].
