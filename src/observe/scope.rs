//! Scope metadata helpers for tap events and normalised traces.
//!
//! Scope range/nest ordinals are encoded into tap `arg2` fields so that
//! observers (CLI, EPF, etc.) can reconstruct structured scopes. This module
//! provides decoding/encoding utilities and `TapEvent` extraction helpers
//! in a no_std compatible manner.

use crate::observe::core::TapEvent;

/// Structured scope trace (range/nest ordinals) attached to tap events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeTrace {
    pub range: u16,
    pub nest: u16,
}

impl Default for ScopeTrace {
    #[inline]
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl ScopeTrace {
    /// Construct a new scope trace from explicit ordinals.
    #[inline]
    pub const fn new(range: u16, nest: u16) -> Self {
        Self { range, nest }
    }

    /// Encode this trace into the packed `u32` representation used by taps.
    #[inline]
    pub const fn pack(self) -> u32 {
        0x8000_0000 | ((self.range as u32) << 16) | (self.nest as u32)
    }

    /// Decode a packed `u32` produced by [`ScopeTrace::pack`].
    #[inline]
    pub const fn decode(packed: u32) -> Option<Self> {
        if (packed & 0x8000_0000) == 0 {
            None
        } else {
            let range = ((packed & 0x7FFF_0000) >> 16) as u16;
            let nest = (packed & 0x0000_FFFF) as u16;
            Some(Self::new(range, nest))
        }
    }
}

/// Extract the scope trace encoded in a tap event's `arg2` field.
#[inline]
pub fn tap_scope(event: &TapEvent) -> Option<ScopeTrace> {
    ScopeTrace::decode(event.arg2)
}
