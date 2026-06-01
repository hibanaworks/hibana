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
#[cfg(not(all(test, hibana_repo_tests)))]
use core::{cell::UnsafeCell, ptr};

use crate::{
    observe::ids,
    runtime::consts::{RING_BUFFER_SIZE, RING_EVENTS},
    transport::wire::{CodecError, Payload, WireEncode, WirePayload, require_exact_len},
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

/// 20-byte tap record with causal key tracking for roll-π reversibility.
///
/// Layout: `ts32, id16, causal_key16, arg0_32, arg1_32, arg2_32`
/// - `ts`: Timestamp (monotonic counter or wall-clock tick)
/// - `id`: Event identifier (from `crate::observe::ids::*`)
/// - `causal_key`: Causal key for reversible rollback tracking (roll-π)
///   - High 8 bits: role/lane index
///   - Low 8 bits: sequence number within epoch
/// - `arg0`, `arg1`: Context-dependent arguments (sid, gen, label, etc.)
/// - `arg2`: Extended context (e.g., ScopeId range/nest ordinals)
///
/// **Future extension**: For roll-π memory tracking, `causal_key` encodes
/// the (role, seq) pair that establishes causal dependencies. Rollback
/// operations can reconstruct causal history by following these keys.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TapEvent {
    pub ts: u32,
    pub id: u16,
    pub causal_key: u16,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
}

impl TapEvent {
    #[inline]
    pub const fn with_arg0(mut self, arg0: u32) -> Self {
        self.arg0 = arg0;
        self
    }

    #[inline]
    pub const fn with_arg1(mut self, arg1: u32) -> Self {
        self.arg1 = arg1;
        self
    }

    #[inline]
    pub const fn with_arg2(mut self, arg2: u32) -> Self {
        self.arg2 = arg2;
        self
    }

    #[inline]
    pub const fn with_causal_key(mut self, causal_key: u16) -> Self {
        self.causal_key = causal_key;
        self
    }

    /// Extract role/lane from causal key (high 8 bits).
    #[inline]
    pub const fn causal_role(self) -> u8 {
        (self.causal_key >> 8) as u8
    }

    /// Extract sequence number from causal key (low 8 bits).
    #[inline]
    pub const fn causal_seq(self) -> u8 {
        (self.causal_key & 0xFF) as u8
    }

    /// Construct causal key from role and sequence.
    #[inline]
    pub const fn make_causal_key(role: u8, seq: u8) -> u16 {
        ((role as u16) << 8) | (seq as u16)
    }

    /// Create a zeroed event (for array initialization).
    #[inline]
    pub const fn zero() -> Self {
        Self {
            ts: 0,
            id: 0,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }
}

impl WireEncode for TapEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(20)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 20 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.ts.to_be_bytes());
        out[4..6].copy_from_slice(&self.id.to_be_bytes());
        out[6..8].copy_from_slice(&self.causal_key.to_be_bytes());
        out[8..12].copy_from_slice(&self.arg0.to_be_bytes());
        out[12..16].copy_from_slice(&self.arg1.to_be_bytes());
        out[16..20].copy_from_slice(&self.arg2.to_be_bytes());
        Ok(20)
    }
}

impl WirePayload for TapEvent {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        require_exact_len(input.as_bytes().len(), 20, "payload length")
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        Self {
            ts: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            id: u16::from_be_bytes([bytes[4], bytes[5]]),
            causal_key: u16::from_be_bytes([bytes[6], bytes[7]]),
            arg0: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            arg1: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            arg2: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        }
    }
}

/// Single-producer event ring buffer storage suited for DMA/SHM environments.
struct RingBuffer<'a> {
    head: Cell<usize>,
    storage: *mut TapEvent,
    _marker: PhantomData<&'a mut [TapEvent]>,
    _no_send_sync: PhantomData<*mut ()>,
}

struct GlobalTap {
    #[cfg(not(all(test, hibana_repo_tests)))]
    ring: UnsafeCell<*mut TapRing<'static>>,
}

impl GlobalTap {
    const fn new() -> Self {
        Self {
            #[cfg(not(all(test, hibana_repo_tests)))]
            ring: UnsafeCell::new(ptr::null_mut()),
        }
    }

    fn with_ring<R>(&self, f: impl FnOnce(&TapRing<'static>) -> R) -> Option<R> {
        #[cfg(all(test, hibana_repo_tests))]
        let ptr = tests::global_tap_ring_ptr();
        #[cfg(not(all(test, hibana_repo_tests)))]
        let ptr = /* SAFETY: the tap ring owns the ring buffer pointer and serializes access through its UnsafeCell owner. */ unsafe { *self.ring.get() };
        if ptr.is_null() {
            None
        } else {
            Some(
                /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
                unsafe { f(&*ptr) },
            )
        }
    }

    fn invoke_post(&self, event: &TapEvent) {
        let _ = event;
    }
}

static GLOBAL_TAP: GlobalTap = GlobalTap::new();
unsafe impl Sync for GlobalTap {}

pub(crate) fn push(event: TapEvent) {
    let _ = GLOBAL_TAP.with_ring(|ring| {
        ring.push(event);
    });
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
        #[cfg(all(test, hibana_repo_tests))]
        {
            tests::check_event_timestamp(event.ts);
        }
        /* SAFETY: `idx` is bounded by `RING_BUFFER_SIZE`, and `storage` was
         * derived from a mutable slice with at least that many `TapEvent`
         * slots. `TapEvent` has no drop glue, so overwriting the ring slot is
         * sound; `RingBuffer` is single-producer and not `Sync`.
         */
        unsafe {
            self.storage.add(idx).write(event);
        }
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

        GLOBAL_TAP.invoke_post(&event);
    }
}

// Canonical tap identifiers are generated at build time (see [`crate::observe::ids`]).
//
// # Event ID Ranges (Dual-Ring Routing)
//
// - `0x0000..0x00FF`: **User Ring** — application/custom events
// - `0x0100..0x013F`: State coordination
// - `0x0200..0x020F`: Abort / Endpoint / Topology core events
// - `0x0210..0x021F`: Lane lifecycle
// - `0x0220..0x022F`: Loop / Route control (LOOP_DECISION, ROUTE_DECISION)
// - `0x0230..0x023F`: Decision-policy staging (DECISION_PICK)
// - `0x0400..0x041F`: Policy VM events (ABORT, ANNOT, TRAP, EFFECT, COMMIT)
// - `0x0500`: Effect initialization (EFFECT_INIT)
// - `0x02FF`: Misuse detection (MISUSE_RECVGUARD_DROP)
