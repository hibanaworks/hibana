//! Type-safe TapEvent builders.
//!
//! Each event type has a dedicated struct with a `new()` constructor that
//! encodes arguments correctly. This eliminates argument ordering mistakes
//! and centralizes encoding logic.

use super::{core::TapEvent, ids};

// ────────────── Cancel / Endpoint (0x0200-0x020F) ──────────────

/// AMPST cancellation initiated.
pub struct CancelBegin;
impl CancelBegin {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, lane: u32) -> TapEvent {
        TapEvent { ts, id: ids::CANCEL_BEGIN, causal_key: 0, arg0: sid, arg1: lane, arg2: 0 }
    }
}

/// AMPST cancellation acknowledged.
pub struct CancelAck;
impl CancelAck {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, lane: u32) -> TapEvent {
        TapEvent { ts, id: ids::CANCEL_ACK, causal_key: 0, arg0: sid, arg1: lane, arg2: 0 }
    }
}

/// Endpoint send operation.
pub struct EndpointSend;
impl EndpointSend {
    /// Pack role/lane/label/flags into arg0.
    #[inline(always)]
    pub const fn pack(role: u8, lane: u8, label: u8, flags: u8) -> u32 {
        ((role as u32) << 24) | ((lane as u32) << 16) | ((label as u32) << 8) | (flags as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::ENDPOINT_SEND, causal_key: 0, arg0: sid, arg1: packed, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_scope(ts: u32, sid: u32, packed: u32, scope_pack: u32) -> TapEvent {
        TapEvent { ts, id: ids::ENDPOINT_SEND, causal_key: 0, arg0: sid, arg1: packed, arg2: scope_pack }
    }
}

/// Endpoint receive operation.
pub struct EndpointRecv;
impl EndpointRecv {
    #[inline(always)]
    pub const fn pack(role: u8, lane: u8, label: u8, flags: u8) -> u32 {
        ((role as u32) << 24) | ((lane as u32) << 16) | ((label as u32) << 8) | (flags as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::ENDPOINT_RECV, causal_key: 0, arg0: sid, arg1: packed, arg2: 0 }
    }
}

/// Endpoint control-plane event.
pub struct EndpointControl;
impl EndpointControl {
    #[inline(always)]
    pub const fn pack(role: u8, lane: u8, label: u8, flags: u8) -> u32 {
        ((role as u32) << 24) | ((lane as u32) << 16) | ((label as u32) << 8) | (flags as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::ENDPOINT_CONTROL, causal_key: 0, arg0: sid, arg1: packed, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal(ts: u32, causal: u16, sid: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::ENDPOINT_CONTROL, causal_key: causal, arg0: sid, arg1: packed, arg2: 0 }
    }
}

/// Relay forward event.
pub struct RelayForward;
impl RelayForward {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::RELAY_FORWARD, causal_key: 0, arg0: sid, arg1: packed, arg2: 0 }
    }
}

/// Forward control-plane event.
pub struct ForwardControl;
impl ForwardControl {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, payload: u32) -> TapEvent {
        TapEvent { ts, id: ids::FORWARD_CONTROL, causal_key: 0, arg0: sid, arg1: payload, arg2: 0 }
    }
}

/// Splice handshake initiated.
pub struct SpliceBegin;
impl SpliceBegin {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, generation: u32) -> TapEvent {
        TapEvent { ts, id: ids::SPLICE_BEGIN, causal_key: 0, arg0: sid, arg1: generation, arg2: 0 }
    }
}

/// Splice handshake committed.
pub struct SpliceCommit;
impl SpliceCommit {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, generation: u32) -> TapEvent {
        TapEvent { ts, id: ids::SPLICE_COMMIT, causal_key: 0, arg0: sid, arg1: generation, arg2: 0 }
    }
}

// ────────────── Lane lifecycle (0x0210-0x021F) ──────────────

/// Lane acquired via LaneLease.
pub struct LaneAcquire;
impl LaneAcquire {
    #[inline(always)]
    pub const fn pack_session_lane(sid: u32, lane: u16) -> u32 {
        (sid << 16) | (lane as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
        let sid_lane = Self::pack_session_lane(sid, lane);
        TapEvent { ts, id: ids::LANE_ACQUIRE, causal_key: 0, arg0: rv_id, arg1: sid_lane, arg2: 0 }
    }

    #[inline(always)]
    pub const fn from_packed(ts: u32, rv_id: u32, sid_lane: u32) -> TapEvent {
        TapEvent { ts, id: ids::LANE_ACQUIRE, causal_key: 0, arg0: rv_id, arg1: sid_lane, arg2: 0 }
    }
}

/// Lane released via LaneLease::Drop.
pub struct LaneRelease;
impl LaneRelease {
    #[inline(always)]
    pub const fn pack_session_lane(sid: u32, lane: u16) -> u32 {
        (sid << 16) | (lane as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, rv_id: u32, sid: u32, lane: u16) -> TapEvent {
        let sid_lane = Self::pack_session_lane(sid, lane);
        TapEvent { ts, id: ids::LANE_RELEASE, causal_key: 0, arg0: rv_id, arg1: sid_lane, arg2: 0 }
    }

    #[inline(always)]
    pub const fn from_packed(ts: u32, rv_id: u32, sid_lane: u32) -> TapEvent {
        TapEvent { ts, id: ids::LANE_RELEASE, causal_key: 0, arg0: rv_id, arg1: sid_lane, arg2: 0 }
    }
}

// ────────────── Route / Loop control (0x0220-0x022F) ──────────────

/// Loop decision recorded.
pub struct LoopDecision;
impl LoopDecision {
    /// Pack lane/idx/disposition into arg1.
    #[inline(always)]
    pub const fn pack(lane: u8, idx: u8, continues: bool) -> u32 {
        ((lane as u32) << 16) | ((idx as u32) << 8) | (continues as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, lane: u8, idx: u8, continues: bool) -> TapEvent {
        let arg1 = Self::pack(lane, idx, continues);
        TapEvent { ts, id: ids::LOOP_DECISION, causal_key: 0, arg0: sid, arg1, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal(ts: u32, causal: u16, sid: u32, arg1: u32) -> TapEvent {
        TapEvent { ts, id: ids::LOOP_DECISION, causal_key: causal, arg0: sid, arg1, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal_and_scope(ts: u32, causal: u16, sid: u32, arg1: u32, scope_pack: u32) -> TapEvent {
        TapEvent { ts, id: ids::LOOP_DECISION, causal_key: causal, arg0: sid, arg1, arg2: scope_pack }
    }
}

/// Route arm selection resolved.
pub struct RouteDecision;
impl RouteDecision {
    /// Pack scope_id and arm into arg1.
    #[inline(always)]
    pub const fn pack(scope_id: u16, arm: u16) -> u32 {
        ((scope_id as u32) << 16) | (arm as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, scope_id: u16, arm: u16) -> TapEvent {
        let arg1 = Self::pack(scope_id, arm);
        TapEvent { ts, id: ids::ROUTE_DECISION, causal_key: 0, arg0: sid, arg1, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal(ts: u32, causal: u16, sid: u32, arg1: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROUTE_DECISION, causal_key: causal, arg0: sid, arg1, arg2: 0 }
    }
}

/// Route scope entered.
pub struct RouteScopeEnter;
impl RouteScopeEnter {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, scope_id: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROUTE_SCOPE_ENTER, causal_key: 0, arg0: sid, arg1: scope_id, arg2: 0 }
    }
}

/// Route scope exited.
pub struct RouteScopeExit;
impl RouteScopeExit {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, scope_id: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROUTE_SCOPE_EXIT, causal_key: 0, arg0: sid, arg1: scope_id, arg2: 0 }
    }
}

/// Loop scope entered.
pub struct LoopScopeEnter;
impl LoopScopeEnter {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, scope_id: u32) -> TapEvent {
        TapEvent { ts, id: ids::LOOP_SCOPE_ENTER, causal_key: 0, arg0: sid, arg1: scope_id, arg2: 0 }
    }
}

/// Loop scope exited.
pub struct LoopScopeExit;
impl LoopScopeExit {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, scope_id: u32) -> TapEvent {
        TapEvent { ts, id: ids::LOOP_SCOPE_EXIT, causal_key: 0, arg0: sid, arg1: scope_id, arg2: 0 }
    }
}

/// Local action handler failure.
pub struct LocalActionFail;
impl LocalActionFail {
    #[inline(always)]
    pub const fn pack(eff_index: u16, reason: u16) -> u32 {
        ((eff_index as u32) << 16) | (reason as u32)
    }

    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, eff_index: u16, reason: u16) -> TapEvent {
        let arg1 = Self::pack(eff_index, reason);
        TapEvent { ts, id: ids::LOCAL_ACTION_FAIL, causal_key: 0, arg0: sid, arg1, arg2: 0 }
    }
}

// ────────────── Capability lifecycle (0x0240-0x024F) ──────────────

/// Session effect initialisation.
pub struct EffectInit;
impl EffectInit {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, effect_count: u32) -> TapEvent {
        TapEvent { ts, id: ids::EFFECT_INIT, causal_key: 0, arg0: sid, arg1: effect_count, arg2: 0 }
    }
}

// ────────────── Checkpoint / Rollback (0x0130-0x013F) ──────────────

/// Checkpoint request.
pub struct CheckpointReq;
impl CheckpointReq {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, generation: u32) -> TapEvent {
        TapEvent { ts, id: ids::CHECKPOINT_REQ, causal_key: 0, arg0: sid, arg1: generation, arg2: 0 }
    }
}

/// Rollback requested.
pub struct RollbackReq;
impl RollbackReq {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, target_gen: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROLLBACK_REQ, causal_key: 0, arg0: sid, arg1: target_gen, arg2: 0 }
    }
}

/// Rollback completed.
pub struct RollbackOk;
impl RollbackOk {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, restored_gen: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROLLBACK_OK, causal_key: 0, arg0: sid, arg1: restored_gen, arg2: 0 }
    }
}

// ────────────── Transport (0x0210-0x021F) ──────────────

/// UDP transmit event.
pub struct UdpTx;
impl UdpTx {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, payload: u32) -> TapEvent {
        TapEvent { ts, id: ids::UDP_TX, causal_key: 0, arg0: sid, arg1: payload, arg2: 0 }
    }
}

/// UDP receive event.
pub struct UdpRx;
impl UdpRx {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, payload: u32) -> TapEvent {
        TapEvent { ts, id: ids::UDP_RX, causal_key: 0, arg0: sid, arg1: payload, arg2: 0 }
    }
}

/// Transport-level telemetry event.
pub struct TransportEvent;
impl TransportEvent {
    #[inline(always)]
    pub const fn new(ts: u32, pn_low: u32, packed: u32) -> TapEvent {
        TapEvent { ts, id: ids::TRANSPORT_EVENT, causal_key: 0, arg0: pn_low, arg1: packed, arg2: 0 }
    }
}

/// Transport-level congestion metrics.
pub struct TransportMetrics;
impl TransportMetrics {
    #[inline(always)]
    pub const fn new(ts: u32, arg0: u32, arg1: u32) -> TapEvent {
        TapEvent { ts, id: ids::TRANSPORT_METRICS, causal_key: 0, arg0, arg1, arg2: 0 }
    }
}

/// Transport-level congestion metrics extension.
pub struct TransportMetricsExt;
impl TransportMetricsExt {
    #[inline(always)]
    pub const fn new(ts: u32, ext0: u32, ext1: u32) -> TapEvent {
        TapEvent { ts, id: ids::TRANSPORT_METRICS_EXT, causal_key: 0, arg0: ext0, arg1: ext1, arg2: 0 }
    }
}

// ────────────── Misuse detection (0x02FF) ──────────────

/// RecvGuard dropped without completion.
pub struct MisuseRecvguardDrop;
impl MisuseRecvguardDrop {
    #[inline(always)]
    pub const fn new(ts: u32, sid: u32, lane_role: u32) -> TapEvent {
        TapEvent { ts, id: ids::MISUSE_RECVGUARD_DROP, causal_key: 0, arg0: sid, arg1: lane_role, arg2: 0 }
    }
}

// ────────────── Delegation (0x0230-0x023F) ──────────────

/// Delegation begins.
pub struct DelegBegin;
impl DelegBegin {
    #[inline(always)]
    pub const fn new(ts: u32, service_hi: u32, service_lo_flags: u32) -> TapEvent {
        TapEvent { ts, id: ids::DELEG_BEGIN, causal_key: 0, arg0: service_hi, arg1: service_lo_flags, arg2: 0 }
    }
}

/// Routing policy picked a target.
pub struct RoutePick;
impl RoutePick {
    #[inline(always)]
    pub const fn new(ts: u32, policy_id: u32, shard: u32) -> TapEvent {
        TapEvent { ts, id: ids::ROUTE_PICK, causal_key: 0, arg0: policy_id, arg1: shard, arg2: 0 }
    }
}

/// Delegation splice completed.
pub struct DelegSplice;
impl DelegSplice {
    #[inline(always)]
    pub const fn pack(from_lane: u8, to_lane: u8, generation: u16) -> u32 {
        (from_lane as u32) | ((to_lane as u32) << 8) | ((generation as u32) << 16)
    }

    /// Create with pre-packed arg0 and sid.
    #[inline(always)]
    pub const fn new(ts: u32, arg0: u32, sid: u32) -> TapEvent {
        TapEvent { ts, id: ids::DELEG_SPLICE, causal_key: 0, arg0, arg1: sid, arg2: 0 }
    }

    /// Create from individual lane/gen fields.
    #[inline(always)]
    pub const fn from_parts(ts: u32, from_lane: u8, to_lane: u8, generation: u16, sid: u32) -> TapEvent {
        let arg0 = Self::pack(from_lane, to_lane, generation);
        TapEvent { ts, id: ids::DELEG_SPLICE, causal_key: 0, arg0, arg1: sid, arg2: 0 }
    }
}

/// Delegation aborted.
pub struct DelegAbort;
impl DelegAbort {
    #[inline(always)]
    pub const fn new(ts: u32, reason: u32, context: u32) -> TapEvent {
        TapEvent { ts, id: ids::DELEG_ABORT, causal_key: 0, arg0: reason, arg1: context, arg2: 0 }
    }
}

/// SLO breach detected.
pub struct SloBreach;
impl SloBreach {
    #[inline(always)]
    pub const fn new(ts: u32, latency_us: u32, queue_retry: u32) -> TapEvent {
        TapEvent { ts, id: ids::SLO_BREACH, causal_key: 0, arg0: latency_us, arg1: queue_retry, arg2: 0 }
    }
}

// ────────────── Policy VM (0x0400-0x040F) ──────────────

/// Policy VM abort.
pub struct PolicyAbort;
impl PolicyAbort {
    #[inline(always)]
    pub const fn new(ts: u32, reason: u16, sid: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_ABORT, causal_key: 0, arg0: reason as u32, arg1: sid, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal(ts: u32, causal: u16, reason: u32, sid: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_ABORT, causal_key: causal, arg0: reason, arg1: sid, arg2: 0 }
    }
}

/// Policy VM annotation.
pub struct PolicyAnnot;
impl PolicyAnnot {
    #[inline(always)]
    pub const fn new(ts: u32, key: u16, value: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_ANNOT, causal_key: 0, arg0: key as u32, arg1: value, arg2: 0 }
    }
}

/// Policy VM trap.
pub struct PolicyTrap;
impl PolicyTrap {
    #[inline(always)]
    pub const fn new(ts: u32, kind: u32, sid: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_TRAP, causal_key: 0, arg0: kind, arg1: sid, arg2: 0 }
    }
}

/// Policy VM effect dispatched.
pub struct PolicyEffect;
impl PolicyEffect {
    #[inline(always)]
    pub const fn new(ts: u32, effect: u16, operand: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_EFFECT, causal_key: 0, arg0: effect as u32, arg1: operand, arg2: 0 }
    }
}

/// Policy-requested effect completed.
pub struct PolicyRaOk;
impl PolicyRaOk {
    #[inline(always)]
    pub const fn new(ts: u32, effect: u16, sid: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_RA_OK, causal_key: 0, arg0: effect as u32, arg1: sid, arg2: 0 }
    }
}

/// Policy VM slot commit.
pub struct PolicyCommit;
impl PolicyCommit {
    #[inline(always)]
    pub const fn new(ts: u32, slot: u32, version: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_COMMIT, causal_key: 0, arg0: slot, arg1: version, arg2: 0 }
    }

    #[inline(always)]
    pub const fn with_causal(ts: u32, causal: u16, slot: u32, version: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_COMMIT, causal_key: causal, arg0: slot, arg1: version, arg2: 0 }
    }
}

/// Policy VM slot rollback.
pub struct PolicyRollback;
impl PolicyRollback {
    #[inline(always)]
    pub const fn new(ts: u32, slot: u32, version: u32) -> TapEvent {
        TapEvent { ts, id: ids::POLICY_ROLLBACK, causal_key: 0, arg0: slot, arg1: version, arg2: 0 }
    }
}

// ────────────── Capability mint/claim/exhaust ──────────────

/// Capability minted.
pub struct CapMint;
impl CapMint {
    #[inline(always)]
    pub const fn id(tag: u8) -> u16 {
        ids::CAP_MINT_BASE + tag as u16
    }

    #[inline(always)]
    pub const fn new(ts: u32, tag: u8, sid: u32, cap_id: u32) -> TapEvent {
        TapEvent { ts, id: Self::id(tag), causal_key: 0, arg0: sid, arg1: cap_id, arg2: 0 }
    }
}

/// Capability claimed.
pub struct CapClaim;
impl CapClaim {
    #[inline(always)]
    pub const fn id(tag: u8) -> u16 {
        ids::CAP_CLAIM_BASE + tag as u16
    }

    #[inline(always)]
    pub const fn new(ts: u32, tag: u8, sid: u32, cap_id: u32) -> TapEvent {
        TapEvent { ts, id: Self::id(tag), causal_key: 0, arg0: sid, arg1: cap_id, arg2: 0 }
    }
}

/// Capability exhausted.
pub struct CapExhaust;
impl CapExhaust {
    #[inline(always)]
    pub const fn id(tag: u8) -> u16 {
        ids::CAP_EXHAUST_BASE + tag as u16
    }

    #[inline(always)]
    pub const fn new(ts: u32, tag: u8, sid: u32, cap_id: u32) -> TapEvent {
        TapEvent { ts, id: Self::id(tag), causal_key: 0, arg0: sid, arg1: cap_id, arg2: 0 }
    }
}

// ────────────── Raw builder (for testing / zero-init) ──────────────

/// Raw TapEvent builder (for special cases only).
pub struct RawEvent;
impl RawEvent {
    /// Zero-initialized event (for static defaults).
    #[inline(always)]
    pub const fn zero() -> TapEvent {
        TapEvent { ts: 0, id: 0, causal_key: 0, arg0: 0, arg1: 0, arg2: 0 }
    }

    /// Raw event with explicit id (for tests and edge cases).
    #[inline(always)]
    pub const fn new(ts: u32, id: u16, arg0: u32, arg1: u32) -> TapEvent {
        TapEvent { ts, id, causal_key: 0, arg0, arg1, arg2: 0 }
    }

    /// Raw event with causal key.
    #[inline(always)]
    pub const fn with_causal(ts: u32, id: u16, causal: u16, arg0: u32, arg1: u32) -> TapEvent {
        TapEvent { ts, id, causal_key: causal, arg0, arg1, arg2: 0 }
    }
}
