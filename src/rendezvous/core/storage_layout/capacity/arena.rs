use super::{Rendezvous, ResidentSidecarKind, Sidecar, Transport};

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    pub(super) fn compact_live_sidecars(&self) {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let mut sidecars = self.live_sidecars();
        let mut idx = 1usize;
        while idx < sidecars.len() {
            let candidate = sidecars[idx];
            let candidate_start = self
                .sidecar_range(candidate.storage)
                .map_or(usize::MAX, |range| range.0);
            let mut insert = idx;
            while insert != 0 {
                let previous_start = self
                    .sidecar_range(sidecars[insert - 1].storage)
                    .map_or(usize::MAX, |range| range.0);
                if previous_start <= candidate_start {
                    break;
                }
                sidecars[insert] = sidecars[insert - 1];
                insert -= 1;
            }
            sidecars[insert] = candidate;
            idx += 1;
        }

        let mut frontier = 0usize;
        for resident in sidecars {
            let Some((source_start, source_end)) = self.sidecar_range(resident.storage) else {
                continue;
            };
            let destination_start = crate::invariant_some(
                Self::align_up(
                    crate::invariant_some(base.checked_add(frontier)),
                    resident.align,
                )
                .checked_sub(base),
            );
            if destination_start > source_start {
                crate::invariant();
            }
            let destination_end =
                crate::invariant_some(destination_start.checked_add(resident.storage.bytes()));
            if destination_end > source_end {
                crate::invariant();
            }
            let destination = /* SAFETY: canonical packing preserves physical
            sidecar order, so each destination ends no later than its source and
            cannot overwrite a later live source. `ptr::copy` handles overlap
            and transfers non-Copy waiter ownership without running callbacks. */ unsafe {
                slab_ptr.add(destination_start)
            };
            if destination_start != source_start {
                /* SAFETY: source/destination are within the same resident slab;
                the owner roots are rebound below and the old bytes are never
                observed or dropped again. */
                unsafe {
                    core::ptr::copy(
                        resident.storage.ptr(),
                        destination,
                        resident.storage.bytes(),
                    );
                }
            }
            let compacted = Sidecar::from_raw_parts(destination, resident.storage.bytes());
            match resident.kind {
                ResidentSidecarKind::EndpointLeases => {
                    self.endpoint_lease_storage.set(compacted.cast());
                }
                ResidentSidecarKind::Associations => {
                    /* SAFETY: bytes were moved intact with the same alignment;
                    rebinding only recomputes the assoc column roots. */
                    unsafe {
                        self.assoc.relocate_storage(compacted.ptr());
                    }
                    self.assoc_storage.set(compacted);
                }
                ResidentSidecarKind::Routes => {
                    /* SAFETY: bytes were moved intact with the current route
                    shape; rebinding publishes the relocated column roots. */
                    unsafe {
                        self.routes.relocate_storage(compacted.ptr());
                    }
                    self.route_storage.set(compacted);
                }
                ResidentSidecarKind::Resolvers => {
                    /* SAFETY: resolver entries are moved intact and capacity is
                    unchanged; this only replaces the bucket's sidecar root. */
                    unsafe {
                        (&mut *self.resolver_bucket.get()).relocate_storage(compacted);
                    }
                }
            }
            frontier = destination_end;
        }

        let reserved_end = crate::invariant_some(
            frontier.checked_add(self.frontier_workspace_bytes.get() as usize),
        );
        if reserved_end > self.endpoint_storage_floor() {
            crate::invariant();
        }
        self.set_image_frontier(crate::invariant_ok(u32::try_from(frontier)));
    }
}
