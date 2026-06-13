//! Internal TapEvent encoders.
//!
//! Production callers use the event owner that matches the runtime operation;
//! Repository fixtures construct raw `TapEvent` values directly.

use super::core::TapEvent;
use super::ids;
// ────────────── Cancel / Endpoint (0x0200-0x020F) ──────────────

// ────────────── Lane lifecycle (0x0210-0x021F) ──────────────

#[inline(always)]
const fn pack_session_lane(sid: u32, lane: u16) -> u32 {
    (sid << 16) | (lane as u32)
}

/// Lane acquired via LaneLease.
#[inline(always)]
pub(crate) const fn lane_acquire(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let sid_lane = pack_session_lane(sid, lane);
    TapEvent {
        ts,
        id: ids::LANE_ACQUIRE,
        causal_key: 0,
        arg0: rv_id,
        arg1: sid_lane,
        arg2: 0,
    }
}

/// Lane released via LaneLease::Drop.
#[inline(always)]
pub(crate) const fn lane_release(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let sid_lane = pack_session_lane(sid, lane);
    TapEvent {
        ts,
        id: ids::LANE_RELEASE,
        causal_key: 0,
        arg0: rv_id,
        arg1: sid_lane,
        arg2: 0,
    }
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
    TapEvent {
        ts,
        id: ids::ROUTE_ARM_SELECTION,
        causal_key: causal,
        arg0: sid,
        arg1,
        arg2: 0,
    }
}

// ────────────── Misuse detection (0x02FF) ──────────────

// ────────────── Resolver VM (0x0400-0x041F) ──────────────

// ────────────── Raw builder for fixed event encoders ──────────────

/// Raw TapEvent builder for callers that already own the event identifier.
#[inline(always)]
pub(crate) const fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent {
        ts,
        id,
        causal_key: 0,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    }
}
