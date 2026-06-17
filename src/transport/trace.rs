//! Observability helpers for tap/trace flows.
//!
//! These structures exist purely for tap/forward observability paths so that the
//! runtime can reconstruct the logical header seen by Hibana without forcing
//! transports to carry those fields on the wire. Transports remain payload-only;
//! metadata is assembled at the Endpoint layer and emitted directly to Tap.

/// Tap-only metadata assembled by the Endpoint layer for observability.
///
/// This keeps typestate-derived label selections together for Tap events.
/// Never passed to Transport; emitted directly via `emit_endpoint_event`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TapFrameMeta {
    pub(crate) sid: u32,
    pub(crate) lane: u8,
    pub(crate) role: u8,
    pub(crate) label: u8,
}

impl TapFrameMeta {
    #[inline(always)]
    pub(crate) const fn new(sid: u32, lane: u8, role: u8, label: u8) -> Self {
        Self {
            sid,
            lane,
            role,
            label,
        }
    }
}
