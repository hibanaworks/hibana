//! Observation primitives (tap ring and association snapshots).
//!
//! Tap is Hibana runtime evidence, not an application logger. The ring keeps a
//! byte-budgeted postmortem window for endpoint, transport, lane, route, and
//! resolver evidence. Timestamps are reconstructed from ring order.

use core::{cell::Cell, marker::PhantomData};

use crate::observe::event::TapRecord;

pub use crate::observe::event::{Evidence, TapEvent};

mod ring_state;

use ring_state::{HeadEra, RingState};

pub(crate) const TAP_RESIDENT_BYTE_LIMIT: usize = 256;
pub(crate) const TAP_EVENTS: usize = TAP_RESIDENT_BYTE_LIMIT / TapRecord::BYTE_LEN;
pub(crate) const TAP_RESIDENT_BYTES: usize = core::mem::size_of::<[TapRecord; TAP_EVENTS]>();
const _: () = assert!(TAP_EVENTS > 0);
const _: () = assert!(TAP_EVENTS <= u8::MAX as usize);
const _: () = assert!(TAP_RESIDENT_BYTES <= TAP_RESIDENT_BYTE_LIMIT);
const _: () = assert!(TAP_RESIDENT_BYTE_LIMIT - TAP_RESIDENT_BYTES < TapRecord::BYTE_LEN);

pub(crate) fn emit(ring: &TapRing<'_>, event: TapEvent) {
    ring.push(event);
}

/// Single-producer event ring buffer storage suited for DMA/SHM environments.
struct RingBuffer<'a> {
    head: Cell<usize>,
    state: Cell<RingState>,
    storage: *mut TapRecord,
    _marker: PhantomData<&'a mut [TapRecord; TAP_EVENTS]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'a> RingBuffer<'a> {
    fn from_ptr(storage: *mut TapRecord) -> Self {
        Self {
            head: Cell::new(0),
            state: Cell::new(RingState::EMPTY),
            storage,
            _marker: PhantomData,
            _no_send_sync: PhantomData,
        }
    }

    /// Append an observation.
    fn push(&self, event: TapEvent) {
        let head = self.head.get();
        let state = self.state.get();
        let idx = state.write_index() as usize;
        /* SAFETY: `idx` is bounded by `TAP_EVENTS`, and `storage` was
         * derived from a mutable slice with exactly that many `TapRecord`
         * slots. `TapRecord` has no drop glue, so overwriting the ring slot is
         * sound; `RingBuffer` is single-producer and not `Sync`.
         */
        unsafe {
            self.storage.add(idx).write(TapRecord::from_event(event));
        }
        self.state.set(state.after_push(head));
        self.head.set(head.wrapping_add(1));
    }

    fn port(&self) -> RingPort<'_> {
        let head = self.head.get();
        let state = self.state.get();
        RingPort {
            head: &self.head,
            state: &self.state,
            storage: self.storage.cast_const(),
            cursor: head.wrapping_sub(state.resident_len() as usize),
            index: state.oldest_index(),
            _marker: PhantomData,
        }
    }
}

struct RingPort<'a> {
    head: &'a Cell<usize>,
    state: &'a Cell<RingState>,
    storage: *const TapRecord,
    cursor: usize,
    index: u8,
    _marker: PhantomData<&'a [TapRecord]>,
}

impl RingPort<'_> {
    #[inline]
    fn normalize_cursor(&mut self, head: usize) {
        let state = self.state.get();
        let distance = head.wrapping_sub(self.cursor);
        if distance > state.resident_len() as usize
            || (distance == 0 && self.index != state.write_index())
        {
            self.cursor = head.wrapping_sub(state.resident_len() as usize);
            self.index = state.oldest_index();
        }
    }

    #[inline]
    fn peek(&mut self) -> Option<TapEvent> {
        let head = self.head.get();
        self.normalize_cursor(head);
        if self.cursor == head {
            return None;
        }
        let index = self.index as usize;
        let record = unsafe {
            // SAFETY: `index` is bounded by `TAP_EVENTS`, and `storage`
            // is the ring storage pointer created from the rendezvous-owned
            // tap buffer.
            core::ptr::read_volatile(self.storage.add(index))
        };
        Some(record.to_event(timestamp(self.cursor, self.state.get().head_era())))
    }

    #[inline]
    fn advance(&mut self) {
        self.cursor = self.cursor.wrapping_add(1);
        self.index = if self.index as usize + 1 == TAP_EVENTS {
            0
        } else {
            self.index + 1
        };
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
    pub(crate) fn from_storage(storage: &'a mut [TapRecord; TAP_EVENTS]) -> Self {
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

#[inline(always)]
const fn timestamp(ordinal: usize, head_era: HeadEra) -> u32 {
    if matches!(head_era, HeadEra::Wrapped) || ordinal > u32::MAX as usize {
        u32::MAX
    } else {
        ordinal as u32
    }
}

// Canonical tap identifiers live in [`crate::observe::ids`].

#[cfg(all(test, hibana_repo_tests))]
mod tests;
