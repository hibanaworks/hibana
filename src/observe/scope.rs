//! Scope metadata helpers for tap events and normalised traces.
//!
//! Scope range/nest ordinals are encoded into tap `arg2` fields so that
//! observers can reconstruct structured scopes. This module provides
//! decoding/encoding utilities and `TapEvent` extraction helpers without
//! allocation.

/// Structured scope trace (range/nest ordinals) attached to tap events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScopeTrace {
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
    pub(crate) const fn new(range: u16, nest: u16) -> Self {
        Self { range, nest }
    }

    /// Encode this trace into the packed `u32` representation used by taps.
    #[inline]
    pub(crate) const fn pack(self) -> u32 {
        0x8000_0000 | ((self.range as u32) << 16) | (self.nest as u32)
    }
}
