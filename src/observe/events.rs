//! Internal TapEvent encoders.
//!
//! Production callers use the event owner that matches the runtime operation;
//! crate tests build synthetic events in their own fixtures.

use super::{core::TapEvent, ids};

// ────────────── Cancel / Endpoint (0x0200-0x020F) ──────────────

// ────────────── Lane lifecycle (0x0210-0x021F) ──────────────

/// Lane acquired via LaneLease.
pub(crate) struct LaneAcquire;
impl LaneAcquire {
    #[inline(always)]
    pub(crate) const fn pack_session_lane(sid: u32, lane: u16) -> u32 {
        (sid << 16) | (lane as u32)
    }

    #[inline(always)]
    pub(crate) const fn new(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
        let sid_lane = Self::pack_session_lane(sid, lane);
        TapEvent {
            ts,
            id: ids::LANE_ACQUIRE,
            causal_key: 0,
            arg0: rv_id,
            arg1: sid_lane,
            arg2: 0,
        }
    }
}

/// Lane released via LaneLease::Drop.
pub(crate) struct LaneRelease;
impl LaneRelease {
    #[inline(always)]
    pub(crate) const fn pack_session_lane(sid: u32, lane: u16) -> u32 {
        (sid << 16) | (lane as u32)
    }

    #[inline(always)]
    pub(crate) const fn new(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
        let sid_lane = Self::pack_session_lane(sid, lane);
        TapEvent {
            ts,
            id: ids::LANE_RELEASE,
            causal_key: 0,
            arg0: rv_id,
            arg1: sid_lane,
            arg2: 0,
        }
    }
}

// ────────────── Route / Loop control (0x0220-0x022F) ──────────────

/// Loop decision recorded.
pub(crate) struct LoopDecision;
impl LoopDecision {
    #[inline(always)]
    pub(crate) const fn with_causal_and_scope(
        ts: u32,
        causal: u16,
        sid: u32,
        arg1: u32,
        scope_pack: u32,
    ) -> TapEvent {
        TapEvent {
            ts,
            id: ids::LOOP_DECISION,
            causal_key: causal,
            arg0: sid,
            arg1,
            arg2: scope_pack,
        }
    }
}

/// Route arm selection resolved.
pub(crate) struct RouteDecision;
impl RouteDecision {
    #[inline(always)]
    pub(crate) const fn with_causal(ts: u32, causal: u16, sid: u32, arg1: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::ROUTE_DECISION,
            causal_key: causal,
            arg0: sid,
            arg1,
            arg2: 0,
        }
    }
}

// ────────────── Capability lifecycle (0x0240-0x024F) ──────────────

/// Session effect initialisation.
pub(crate) struct EffectInit;
impl EffectInit {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, sid: u32, effect_count: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::EFFECT_INIT,
            causal_key: 0,
            arg0: sid,
            arg1: effect_count,
            arg2: 0,
        }
    }
}

// ────────────── State Snapshot / State Restore (0x0130-0x013F) ──────────────

/// State restore completed.
pub(crate) struct StateRestoreOk;
impl StateRestoreOk {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, sid: u32, restored_gen: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::STATE_RESTORE_OK,
            causal_key: 0,
            arg0: sid,
            arg1: restored_gen,
            arg2: 0,
        }
    }
}

// ────────────── Transport (0x0210-0x021F) ──────────────

/// Transport-level telemetry event.
pub(crate) struct TransportEvent;
impl TransportEvent {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, pn_low: u32, packed: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::TRANSPORT_EVENT,
            causal_key: 0,
            arg0: pn_low,
            arg1: packed,
            arg2: 0,
        }
    }
}

/// Transport-level congestion metrics.
pub(crate) struct TransportMetrics;
impl TransportMetrics {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, arg0: u32, arg1: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::TRANSPORT_METRICS,
            causal_key: 0,
            arg0,
            arg1,
            arg2: 0,
        }
    }
}

/// Transport-level congestion metrics extension.
pub(crate) struct TransportMetricsExt;
impl TransportMetricsExt {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, ext0: u32, ext1: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::TRANSPORT_METRICS_EXT,
            causal_key: 0,
            arg0: ext0,
            arg1: ext1,
            arg2: 0,
        }
    }
}

// ────────────── Misuse detection (0x02FF) ──────────────

// ────────────── Delegation (0x0230-0x023F) ──────────────

/// Delegation begins.
pub(crate) struct DelegBegin;
impl DelegBegin {
    #[inline(always)]
    pub(crate) const fn new(ts: u32, service_hi: u32, service_lo_flags: u32) -> TapEvent {
        TapEvent {
            ts,
            id: ids::DELEG_BEGIN,
            causal_key: 0,
            arg0: service_hi,
            arg1: service_lo_flags,
            arg2: 0,
        }
    }
}

// ────────────── Policy VM (0x0400-0x041F) ──────────────

// ────────────── Raw builder for fixed event encoders ──────────────

/// Raw TapEvent builder for callers that already own the event identifier.
pub struct RawEvent;
impl RawEvent {
    /// Raw event with an explicit id.
    #[inline(always)]
    pub const fn new(ts: u32, id: u16) -> TapEvent {
        TapEvent {
            ts,
            id,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }
}
