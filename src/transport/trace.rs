//! Observability helpers for tap/trace flows.
//!
//! These structures exist purely for tap/forward/test code paths so that the
//! runtime can reconstruct the logical header seen by Hibana without forcing
//! transports to carry those fields on the wire. Transports remain payload-only;
//! metadata is assembled at the Endpoint layer and emitted directly to Tap.

use core::fmt;

use super::wire::{FrameFlags, Payload};

/// Tap-only metadata assembled by the Endpoint layer for observability.
///
/// This keeps typestate-derived label/flag selections together for Tap events.
/// Never passed to Transport; emitted directly via `emit_endpoint_event`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TapFrameMeta {
    pub sid: u32,
    pub lane: u8,
    pub role: u8,
    pub label: u8,
    pub flags: FrameFlags,
}

impl TapFrameMeta {
    #[inline(always)]
    pub const fn new(sid: u32, lane: u8, role: u8, label: u8, flags: FrameFlags) -> Self {
        Self {
            sid,
            lane,
            role,
            label,
            flags,
        }
    }
}

/// Tap frame = metadata + payload (never serialized as-is on the wire).
pub struct TapFrame<'a> {
    pub meta: TapFrameMeta,
    pub payload: Payload<'a>,
}

impl<'a> TapFrame<'a> {
    #[inline]
    pub const fn new(meta: TapFrameMeta, payload: Payload<'a>) -> Self {
        Self { meta, payload }
    }
}

impl<'a> fmt::Debug for TapFrame<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TapFrame")
            .field("meta", &self.meta)
            .field("payload", &self.payload)
            .finish()
    }
}
