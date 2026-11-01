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
pub(crate) const fn lane_acquire(rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let rv_lane = (rv_id << 16) | (lane as u32);
    TapEvent::new(0, ids::LANE_ACQUIRE, 0, sid, rv_lane)
}

/// Session/lane association count moved 1->0.
#[inline(always)]
pub(crate) const fn lane_release(rv_id: u32, sid: u32, lane: u16) -> TapEvent {
    let rv_lane = (rv_id << 16) | (lane as u32);
    TapEvent::new(0, ids::LANE_RELEASE, 0, sid, rv_lane)
}

// ────────────── Route decision (0x0220-0x022F) ──────────────

/// Route arm selection resolved.
#[inline(always)]
pub(crate) const fn route_arm_selection_with_causal(
    causal: u16,
    sid: u32,
    scope_id: ScopeId,
    arm: u8,
) -> TapEvent {
    let arg1 = ((route_site(scope_id) as u32) << 16) | (arm as u32);
    TapEvent::new(0, ids::ROUTE_ARM_SELECTION, causal, sid, arg1)
}

#[inline(always)]
pub(crate) const fn scope_reentry(
    actor: u8,
    view: u8,
    sid: u32,
    scope: ScopeId,
    detail: u16,
) -> TapEvent {
    TapEvent::new(
        0,
        ids::SCOPE_REENTRY,
        TapEvent::make_causal_key(actor, view),
        sid,
        (scope.raw() as u32) << 16 | detail as u32,
    )
}

#[inline(always)]
pub(crate) const fn send_poll(
    actor: u8,
    view: u8,
    sid: u32,
    reset: ScopeId,
    source: u16,
) -> TapEvent {
    TapEvent::new(
        0,
        ids::SEND_POLL,
        TapEvent::make_causal_key(actor, view),
        sid,
        (reset.raw() as u32) << 16 | source as u32,
    )
}

// ────────────── Misuse detection (0x02FF) ──────────────

// ────────────── Resolver audit (0x0400-0x041F) ──────────────

#[inline(always)]
pub(crate) const fn resolver_audit(
    actor: u8,
    sid: u32,
    scope_id: ScopeId,
    resolver_id: u16,
    result: u8,
) -> TapEvent {
    let causal = TapEvent::make_causal_key(actor, result);
    let arg1 = ((route_site(scope_id) as u32) << 16) | (resolver_id as u32);
    TapEvent::new(0, ids::RESOLVER_AUDIT, causal, sid, arg1)
}

#[inline(always)]
const fn route_site(scope_id: ScopeId) -> u16 {
    if !matches!(scope_id.kind(), Some(ScopeKind::Route)) {
        crate::invariant();
    }
    scope_id.local_ordinal()
}

// ────────────── Raw builder for fixed event encoders ──────────────

/// Raw TapEvent builder for callers that already own the event identifier.
#[inline(always)]
pub(crate) const fn raw_event(id: u16) -> TapEvent {
    TapEvent::new(0, id, 0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_poll_keeps_the_actor_view_prepared_scope_and_complete_source_identity() {
        for actor in 0..=u8::MAX {
            let view = actor.wrapping_add(127);
            for reset in [
                ScopeId::none(),
                ScopeId::roll_scope(0),
                ScopeId::roll_scope(8191),
            ] {
                for source in [0, 1, 256, u16::MAX] {
                    let record = send_poll(actor, view, u32::MAX, reset, source);
                    assert_eq!(record.id(), ids::SEND_POLL);
                    assert_eq!(record.causal_key() >> 8, u16::from(actor));
                    assert_eq!(record.causal_key() as u8, view);
                    assert_eq!(record.arg0(), u32::MAX);
                    assert_eq!(record.arg1() >> 16, u32::from(reset.raw()));
                    assert_eq!(record.arg1() as u16, source);
                    assert_eq!(TapEvent::from(<[u8; 16]>::from(record)), record);
                    assert_eq!(record.evidence().kind(), ids::SEND_POLL);
                    assert_eq!(record.evidence().input()[3], u32::from(record.causal_key()));
                }
            }
        }
    }

    #[test]
    fn scope_reentry_keeps_scope_kind_actor_view_and_full_detail() {
        for (actor, view) in [(0, 255), (255, 0), (255, 255), (1, 9)] {
            for scope in [ScopeId::route(8191), ScopeId::roll_scope(8191)] {
                for detail in [0, 1, 256, u16::MAX] {
                    let event = scope_reentry(actor, view, u32::MAX, scope, detail);
                    assert_eq!(event.id(), ids::SCOPE_REENTRY);
                    assert_eq!(event.causal_key() >> 8, u16::from(actor));
                    assert_eq!(event.causal_key() as u8, view);
                    assert_eq!(event.arg0(), u32::MAX);
                    assert_eq!(event.arg1() >> 16, u32::from(scope.raw()));
                    assert_eq!(event.arg1() as u16, detail);
                }
            }
        }
    }

    #[test]
    fn route_site_evidence_uses_scope_local_ordinal() {
        let first = route_arm_selection_with_causal(2, 3, ScopeId::route(0), 1);
        let second = route_arm_selection_with_causal(2, 3, ScopeId::route(1), 1);
        assert_eq!(first.arg1() >> 16, 0);
        assert_eq!(second.arg1() >> 16, 1);
        assert_eq!(first.arg1() & 0xffff, 1);
        assert_eq!(second.arg1() & 0xffff, 1);

        let first_audit = resolver_audit(7, 3, ScopeId::route(0), 77, 0);
        let second_audit = resolver_audit(7, 3, ScopeId::route(1), 77, 0);
        assert_eq!(first_audit.arg1() >> 16, 0);
        assert_eq!(second_audit.arg1() >> 16, 1);
        assert_eq!(first_audit.arg1() & 0xffff, 77);
        assert_eq!(second_audit.arg1() & 0xffff, 77);
        assert_eq!(first_audit.causal_key(), second_audit.causal_key());
    }

    #[test]
    fn every_control_actor_preserves_the_site_resolver_and_result_fields() {
        for actor in 0..=u8::MAX {
            for result in [0, 1, u8::MAX] {
                for site in [0, ScopeId::LOCAL_CAPACITY - 1] {
                    for resolver in [0, u16::MAX] {
                        let event =
                            resolver_audit(actor, u32::MAX, ScopeId::route(site), resolver, result);
                        assert_eq!(event.causal_key() >> 8, u16::from(actor));
                        assert_eq!(event.causal_key() as u8, result);
                        assert_eq!(event.arg0(), u32::MAX);
                        assert_eq!(event.arg1() >> 16, site as u32);
                        assert_eq!(event.arg1() as u16, resolver);
                        assert_eq!(TapEvent::from(<[u8; 16]>::from(event)), event);
                    }
                }
            }
        }
    }

    #[test]
    #[should_panic]
    fn route_site_rejects_non_route_scope() {
        let _ = route_arm_selection_with_causal(2, 3, ScopeId::roll_scope(0), 1);
    }

    #[test]
    #[should_panic]
    fn route_site_rejects_out_of_domain_scope() {
        let _ = route_arm_selection_with_causal(2, 3, ScopeId::route(ScopeId::LOCAL_CAPACITY), 1);
    }
}
