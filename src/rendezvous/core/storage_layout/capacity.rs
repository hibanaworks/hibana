use super::{AssocTable, Rendezvous, RouteTable, Sidecar, Transport};
use crate::session::cluster::error::ResourceScope;
mod arena;
mod endpoint_lease;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

// # Unsafe Owner Contract
//
// This file owns rendezvous sidecar capacity growth and resident table rebinding
// after storage has been allocated by the parent storage layout owner. Raw
// endpoint-lease and sidecar pointers are range-checked against the pinned
// rendezvous slab. Allocation derives gaps from the four live owner roots, and
// retirement packs those roots canonically and recomputes the frontier, so
// replacement history cannot consume resident storage.

#[derive(Clone, Copy)]
enum ResidentSidecarKind {
    EndpointLeases,
    Associations,
    Routes,
    Resolvers,
}

#[derive(Clone, Copy)]
struct ResidentSidecar {
    kind: ResidentSidecarKind,
    storage: Sidecar<u8>,
    align: usize,
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    fn live_sidecars(&self) -> [ResidentSidecar; 4] {
        [
            ResidentSidecar {
                kind: ResidentSidecarKind::EndpointLeases,
                storage: self.endpoint_lease_storage.get().cast(),
                align: core::mem::align_of::<super::EndpointLeaseSlot>(),
            },
            ResidentSidecar {
                kind: ResidentSidecarKind::Associations,
                storage: self.assoc_storage.get(),
                align: AssocTable::storage_align(),
            },
            ResidentSidecar {
                kind: ResidentSidecarKind::Routes,
                storage: self.route_storage.get(),
                align: RouteTable::storage_align(),
            },
            ResidentSidecar {
                kind: ResidentSidecarKind::Resolvers,
                storage: self.resolver_storage_sidecar(),
                align: crate::session::cluster::core::ResolverBucket::storage_align(),
            },
        ]
    }

    #[inline]
    fn sidecar_range(&self, sidecar: Sidecar<u8>) -> Option<(usize, usize)> {
        if sidecar.is_empty() {
            return None;
        }
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let start = crate::invariant_some(sidecar.ptr().addr().checked_sub(slab_ptr.addr()));
        let end = crate::invariant_some(start.checked_add(sidecar.bytes()));
        if end > slab_len {
            crate::invariant();
        }
        Some((start, end))
    }

    #[inline]
    pub(in crate::rendezvous::core) fn allocate_persistent_sidecar_bytes(
        &self,
        bytes: usize,
        align: usize,
    ) -> Option<Sidecar<u8>> {
        if bytes == 0 {
            crate::invariant();
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let mut start = Self::align_up(base, align).checked_sub(base)?;
        loop {
            let end = start.checked_add(bytes)?;
            let mut conflict_end = None;
            for resident in self.live_sidecars() {
                let Some((live_start, live_end)) = self.sidecar_range(resident.storage) else {
                    continue;
                };
                if start < live_end && end > live_start {
                    conflict_end =
                        Some(conflict_end.map_or(live_end, |seen: usize| seen.max(live_end)));
                }
            }
            if let Some(next) = conflict_end {
                start = Self::align_up(base.checked_add(next)?, align).checked_sub(base)?;
                continue;
            }
            let reserved_end = end.checked_add(self.frontier_workspace_bytes.get() as usize)?;
            if reserved_end > self.endpoint_storage_floor() || end > u32::MAX as usize {
                return None;
            }
            self.set_image_frontier(
                self.image_frontier
                    .get()
                    .max(crate::invariant_ok(u32::try_from(end))),
            );
            let ptr = /* SAFETY: `start..end` is disjoint from every live
            sidecar and `reserved_end` stays below endpoint storage. */ unsafe {
                slab_ptr.add(start)
            };
            return Some(Sidecar::from_raw_parts(ptr, bytes));
        }
    }

    #[inline]
    pub(in crate::rendezvous::core) fn retire_persistent_sidecar(&self, sidecar: Sidecar<u8>) {
        if sidecar.is_empty() {
            return;
        }
        let retired = crate::invariant_some(self.sidecar_range(sidecar));
        if retired.1 > self.image_frontier.get() as usize {
            crate::invariant();
        }
        for live in self.live_sidecars() {
            if !live.storage.is_empty() && live.storage.ptr() == sidecar.ptr() {
                crate::invariant();
            }
        }
        self.compact_live_sidecars();
    }

    #[inline]
    fn lane_base(&self) -> u32 {
        self.lane_base.get()
    }

    #[inline]
    fn lane_slot_count(&self) -> usize {
        if self.lane_end.get() < self.lane_base.get() {
            crate::invariant();
        }
        (self.lane_end.get() - self.lane_base.get()) as usize
    }

    #[inline]
    fn target_lane_slots(required_lane_slots: usize) -> Option<usize> {
        if required_lane_slots > usize::from(crate::runtime_core::consts::LANE_DOMAIN_SIZE) {
            return None;
        }
        Some(required_lane_slots.max(1))
    }

    #[inline]
    fn target_assoc_slots(required_assoc_slots: usize) -> Option<usize> {
        if required_assoc_slots > usize::from(u16::MAX) {
            return None;
        }
        Some(required_assoc_slots.max(1))
    }

    #[inline]
    fn lease_sidecar<S>(&self, bytes: usize, align: usize) -> Option<Sidecar<S>> {
        Some(
            self.allocate_persistent_sidecar_bytes(bytes, align)?
                .cast::<S>(),
        )
    }

    #[inline]
    fn retire_sidecar<S>(&self, sidecar: Sidecar<S>) {
        self.retire_persistent_sidecar(sidecar.cast::<u8>());
    }

    pub(crate) fn ensure_dynamic_resolver_capacity(
        &self,
        additional_entries: usize,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        let bucket = /* SAFETY: shared planning only reads the initialized
        resolver root and copied entries. */ unsafe { &*self.resolver_bucket.get() };
        let Some(required) = bucket.required_capacity(additional_entries)? else {
            return Ok(());
        };
        let source = bucket.erased_storage_sidecar();
        let storage = self
            .allocate_persistent_sidecar_bytes(
                crate::session::cluster::core::ResolverBucket::storage_bytes(required),
                crate::session::cluster::core::ResolverBucket::storage_align(),
            )
            .ok_or(
                crate::session::cluster::error::ClusterError::resource_exhausted(
                    ResourceScope::ResolverTable,
                ),
            )?;
        /* SAFETY: no mutable resolver borrow existed during allocation. This
        local-only update copies initialized entries and publishes the new root
        without invoking external code. */
        unsafe {
            (&mut *self.resolver_bucket.get()).replace_storage(storage, required);
        }
        self.retire_persistent_sidecar(source);
        Ok(())
    }

    fn ensure_lane_storage(
        &self,
        required_lane_slots: usize,
        required_assoc_slots: usize,
    ) -> Result<(), ResourceScope> {
        let target_lane_slots = self
            .lane_slot_count()
            .max(Self::target_lane_slots(required_lane_slots).ok_or(ResourceScope::LaneStorage)?);
        let target_assoc_slots = self
            .assoc
            .assoc_slots()
            .max(Self::target_assoc_slots(required_assoc_slots).ok_or(ResourceScope::LaneStorage)?);
        let lane_base = self.lane_base();
        let target_slots_u32 =
            u32::try_from(target_lane_slots).map_err(|_| ResourceScope::LaneStorage)?;
        let lane_end = lane_base
            .checked_add(target_slots_u32)
            .ok_or(ResourceScope::LaneStorage)?;
        let core_growth = self.lane_slot_count() < target_lane_slots
            || self.assoc.assoc_slots() < target_assoc_slots;

        let assoc_was_bound = self.assoc.is_bound();
        let need_assoc = !assoc_was_bound || core_growth;

        if !need_assoc {
            return Ok(());
        }

        let source_assoc = self.assoc_storage.get();
        let lease = self
            .lease_sidecar::<u8>(
                AssocTable::storage_bytes(target_assoc_slots),
                AssocTable::storage_align(),
            )
            .ok_or(ResourceScope::LaneStorage)?;

        /* SAFETY: all required sidecar storage was leased before any table owner is rebound. */
        unsafe {
            if assoc_was_bound {
                self.assoc.init_replacement_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            } else {
                self.assoc.bind_from_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            }
        }
        if assoc_was_bound {
            /* SAFETY: the replacement assoc arena was staged before the owner pointer is published. */
            unsafe {
                self.assoc.commit_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            }
        }
        self.assoc_storage.set(lease);
        if assoc_was_bound {
            self.retire_sidecar(source_assoc);
        }
        self.lane_base.set(lane_base);
        self.lane_end.set(lane_end);
        Ok(())
    }

    pub(crate) fn ensure_core_lane_tables_for_assoc_entries(
        &self,
        required_lane_slots: usize,
        required_assoc_slots: usize,
    ) -> Result<(), ResourceScope> {
        self.ensure_lane_storage(required_lane_slots, required_assoc_slots)
    }

    pub(crate) fn ensure_route_table_capacity(
        &self,
        required_frame_slots: usize,
        required_lane_slots: usize,
    ) -> Result<(), ResourceScope> {
        if required_frame_slots == 0
            || (self.routes.route_slots() >= required_frame_slots
                && self.routes.lane_slots() >= required_lane_slots)
        {
            return Ok(());
        }
        let lease = self
            .lease_sidecar::<u8>(
                RouteTable::storage_bytes(required_frame_slots, required_lane_slots),
                RouteTable::storage_align(),
            )
            .ok_or(ResourceScope::RouteTable)?;
        let source_route = self.route_storage.get();
        if self.routes.route_slots() == 0 {
            /* SAFETY: route storage is currently unbound. The fresh sidecar
            lease has route-table size/alignment, and binding initializes all
            frame/lane/waiter columns before publication. */
            unsafe {
                self.routes.bind_from_storage_with_layout(
                    lease.ptr(),
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
            }
            self.route_storage.set(lease);
            return Ok(());
        }

        /* SAFETY: the fresh route-table sidecar is unpublished replacement
        storage. Migration transfers initialized frame/lane/waiter state before
        the new owner root is published and the old sidecar is retired. */
        unsafe {
            self.routes.migrate_from_storage(
                lease.ptr(),
                required_frame_slots,
                self.lane_base(),
                required_lane_slots,
            );
        }
        /* SAFETY: migration populated the replacement route-table columns before
        rebinding publishes them for the same rendezvous lane range. */
        unsafe {
            self.routes.rebind_from_storage(
                lease.ptr(),
                required_frame_slots,
                self.lane_base(),
                required_lane_slots,
            );
        }
        self.route_storage.set(lease);
        self.retire_sidecar(source_route);
        Ok(())
    }
}
