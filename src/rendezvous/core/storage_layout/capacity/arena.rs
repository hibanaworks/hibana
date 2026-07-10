use super::{Rendezvous, ResidentSidecarKind, Sidecar, Transport};

pub(in crate::rendezvous::core) fn packed_sidecar_range(
    base: usize,
    frontier: usize,
    bytes: usize,
    align: usize,
) -> Option<(usize, usize)> {
    if !align.is_power_of_two() {
        crate::invariant();
    }
    let mask = align - 1;
    let absolute = base.checked_add(frontier)?.checked_add(mask)? & !mask;
    let start = absolute.checked_sub(base)?;
    let end = start.checked_add(bytes)?;
    Some((start, end))
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    fn sort_resident_sidecars(&self, sidecars: &mut [super::ResidentSidecar; 4]) {
        let mut idx = 1usize;
        while idx < sidecars.len() {
            let candidate = sidecars[idx];
            let mut insert = idx;
            while insert != 0 {
                let previous = sidecars[insert - 1];
                let ordered = match (
                    self.sidecar_range(previous.storage),
                    self.sidecar_range(candidate.storage),
                ) {
                    (Some((previous_start, _)), Some((candidate_start, _))) => {
                        previous_start <= candidate_start
                    }
                    (Some(_), None) | (None, None) => true,
                    (None, Some(_)) => false,
                };
                if ordered {
                    break;
                }
                sidecars[insert] = previous;
                insert -= 1;
            }
            sidecars[insert] = candidate;
            idx += 1;
        }
    }

    pub(super) fn packed_sidecar_frontier(
        &self,
        mut sidecars: [super::ResidentSidecar; 4],
    ) -> Option<usize> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        self.sort_resident_sidecars(&mut sidecars);
        let mut frontier = 0usize;
        for resident in sidecars {
            if resident.storage.is_empty() {
                continue;
            }
            let (_, destination_end) =
                packed_sidecar_range(base, frontier, resident.storage.bytes(), resident.align)?;
            frontier = destination_end;
        }
        Some(frontier)
    }

    pub(super) fn compact_live_sidecars(&self) {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let mut sidecars = self.live_sidecars();
        self.sort_resident_sidecars(&mut sidecars);

        let mut frontier = 0usize;
        for resident in sidecars {
            let Some((source_start, source_end)) = self.sidecar_range(resident.storage) else {
                continue;
            };
            let (destination_start, destination_end) = crate::invariant_some(packed_sidecar_range(
                base,
                frontier,
                resident.storage.bytes(),
                resident.align,
            ));
            if destination_start > source_start {
                crate::invariant();
            }
            if destination_end > source_end {
                crate::invariant();
            }
            let destination = /* SAFETY: canonical packing preserves physical
            sidecar order, so each destination ends no later than its source and
            cannot overwrite a later live source. `ptr::copy` handles overlap;
            no owner callback runs before every typed root is rebound. */ unsafe {
                slab_ptr.add(destination_start)
            };
            if destination_start != source_start {
                /* SAFETY: source/destination are within the same resident slab;
                the owner roots are rebound below and the source bytes are never
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
