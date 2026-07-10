use super::*;
use crate::{
    global::const_dsl::{ScopeId, ScopeKind},
    runtime_core::resources::RuntimeResources,
    session::types::RendezvousId,
    transport::{Outgoing, PortOpen, ReceivedFrame, TransportError},
};
use core::task::{Context, Poll};

#[derive(Clone, Copy)]
struct FailingTransport;

struct FailingTx;
struct FailingRx;

impl Transport for FailingTransport {
    type Tx<'a> = FailingTx;
    type Rx<'a> = FailingRx;

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (FailingTx, FailingRx)
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Err(TransportError::Failed))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Ready(Err(TransportError::Failed))
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Err(TransportError::Failed)
    }
}

fn init_test_rendezvous(slab: &mut [u8]) -> &mut Rendezvous<'_, '_, FailingTransport> {
    let rv_ptr = unsafe {
        // SAFETY: the test owns the whole slab for the duration of the rendezvous.
        Rendezvous::init_in_slab_auto(
            RendezvousId::new(1),
            RuntimeResources::new(slab),
            FailingTransport,
        )
        .expect("rendezvous")
    };
    // SAFETY: init_in_slab_auto returned a unique resident pointer backed by slab.
    unsafe { &mut *rv_ptr }
}

#[test]
#[should_panic]
fn nonempty_sidecar_before_slab_fails_closed() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let (slab_ptr, _) = rv.slab_ptr_and_len();
    let malformed = Sidecar::from_raw_parts(slab_ptr.wrapping_sub(1), 1);

    core::hint::black_box(rv.sidecar_range(malformed));
}

#[test]
fn assoc_replacement_retires_old_root_without_history_growth() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("initial assoc storage");
    let lane = crate::session::types::Lane::new(0);
    let sid = crate::session::types::SessionId::new(1);
    rv.activate_lane_attachment(sid, lane)
        .expect("initial live association");
    let mut slots = 2usize;
    while slots <= 24 {
        let before = rv.assoc_storage.get();
        rv.ensure_core_lane_tables_for_assoc_entries(slots, slots)
            .expect("grown assoc storage");
        let current = rv.assoc_storage.get();
        assert_eq!(current.ptr(), before.ptr());
        assert!(current.bytes() > before.bytes());
        let replacement_bound = current
            .bytes()
            .checked_add(AssocTable::storage_align() - 1)
            .expect("replacement bound");
        assert!(
            rv.image_frontier.get() as usize <= replacement_bound,
            "assoc frontier must depend on live/replacement bytes, not growth count"
        );
        assert!(rv.has_lane_attachment(sid, lane));
        slots += 1;
    }
    assert_eq!(rv.lane_end.get() - rv.lane_base.get(), 24);
}

#[test]
fn assoc_shrink_returns_storage_to_active_claim_count() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(3, 6)
        .expect("assoc storage");
    let lane_a = crate::session::types::Lane::new(2);
    let lane_b = crate::session::types::Lane::new(0);
    let sid_a = crate::session::types::SessionId::new(1);
    let sid_b = crate::session::types::SessionId::new(2);
    rv.activate_lane_attachment(sid_a, lane_a)
        .expect("session A claim");
    rv.activate_lane_attachment(sid_b, lane_b)
        .expect("session B claim");
    assert_eq!(rv.assoc.assoc_slots(), 6);

    assert_eq!(
        rv.release_lane(sid_a, lane_a),
        crate::rendezvous::core::LaneRelease::Released
    );
    rv.shrink_assoc_table_capacity(rv.active_lane_attachment_count());
    rv.shrink_lane_range(rv.assoc.active_lane_slots());

    assert_eq!(rv.assoc.assoc_slots(), 1);
    assert_eq!(rv.lane_slot_count(), 1);
    assert!(rv.has_lane_attachment(sid_b, lane_b));
    assert_eq!(rv.assoc_storage.get().bytes(), AssocTable::storage_bytes(1));
}

#[test]
fn route_replacement_retires_old_root_without_history_growth() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(1, 1)
        .expect("initial route storage");
    let mut slots = 2usize;
    while slots <= 24 {
        let before = rv.route_storage.get();
        rv.ensure_route_table_capacity(slots, 1)
            .expect("grown route storage");
        let current = rv.route_storage.get();
        assert_eq!(current.ptr(), before.ptr());
        assert!(current.bytes() > before.bytes());
        let replacement_bound = current
            .bytes()
            .checked_add(RouteTable::storage_align() - 1)
            .expect("replacement bound");
        assert!(
            rv.image_frontier.get() as usize <= replacement_bound,
            "route frontier must depend on live/replacement bytes, not growth count"
        );
        slots += 1;
    }
    assert_eq!(rv.routes.route_slots(), 24);
    assert_eq!(rv.routes.lane_slots(), 1);
}

#[test]
fn route_growth_preserves_session_isolation() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(2, 1).expect("route storage");
    let sid_a = crate::session::types::SessionId::new(1);
    let sid_b = crate::session::types::SessionId::new(2);
    let lane = crate::session::types::Lane::new(0);
    let scope = ScopeId::new(ScopeKind::Route, 0);
    assert!(
        rv.routes
            .poll_with_role_count(sid_a, lane, 2, 0, scope)
            .is_pending()
    );
    assert!(
        rv.routes
            .poll_with_role_count(sid_b, lane, 2, 0, scope)
            .is_pending()
    );

    rv.ensure_route_table_capacity(4, 1)
        .expect("grown route storage");
    rv.routes
        .record_with_role_count(sid_a, lane, 2, 1, scope, 0);
    rv.routes
        .record_with_role_count(sid_b, lane, 2, 1, scope, 1);
    assert_eq!(
        rv.routes.poll_with_role_count(sid_a, lane, 2, 0, scope),
        Poll::Ready(0)
    );
    assert_eq!(
        rv.routes.poll_with_role_count(sid_b, lane, 2, 0, scope),
        Poll::Ready(1)
    );
}

#[test]
fn route_shrink_compacts_live_frames_without_losing_session_keys() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(6, 1).expect("route storage");
    let lane = crate::session::types::Lane::new(0);
    let scope = ScopeId::new(ScopeKind::Route, 0);
    let mut raw_sid = 1u32;
    while raw_sid <= 6 {
        assert!(
            rv.routes
                .poll_with_role_count(
                    crate::session::types::SessionId::new(raw_sid),
                    lane,
                    2,
                    0,
                    scope,
                )
                .is_pending()
        );
        raw_sid += 1;
    }
    raw_sid = 1;
    while raw_sid <= 3 {
        let sid = crate::session::types::SessionId::new(raw_sid);
        rv.routes.record_with_role_count(sid, lane, 2, 1, scope, 0);
        assert_eq!(
            rv.routes.poll_with_role_count(sid, lane, 2, 0, scope),
            Poll::Ready(0)
        );
        raw_sid += 1;
    }

    rv.shrink_route_table_capacity(3, 1);
    assert_eq!(rv.routes.route_slots(), 3);
    assert_eq!(
        rv.route_storage.get().bytes(),
        RouteTable::storage_bytes(3, 1)
    );

    raw_sid = 4;
    while raw_sid <= 6 {
        let sid = crate::session::types::SessionId::new(raw_sid);
        let arm = (raw_sid & 1) as u8;
        rv.routes
            .record_with_role_count(sid, lane, 2, 1, scope, arm);
        assert_eq!(
            rv.routes.poll_with_role_count(sid, lane, 2, 0, scope),
            Poll::Ready(arm)
        );
        raw_sid += 1;
    }
}

#[test]
fn route_reset_and_shrink_preserve_other_sessions_and_free_slots() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_route_table_capacity(6, 1).expect("route storage");
    let lane = crate::session::types::Lane::new(0);
    let scope = ScopeId::new(ScopeKind::Route, 0);
    let removed_sid = crate::session::types::SessionId::new(1);
    let retained_sid = crate::session::types::SessionId::new(2);

    assert!(
        rv.routes
            .poll_with_role_count(removed_sid, lane, 2, 0, scope)
            .is_pending()
    );
    rv.routes
        .record_with_role_count(retained_sid, lane, 2, 1, scope, 1);

    rv.routes.reset_session_lane(removed_sid, lane);
    assert_eq!(
        rv.routes
            .peek_with_role_count(removed_sid, lane, 2, 0, scope),
        None
    );
    assert_eq!(
        rv.routes
            .peek_with_role_count(retained_sid, lane, 2, 0, scope),
        Some(1)
    );

    rv.shrink_route_table_capacity(3, 1);
    assert_eq!(rv.routes.route_slots(), 3);
    assert_eq!(
        rv.routes
            .peek_with_role_count(retained_sid, lane, 2, 0, scope),
        Some(1)
    );

    let first_reused_sid = crate::session::types::SessionId::new(3);
    let second_reused_sid = crate::session::types::SessionId::new(4);
    rv.routes
        .record_with_role_count(first_reused_sid, lane, 2, 1, scope, 0);
    rv.routes
        .record_with_role_count(second_reused_sid, lane, 2, 1, scope, 1);
    assert_eq!(
        rv.routes
            .poll_with_role_count(retained_sid, lane, 2, 0, scope),
        Poll::Ready(1)
    );
    assert_eq!(
        rv.routes
            .poll_with_role_count(first_reused_sid, lane, 2, 0, scope),
        Poll::Ready(0)
    );
    assert_eq!(
        rv.routes
            .poll_with_role_count(second_reused_sid, lane, 2, 0, scope),
        Poll::Ready(1)
    );
}

#[test]
fn reserved_endpoint_lease_is_invisible_to_lookup_and_wake() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_endpoint_lease_capacity(1)
        .expect("endpoint lease table");
    let sid = crate::session::types::SessionId::new(1);
    let generation = rv.next_endpoint_lease_generation();
    let lease_slot = crate::rendezvous::core::EndpointLeaseId::from(0u8);
    rv.write_endpoint_lease_slot(
        0,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation,
            sid,
            role: 0,
            offset: u32::MAX,
            len: u32::MAX,
            resident_budget: crate::rendezvous::core::EndpointResidentBudget::ZERO,
            state: crate::rendezvous::core::EndpointLeaseState::Reserved,
        },
    );

    assert!(rv.endpoint_lease_storage(lease_slot, generation).is_none());
    rv.wake_session_endpoint_waiters(sid, 1);

    rv.release_endpoint_lease(lease_slot, generation);
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
}

#[test]
fn endpoint_lease_lookup_rejects_out_of_range_before_record_access() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let generation = 1;

    assert!(
        rv.endpoint_lease_storage(
            crate::rendezvous::core::EndpointLeaseId::from(0u8),
            generation,
        )
        .is_none()
    );
    rv.ensure_endpoint_lease_capacity(1)
        .expect("endpoint lease table");
    assert!(
        rv.endpoint_lease_storage(
            crate::rendezvous::core::EndpointLeaseId::from(1u8),
            generation,
        )
        .is_none()
    );
}

#[test]
fn endpoint_lease_generation_wrap_skips_vacant_generation() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.endpoint_lease_generation.set(u32::MAX);

    assert_eq!(rv.next_endpoint_lease_generation(), 1);
    assert_eq!(rv.next_endpoint_lease_generation(), 2);
}

#[test]
fn published_endpoint_lease_cannot_be_published_twice() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let header_bytes = core::mem::size_of::<crate::endpoint::carrier::KernelEndpointHeader<'_>>();
    let header_align = core::mem::align_of::<crate::endpoint::carrier::KernelEndpointHeader<'_>>();
    let (lease_slot, generation, _, _) = rv
        .allocate_endpoint_lease(
            crate::session::types::SessionId::new(1),
            0,
            header_bytes,
            header_align,
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        )
        .expect("reserved endpoint lease");

    rv.publish_endpoint_lease(lease_slot, generation);
    let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rv.publish_endpoint_lease(lease_slot, generation);
    }));
    assert!(duplicate.is_err());

    rv.release_endpoint_lease(lease_slot, generation);
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
}

#[test]
fn route_budget_sums_distinct_sessions_and_maxes_roles() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_endpoint_lease_capacity(3)
        .expect("endpoint lease table");
    let budget = |route_frame_slots| crate::rendezvous::core::EndpointResidentBudget {
        route_frame_slots,
        route_lane_slots: 1,
        frontier_workspace_bytes: 0,
    };
    let occupied = |sid, role, resident_budget| crate::rendezvous::core::EndpointLeaseSlot {
        generation: 1,
        sid,
        role,
        offset: 0,
        len: 1,
        resident_budget,
        state: crate::rendezvous::core::EndpointLeaseState::Reserved,
    };
    let sid_a = crate::session::types::SessionId::new(1);
    let sid_b = crate::session::types::SessionId::new(2);
    rv.write_endpoint_lease_slot(0, occupied(sid_a, 0, budget(3)));
    rv.write_endpoint_lease_slot(1, occupied(sid_a, 1, budget(5)));
    rv.write_endpoint_lease_slot(2, occupied(sid_b, 0, budget(4)));

    assert_eq!(rv.resident_route_frame_slots_floor(), 9);
    rv.write_endpoint_lease_slot(
        1,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation: 1,
            ..crate::rendezvous::core::EndpointLeaseSlot::EMPTY
        },
    );
    assert_eq!(rv.resident_route_frame_slots_floor(), 7);
    rv.write_endpoint_lease_slot(1, occupied(sid_a, 1, budget(5)));
    rv.write_endpoint_lease_slot(
        0,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation: 1,
            ..crate::rendezvous::core::EndpointLeaseSlot::EMPTY
        },
    );
    assert_eq!(rv.resident_route_frame_slots_floor(), 9);
    rv.write_endpoint_lease_slot(
        2,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation: 1,
            ..crate::rendezvous::core::EndpointLeaseSlot::EMPTY
        },
    );
    assert_eq!(rv.resident_route_frame_slots_floor(), 5);
}

#[test]
fn endpoint_lease_shrink_preserves_owner_generation_across_rebind() {
    assert_eq!(
        core::mem::size_of::<crate::rendezvous::core::EndpointLeaseSlot>(),
        24,
        "endpoint lease ledger must stay compact on Pico"
    );
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_endpoint_lease_capacity(3)
        .expect("endpoint lease table");
    let first_generation = rv.next_endpoint_lease_generation();
    let (_, slab_len) = rv.slab_ptr_and_len();
    rv.write_endpoint_lease_slot(
        0,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation: first_generation,
            sid: crate::session::types::SessionId::new(1),
            role: 0,
            offset: crate::invariant_ok(u32::try_from(slab_len - 1)),
            len: 1,
            resident_budget: crate::rendezvous::core::EndpointResidentBudget::ZERO,
            state: crate::rendezvous::core::EndpointLeaseState::Reserved,
        },
    );
    rv.shrink_endpoint_lease_capacity();
    assert_eq!(rv.endpoint_lease_slot_count(), 1);

    rv.write_endpoint_lease_slot(
        0,
        crate::rendezvous::core::EndpointLeaseSlot {
            generation: first_generation,
            ..crate::rendezvous::core::EndpointLeaseSlot::EMPTY
        },
    );
    rv.shrink_endpoint_lease_capacity();
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
    rv.ensure_endpoint_lease_capacity(1)
        .expect("rebound endpoint lease table");
    assert_ne!(rv.next_endpoint_lease_generation(), first_generation);
}

#[test]
fn repeated_reserved_release_restores_all_resident_high_water() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let lane0 = crate::session::types::Lane::new(0);
    let lane2 = crate::session::types::Lane::new(2);
    let scope = ScopeId::new(ScopeKind::Route, 0);
    let resident_budget = crate::rendezvous::core::EndpointResidentBudget {
        route_frame_slots: 4,
        route_lane_slots: 3,
        frontier_workspace_bytes: 32,
    };

    let mut cycle = 0u32;
    while cycle < 16 {
        let sid = crate::session::types::SessionId::new(cycle + 1);
        let (lease_slot, generation, _, _) = rv
            .allocate_endpoint_lease(sid, 0, 128, core::mem::align_of::<usize>(), resident_budget)
            .expect("reserved endpoint lease");
        rv.ensure_endpoint_resident_capacity()
            .expect("resident route/frontier capacity");
        rv.ensure_core_lane_tables_for_assoc_entries(3, 2)
            .expect("association capacity");
        rv.activate_lane_attachment(sid, lane0)
            .expect("lane 0 claim");
        rv.activate_lane_attachment(sid, lane2)
            .expect("lane 2 claim");
        assert!(
            rv.routes
                .poll_with_role_count(sid, lane2, 2, 0, scope)
                .is_pending()
        );

        assert_eq!(
            rv.release_lane(sid, lane2),
            crate::rendezvous::core::LaneRelease::Released
        );
        assert_eq!(
            rv.release_lane(sid, lane0),
            crate::rendezvous::core::LaneRelease::Released
        );
        rv.release_endpoint_lease(lease_slot, generation);

        assert_eq!(rv.endpoint_lease_slot_count(), 0);
        assert_eq!(rv.assoc.assoc_slots(), 0);
        assert_eq!(rv.routes.route_slots(), 0);
        assert_eq!(rv.lane_slot_count(), 0);
        assert_eq!(rv.frontier_workspace_bytes.get(), 0);
        assert_eq!(rv.image_frontier.get(), 0);
        assert!(rv.endpoint_lease_storage.get().is_empty());
        assert!(rv.assoc_storage.get().is_empty());
        assert!(rv.route_storage.get().is_empty());
        cycle += 1;
    }
}
