//! TapEvent encoders owned by runtime operations.
//!
//! Production callers use the event owner that matches the runtime operation.

use super::core::TapEvent;
use super::ids;
use crate::global::const_dsl::{ScopeId, ScopeKind};

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
    scope_id: ScopeId,
    arm: u8,
) -> TapEvent {
    let arg1 = ((route_site(scope_id) as u32) << 16) | (arm as u32);
    TapEvent::new(ts, ids::ROUTE_ARM_SELECTION, causal, sid, arg1)
}

// ────────────── Misuse detection (0x02FF) ──────────────

// ────────────── Resolver audit (0x0400-0x041F) ──────────────

#[inline(always)]
pub(crate) const fn resolver_audit(
    ts: u32,
    lane: u8,
    sid: u32,
    scope_id: ScopeId,
    resolver_id: u16,
    result: u8,
) -> TapEvent {
    let causal = TapEvent::make_causal_key(lane, result);
    let arg1 = ((route_site(scope_id) as u32) << 16) | (resolver_id as u32);
    TapEvent::new(ts, ids::RESOLVER_AUDIT, causal, sid, arg1)
}

#[inline(always)]
const fn route_site(scope_id: ScopeId) -> u16 {
    if !matches!(scope_id.kind(), Some(ScopeKind::Route))
        || scope_id.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
    {
        crate::invariant();
    }
    scope_id.local_ordinal()
}

// ────────────── Raw builder for fixed event encoders ──────────────

/// Raw TapEvent builder for callers that already own the event identifier.
#[inline(always)]
pub(crate) const fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent::new(ts, id, 0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_site_evidence_uses_scope_local_ordinal() {
        let first = route_arm_selection_with_causal(1, 2, 3, ScopeId::route(0), 1);
        let second = route_arm_selection_with_causal(1, 2, 3, ScopeId::route(1), 1);
        assert_eq!(first.arg1() >> 16, 0);
        assert_eq!(second.arg1() >> 16, 1);
        assert_eq!(first.arg1() & 0xffff, 1);
        assert_eq!(second.arg1() & 0xffff, 1);

        let first_audit = resolver_audit(1, 7, 3, ScopeId::route(0), 77, 0);
        let second_audit = resolver_audit(1, 7, 3, ScopeId::route(1), 77, 0);
        assert_eq!(first_audit.arg1() >> 16, 0);
        assert_eq!(second_audit.arg1() >> 16, 1);
        assert_eq!(first_audit.arg1() & 0xffff, 77);
        assert_eq!(second_audit.arg1() & 0xffff, 77);
        assert_eq!(first_audit.causal_key(), second_audit.causal_key());
    }

    #[test]
    #[should_panic]
    fn route_site_rejects_non_route_scope() {
        let _ = route_arm_selection_with_causal(1, 2, 3, ScopeId::roll_scope(0), 1);
    }

    #[test]
    #[should_panic]
    fn route_site_rejects_out_of_domain_scope() {
        let _ = route_arm_selection_with_causal(
            1,
            2,
            3,
            ScopeId::route(crate::eff::meta::MAX_EFF_NODES as u16),
            1,
        );
    }
}
