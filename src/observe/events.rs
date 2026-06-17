//! TapEvent encoders owned by runtime operations.
//!
//! Production callers use the event owner that matches the runtime operation.

use super::core::TapEvent;
use super::ids;

// ────────────── Endpoint boundary (0x0200-0x020F) ──────────────

// ────────────── Lane lifecycle (0x0210-0x021F) ──────────────

/// Session/lane association count moved 0->1.
#[inline(always)]
pub(crate) const fn lane_acquire(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let rv_lane = (rv_id << 16) | (lane as u32);
    TapEvent::new(ts, ids::LANE_ACQUIRE, 0, sid, rv_lane)
}

/// Session/lane association count moved 1->0.
#[inline(always)]
pub(crate) const fn lane_release(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let rv_lane = (rv_id << 16) | (lane as u32);
    TapEvent::new(ts, ids::LANE_RELEASE, 0, sid, rv_lane)
}

// ────────────── Route decision (0x0220-0x022F) ──────────────

/// Route arm selection resolved.
#[inline(always)]
pub(crate) const fn route_arm_selection_with_causal(
    ts: u32,
    causal: u16,
    sid: u32,
    arg1: u32,
) -> TapEvent {
    TapEvent::new(ts, ids::ROUTE_ARM_SELECTION, causal, sid, arg1)
}

// ────────────── Misuse detection (0x02FF) ──────────────

// ────────────── Resolver audit (0x0400-0x041F) ──────────────

// ────────────── Raw builder for fixed event encoders ──────────────

/// Raw TapEvent builder for callers that already own the event identifier.
#[inline(always)]
pub(crate) const fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent::new(ts, id, 0, 0, 0)
}
