use super::super::{Rendezvous, Sidecar, Transport};
use super::{PersistentSidecarLease, ResidentSidecarKind};
use crate::{
    rendezvous::core::{EndpointLeaseId, EndpointLeaseRecord, EndpointLeaseSlot},
    session::cluster::error::ResourceScope,
};

pub(in crate::rendezvous::core) enum EndpointLeaseCapacityPlan {
    Retained,
    Replace {
        required_slots: usize,
        lease: PersistentSidecarLease,
    },
}

pub(in crate::rendezvous::core) const fn next_endpoint_lease_generation(
    current: u32,
) -> Option<u32> {
    current.checked_add(1)
}

impl EndpointLeaseRecord {
    fn take_published_waiter(&self) -> Option<core::task::Waker> {
        if !self.slot().is_published() {
            crate::invariant();
        }
        self.take_waiter()
    }

    pub(crate) fn wake_session_waiters(
        storage: &core::cell::Cell<Sidecar<Self>>,
        sid: crate::session::types::SessionId,
        excluded_role: u8,
    ) {
        if excluded_role >= crate::g::ROLE_DOMAIN_SIZE {
            crate::invariant();
        }
        let mut idx = 0usize;
        loop {
            let current = storage.get();
            let slot_count = Self::storage_slot_count(current);
            if idx >= slot_count {
                return;
            }
            let record = unsafe {
                // SAFETY: `idx` is inside the freshly reloaded lease-record
                // sidecar. No record reference survives the callback below.
                &*current.ptr().add(idx)
            };
            let slot = record.slot();
            let waiter = if slot.is_published() && slot.sid == sid && slot.role != excluded_role {
                record.take_published_waiter()
            } else {
                None
            };
            idx += 1;
            if let Some(waiter) = waiter {
                // No sidecar pointer or lease-record reference survives this
                // callback. The next iteration reloads the owner root.
                waiter.wake();
            }
        }
    }
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn endpoint_lease_slot_count(&self) -> usize {
        EndpointLeaseRecord::storage_slot_count(self.endpoint_lease_storage.get())
    }

    #[inline]
    fn with_endpoint_lease_record_at<R>(
        &self,
        idx: usize,
        f: impl FnOnce(&EndpointLeaseRecord) -> R,
    ) -> Option<R> {
        if idx >= self.endpoint_lease_slot_count() {
            return None;
        }
        let records = self.endpoint_lease_records_ptr();
        Some(unsafe {
            /* SAFETY: `idx` is within the initialized record table. The scoped
            callback result cannot borrow this relocatable record, and callers
            invoke no Waker callback while it runs. */
            f(&*records.add(idx))
        })
    }

    #[inline]
    pub(crate) fn endpoint_lease_slot_by_index(&self, idx: usize) -> Option<EndpointLeaseSlot> {
        self.with_endpoint_lease_record_at(idx, EndpointLeaseRecord::slot)
    }

    #[inline]
    pub(crate) fn write_endpoint_lease_slot(&self, idx: usize, slot: EndpointLeaseSlot) {
        if idx >= self.endpoint_lease_slot_count() {
            crate::invariant();
        }
        let records = self.endpoint_lease_records_ptr();
        /* SAFETY: `idx` is inside the owner-bound record table. The local-only
        rendezvous serializes metadata updates; writing only `slot` preserves
        the record's independent wake owner. */
        unsafe {
            core::ptr::addr_of_mut!((*records.add(idx)).slot).write(slot);
        }
    }

    #[inline]
    fn with_endpoint_lease_record<R>(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
        f: impl FnOnce(&EndpointLeaseRecord) -> R,
    ) -> Option<R> {
        let idx = usize::from(lease_slot);
        self.with_endpoint_lease_record_at(idx, |record| {
            let slot = record.slot();
            (slot.is_published() && slot.generation == generation).then(|| f(record))
        })?
    }

    #[inline]
    fn endpoint_lease(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<EndpointLeaseSlot> {
        self.with_endpoint_lease_record(lease_slot, generation, EndpointLeaseRecord::slot)
    }

    pub(crate) fn wake_session_endpoint_waiters(
        &self,
        sid: crate::session::types::SessionId,
        excluded_role: u8,
    ) {
        EndpointLeaseRecord::wake_session_waiters(&self.endpoint_lease_storage, sid, excluded_role);
    }

    #[inline]
    pub(crate) fn replace_endpoint_waiter(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
        replacement: core::task::Waker,
    ) -> Option<core::task::Waker> {
        crate::invariant_some(
            self.with_endpoint_lease_record(lease_slot, generation, |record| {
                record.replace_waiter(replacement)
            }),
        )
    }

    #[inline]
    pub(crate) fn take_endpoint_waiter(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<core::task::Waker> {
        crate::invariant_some(self.with_endpoint_lease_record(
            lease_slot,
            generation,
            EndpointLeaseRecord::take_waiter,
        ))
    }

    #[inline]
    pub(crate) fn require_empty_endpoint_waiter_slot(&self, idx: usize) {
        let is_empty = crate::invariant_some(
            self.with_endpoint_lease_record_at(idx, EndpointLeaseRecord::waiter_is_empty),
        );
        if !is_empty {
            crate::invariant();
        }
    }

    #[inline]
    pub(crate) fn next_endpoint_lease_generation(&self) -> Option<u32> {
        let next = next_endpoint_lease_generation(self.endpoint_lease_generation.get())?;
        self.endpoint_lease_generation.set(next);
        Some(next)
    }

    #[inline]
    fn endpoint_lease_storage_bytes(capacity: usize) -> Option<usize> {
        capacity.checked_mul(core::mem::size_of::<EndpointLeaseRecord>())
    }

    pub(in crate::rendezvous::core) fn plan_endpoint_lease_capacity(
        &self,
        required_slots: usize,
    ) -> Result<EndpointLeaseCapacityPlan, ResourceScope> {
        let current = self.endpoint_lease_slot_count();
        if required_slots <= current {
            return Ok(EndpointLeaseCapacityPlan::Retained);
        }
        if EndpointLeaseId::try_from(required_slots).is_err() {
            return Err(ResourceScope::EndpointLease);
        }
        let bytes = Self::endpoint_lease_storage_bytes(required_slots)
            .ok_or(ResourceScope::EndpointLease)?;
        let lease = self
            .plan_persistent_sidecar_bytes(bytes, core::mem::align_of::<EndpointLeaseRecord>())
            .ok_or(ResourceScope::EndpointLease)?;
        Ok(EndpointLeaseCapacityPlan::Replace {
            required_slots,
            lease,
        })
    }

    pub(in crate::rendezvous::core) fn endpoint_lease_floor_after_capacity_plan(
        &self,
        plan: &EndpointLeaseCapacityPlan,
    ) -> Result<usize, ResourceScope> {
        let EndpointLeaseCapacityPlan::Replace { lease, .. } = plan else {
            return Ok(self.endpoint_lease_floor());
        };
        let mut sidecars = self.live_sidecars();
        if !matches!(sidecars[0].kind, ResidentSidecarKind::EndpointLeases) {
            crate::invariant();
        }
        sidecars[0].storage = lease.storage();
        self.packed_sidecar_frontier(sidecars)
            .and_then(|frontier| frontier.checked_add(self.frontier_workspace_bytes.get() as usize))
            .ok_or(ResourceScope::EndpointLease)
    }

    pub(in crate::rendezvous::core) fn commit_endpoint_lease_capacity(
        &self,
        plan: EndpointLeaseCapacityPlan,
    ) {
        let EndpointLeaseCapacityPlan::Replace {
            required_slots,
            lease,
        } = plan
        else {
            return;
        };
        let current = self.endpoint_lease_slot_count();
        if required_slots <= current {
            crate::invariant();
        }
        let source_sidecar = self.endpoint_lease_storage.get();
        let lease = self.commit_persistent_sidecar_lease(lease);
        let new_ptr = lease.ptr().cast::<EndpointLeaseRecord>();
        unsafe {
            /* SAFETY: the replacement is disjoint and unpublished. The loop
            initializes its tail, then bytewise relocation transfers each live
            Waker exactly once before the owner root is published. */
            let mut idx = current;
            while idx < required_slots {
                new_ptr.add(idx).write(EndpointLeaseRecord::empty());
                idx += 1;
            }
            if current != 0 {
                core::ptr::copy_nonoverlapping(source_sidecar.ptr(), new_ptr, current);
            }
        }
        self.endpoint_lease_storage.set(lease.cast());
        if source_sidecar.is_empty() {
            self.compact_live_sidecars();
        } else {
            self.retire_persistent_sidecar(source_sidecar.cast());
        }
    }

    pub(crate) fn shrink_endpoint_lease_capacity(&self) {
        let current_slots = self.endpoint_lease_slot_count();
        let mut required_slots = current_slots;
        while required_slots != 0 {
            let idx = required_slots - 1;
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_occupied() {
                break;
            }
            self.require_empty_endpoint_waiter_slot(idx);
            required_slots -= 1;
        }
        if required_slots == current_slots {
            return;
        }
        let source = self.endpoint_lease_storage.get();
        if required_slots == 0 {
            self.endpoint_lease_storage.set(Sidecar::EMPTY);
            self.retire_persistent_sidecar(source.cast());
            return;
        }
        let bytes = crate::invariant_some(Self::endpoint_lease_storage_bytes(required_slots));
        self.endpoint_lease_storage
            .set(Sidecar::from_raw_parts(source.ptr(), bytes));
        self.compact_live_sidecars();
    }

    #[inline]
    pub(crate) fn endpoint_lease_storage(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<(usize, usize)> {
        let slot = self.endpoint_lease(lease_slot, generation)?;
        Some((slot.offset as usize, slot.len as usize))
    }

    pub(crate) fn resident_route_frame_slots_floor(&self) -> usize {
        let slot_count = self.endpoint_lease_slot_count();
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if !slot.is_occupied() {
                idx += 1;
                continue;
            }
            let mut prior_idx = 0usize;
            let mut is_session_leader = true;
            while prior_idx < idx {
                let prior = crate::invariant_some(self.endpoint_lease_slot_by_index(prior_idx));
                if prior.is_occupied() && prior.sid == slot.sid {
                    is_session_leader = false;
                    break;
                }
                prior_idx += 1;
            }
            if is_session_leader {
                let mut session_required = 0usize;
                let mut peer_idx = idx;
                while peer_idx < slot_count {
                    let peer = crate::invariant_some(self.endpoint_lease_slot_by_index(peer_idx));
                    if peer.is_occupied() && peer.sid == slot.sid {
                        session_required = core::cmp::max(
                            session_required,
                            peer.resident_budget.route_frame_slots as usize,
                        );
                    }
                    peer_idx += 1;
                }
                required = crate::invariant_some(required.checked_add(session_required));
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_route_lane_slots_floor(&self) -> usize {
        let slot_count = self.endpoint_lease_slot_count();
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_occupied() {
                required = core::cmp::max(required, slot.resident_budget.route_lane_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_frontier_workspace_floor(&self) -> usize {
        let slot_count = self.endpoint_lease_slot_count();
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_occupied() {
                required = core::cmp::max(
                    required,
                    slot.resident_budget.frontier_workspace_bytes as usize,
                );
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn ensure_frontier_workspace_capacity(
        &self,
        required_bytes: usize,
    ) -> Result<(), ResourceScope> {
        if required_bytes > u32::MAX as usize {
            return Err(ResourceScope::EndpointLease);
        }
        if required_bytes <= self.frontier_workspace_bytes.get() as usize {
            return Ok(());
        }
        let floor = (self.image_frontier.get() as usize)
            .checked_add(required_bytes)
            .ok_or(ResourceScope::EndpointLease)?;
        if floor > self.endpoint_storage_floor() {
            return Err(ResourceScope::EndpointLease);
        }
        self.set_frontier_workspace_bytes(required_bytes as u32);
        Ok(())
    }

    #[inline]
    pub(crate) fn recompute_frontier_workspace_bytes(&self) {
        let required = self.resident_frontier_workspace_floor();
        let required = crate::invariant_ok(u32::try_from(required));
        self.set_frontier_workspace_bytes(required);
    }
}
