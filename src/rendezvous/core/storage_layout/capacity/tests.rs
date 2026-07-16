use super::*;
use crate::{
    runtime_core::resources::RuntimeResources,
    session::types::RendezvousId,
    transport::{Outgoing, PortOpen, ReceivedFrame, TransportError},
};
use core::task::{Context, Poll};

mod formal_certificate_export;

static_assertions::assert_not_impl_any!(super::PersistentSidecarLease: Copy, Clone);
static_assertions::assert_not_impl_any!(
    super::endpoint_lease::EndpointLeaseCapacityPlan: Copy,
    Clone
);

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

fn bind_endpoint_lease_capacity(rv: &Rendezvous<'_, '_, FailingTransport>, required_slots: usize) {
    let plan = rv
        .plan_endpoint_lease_capacity(required_slots)
        .expect("endpoint lease capacity plan");
    rv.commit_endpoint_lease_capacity(plan);
}

fn populate_non_endpoint_sidecars(rv: &Rendezvous<'_, '_, FailingTransport>) {
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("association storage");
    rv.ensure_dynamic_resolver_capacity(1)
        .expect("resolver storage");
}

fn resident_owner_capacities(rv: &Rendezvous<'_, '_, FailingTransport>) -> [usize; 3] {
    rv.live_sidecars().map(|resident| resident.storage.bytes())
}

#[test]
fn nonempty_sidecar_before_slab_fails_closed() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let (slab_ptr, _) = rv.slab_ptr_and_len();
    let malformed = Sidecar::from_raw_parts(slab_ptr.wrapping_sub(1), 1);

    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        core::hint::black_box(rv.sidecar_range(malformed));
    }));
    assert!(rejected.is_err());
}

#[test]
fn association_storage_rejects_zero_requirements_without_allocating_reserves() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let frontier_before = rv.image_frontier.get();

    assert_eq!(
        rv.ensure_core_lane_tables_for_assoc_entries(0, 1),
        Err(ResourceScope::LaneStorage)
    );
    assert_eq!(
        rv.ensure_core_lane_tables_for_assoc_entries(1, 0),
        Err(ResourceScope::LaneStorage)
    );
    assert_eq!(rv.lane_slot_count(), 0);
    assert_eq!(rv.assoc.assoc_slots(), 0);
    assert!(rv.assoc_storage.get().is_empty());
    assert_eq!(rv.image_frontier.get(), frontier_before);
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
fn association_accepts_256_attachments_without_corrupting_fault_state() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("association storage");
    let lane = crate::session::types::Lane::new(0);
    let sid = crate::session::types::SessionId::new(1);

    let mut attached = 0u16;
    while attached < 256 {
        rv.activate_lane_attachment(sid, lane)
            .expect("full u8 role-domain attachment");
        attached += 1;
    }
    assert_eq!(
        rv.activate_lane_attachment(sid, lane),
        Err(crate::rendezvous::error::RendezvousError::LaneAttachOverflow { lane })
    );

    let fault = crate::rendezvous::SessionFaultKind::ProtocolViolation;
    assert_eq!(rv.poison_session(sid, u8::MAX, fault), fault);
    assert_eq!(rv.session_fault(sid), Some(fault));

    let mut released = 0u16;
    while released < 255 {
        assert_eq!(
            rv.release_lane(sid, lane),
            crate::rendezvous::core::LaneRelease::StillHeld
        );
        assert_eq!(rv.session_fault(sid), Some(fault));
        released += 1;
    }
    assert_eq!(
        rv.release_lane(sid, lane),
        crate::rendezvous::core::LaneRelease::Released
    );
    assert_eq!(rv.session_fault(sid), None);
}

#[test]
fn reserved_endpoint_lease_is_invisible_to_lookup_and_wake() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    bind_endpoint_lease_capacity(rv, 1);
    let sid = crate::session::types::SessionId::new(1);
    let generation = rv
        .next_endpoint_lease_generation()
        .expect("endpoint generation");
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
    bind_endpoint_lease_capacity(rv, 1);
    assert!(
        rv.endpoint_lease_storage(
            crate::rendezvous::core::EndpointLeaseId::from(1u8),
            generation,
        )
        .is_none()
    );
}

#[test]
fn endpoint_lease_generation_exhaustion_fails_closed() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    rv.endpoint_lease_generation.set(u32::MAX - 1);

    assert_eq!(rv.next_endpoint_lease_generation(), Some(u32::MAX));
    assert_eq!(rv.next_endpoint_lease_generation(), None);
    assert_eq!(rv.endpoint_lease_generation.get(), u32::MAX);
    assert_eq!(
        rv.allocate_endpoint_lease(
            crate::session::types::SessionId::new(1),
            0,
            1,
            1,
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        ),
        Err(ResourceScope::EndpointLease)
    );
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
    assert_eq!(rv.image_frontier.get(), 0);
}

#[test]
fn endpoint_lease_payload_exhaustion_preserves_resident_state() {
    let mut slab = [0u8; 4096];
    let slab_bytes = slab.len();
    let rv = init_test_rendezvous(&mut slab);

    assert_eq!(
        rv.allocate_endpoint_lease(
            crate::session::types::SessionId::new(1),
            0,
            slab_bytes,
            1,
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        ),
        Err(ResourceScope::EndpointLease)
    );
    assert_eq!(rv.endpoint_lease_generation.get(), 0);
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
    assert!(rv.endpoint_lease_storage.get().is_empty());
    assert_eq!(rv.image_frontier.get(), 0);
}

#[test]
fn endpoint_lease_capacity_plan_is_read_only_until_commit() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let endpoint_before = rv.endpoint_lease_storage.get();
    let assoc_before = rv.assoc_storage.get();
    let resolver_before = rv.resolver_storage_sidecar();
    let frontier_before = rv.image_frontier.get();

    let plan = rv
        .plan_endpoint_lease_capacity(1)
        .expect("endpoint lease capacity plan");

    assert!(endpoint_before.is_empty());
    assert_eq!(rv.endpoint_lease_storage.get().ptr(), endpoint_before.ptr());
    assert_eq!(rv.assoc_storage.get().ptr(), assoc_before.ptr());
    assert_eq!(rv.resolver_storage_sidecar().ptr(), resolver_before.ptr());
    assert_eq!(rv.endpoint_lease_slot_count(), 0);
    assert_eq!(rv.image_frontier.get(), frontier_before);

    rv.commit_endpoint_lease_capacity(plan);
    assert_eq!(rv.endpoint_lease_slot_count(), 1);
    assert!(!rv.endpoint_lease_storage.get().is_empty());
}

#[test]
fn first_endpoint_table_commit_preserves_existing_sidecar_owners() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let sid = crate::session::types::SessionId::new(1);
    let lane = crate::session::types::Lane::new(0);
    rv.ensure_core_lane_tables_for_assoc_entries(1, 1)
        .expect("association storage");
    rv.activate_lane_attachment(sid, lane)
        .expect("live association");
    rv.ensure_dynamic_resolver_capacity(2)
        .expect("resolver storage");

    rv.allocate_endpoint_lease(
        crate::session::types::SessionId::new(2),
        0,
        128,
        core::mem::align_of::<usize>(),
        crate::rendezvous::core::EndpointResidentBudget::ZERO,
    )
    .expect("first endpoint lease");

    assert!(rv.has_lane_attachment(sid, lane));
    assert!(!rv.resolver_storage_sidecar().is_empty());
}

#[test]
fn endpoint_lease_growth_failure_preserves_existing_owner_state() {
    let mut slab = [0u8; 4096];
    let slab_bytes = slab.len();
    let rv = init_test_rendezvous(&mut slab);
    let (lease_slot, generation, _, _) = rv
        .allocate_endpoint_lease(
            crate::session::types::SessionId::new(1),
            0,
            64,
            core::mem::align_of::<usize>(),
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        )
        .expect("first endpoint lease");
    populate_non_endpoint_sidecars(rv);
    let storage_before = rv.endpoint_lease_storage.get();
    let owners_before = resident_owner_capacities(rv);
    let slot_before = rv
        .endpoint_lease_slot_by_index(usize::from(lease_slot))
        .expect("first endpoint slot");
    let frontier_before = rv.image_frontier.get();

    assert_eq!(
        rv.allocate_endpoint_lease(
            crate::session::types::SessionId::new(2),
            0,
            slab_bytes,
            1,
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        ),
        Err(ResourceScope::EndpointLease)
    );
    assert_eq!(rv.endpoint_lease_generation.get(), generation);
    assert_eq!(rv.endpoint_lease_slot_count(), 1);
    assert_eq!(rv.endpoint_lease_storage.get().ptr(), storage_before.ptr());
    assert_eq!(
        rv.endpoint_lease_storage.get().bytes(),
        storage_before.bytes()
    );
    assert_eq!(rv.image_frontier.get(), frontier_before);
    assert_eq!(resident_owner_capacities(rv), owners_before);
    assert_eq!(
        rv.endpoint_lease_slot_by_index(usize::from(lease_slot)),
        Some(slot_before)
    );
}

#[test]
fn aborted_endpoint_reservation_restores_generation_and_owner_capacities() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let first_sid = crate::session::types::SessionId::new(1);
    let (first_slot, first_generation, _, _) = rv
        .allocate_endpoint_lease(
            first_sid,
            0,
            64,
            core::mem::align_of::<usize>(),
            crate::rendezvous::core::EndpointResidentBudget {
                frontier_workspace_bytes: 0,
            },
        )
        .expect("first endpoint lease");
    rv.publish_endpoint_lease(first_slot, first_generation);
    populate_non_endpoint_sidecars(rv);
    rv.activate_lane_attachment(first_sid, crate::session::types::Lane::new(0))
        .expect("existing lane authority");
    let first_before = rv
        .endpoint_lease_slot_by_index(usize::from(first_slot))
        .expect("first endpoint slot");
    let owners_before = resident_owner_capacities(rv);
    let frontier_before = rv.image_frontier.get();

    let (aborted_slot, aborted_generation, _, _) = rv
        .allocate_endpoint_lease(
            crate::session::types::SessionId::new(2),
            0,
            64,
            core::mem::align_of::<usize>(),
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        )
        .expect("reserved endpoint lease");
    rv.abort_endpoint_lease_reservation(aborted_slot, aborted_generation);

    assert_eq!(rv.endpoint_lease_generation.get(), first_generation);
    assert_eq!(rv.endpoint_lease_slot_count(), 1);
    assert!(rv.image_frontier.get() <= frontier_before);
    assert_eq!(resident_owner_capacities(rv), owners_before);
    assert_eq!(rv.active_lane_attachment_count(), 1);
    assert!(rv.has_lane_attachment(first_sid, crate::session::types::Lane::new(0)));
    assert_eq!(
        rv.endpoint_lease_slot_by_index(usize::from(first_slot)),
        Some(first_before)
    );
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
fn endpoint_lease_shrink_preserves_owner_generation_across_rebind() {
    assert_eq!(
        core::mem::size_of::<crate::rendezvous::core::EndpointLeaseSlot>(),
        20,
        "endpoint lease ledger must stay compact on Pico"
    );
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    bind_endpoint_lease_capacity(rv, 3);
    let first_generation = rv
        .next_endpoint_lease_generation()
        .expect("first endpoint generation");
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
    bind_endpoint_lease_capacity(rv, 1);
    assert_ne!(
        rv.next_endpoint_lease_generation()
            .expect("rebound endpoint generation"),
        first_generation
    );
}

#[test]
fn repeated_reserved_release_restores_all_resident_high_water() {
    let mut slab = [0u8; 4096];
    let rv = init_test_rendezvous(&mut slab);
    let lane0 = crate::session::types::Lane::new(0);
    let lane2 = crate::session::types::Lane::new(2);
    let resident_budget = crate::rendezvous::core::EndpointResidentBudget {
        frontier_workspace_bytes: 32,
    };

    let mut cycle = 0u32;
    while cycle < 16 {
        let sid = crate::session::types::SessionId::new(cycle + 1);
        let (lease_slot, generation, _, _) = rv
            .allocate_endpoint_lease(sid, 0, 128, core::mem::align_of::<usize>(), resident_budget)
            .expect("reserved endpoint lease");
        rv.ensure_endpoint_resident_capacity()
            .expect("resident frontier capacity");
        rv.ensure_core_lane_tables_for_assoc_entries(3, 2)
            .expect("association capacity");
        rv.activate_lane_attachment(sid, lane0)
            .expect("lane 0 claim");
        rv.activate_lane_attachment(sid, lane2)
            .expect("lane 2 claim");
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
        assert_eq!(rv.lane_slot_count(), 0);
        assert_eq!(rv.frontier_workspace_bytes.get(), 0);
        assert_eq!(rv.image_frontier.get(), 0);
        assert!(rv.endpoint_lease_storage.get().is_empty());
        assert!(rv.assoc_storage.get().is_empty());
        cycle += 1;
    }
}
