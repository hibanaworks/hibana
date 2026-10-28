use super::{Rendezvous, ResidentSidecarKind, Sidecar, Transport};

pub(in crate::rendezvous::core) fn packed_sidecar_range(
    base: usize,
    frontier: usize,
    bytes: usize,
    align: usize,
) -> Option<(usize, usize)> {
    let start = crate::runtime_core::layout::checked_align_offset(base, frontier, align)?;
    let end = start.checked_add(bytes)?;
    Some((start, end))
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    fn sort_resident_sidecars(&self, sidecars: &mut [super::ResidentSidecar; 3]) {
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
        sidecars: [super::ResidentSidecar; 3],
    ) -> Option<usize> {
        self.sidecar_packing_order(sidecars).map(|(_, end)| end)
    }

    /// Choose the smallest packing of the three owner classes. Allocation
    /// replacement must not change persistent capacity through alignment alone.
    fn sidecar_packing_order(
        &self,
        sidecars: [super::ResidentSidecar; 3],
    ) -> Option<([super::ResidentSidecar; 3], usize)> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let mut best = None;
        for first in 0..sidecars.len() {
            for second in 0..sidecars.len() {
                if second == first {
                    continue;
                }
                for third in 0..sidecars.len() {
                    if third == first || third == second {
                        continue;
                    }
                    let order = [sidecars[first], sidecars[second], sidecars[third]];
                    let mut frontier = Some(0);
                    for resident in order {
                        if !resident.storage.is_empty() {
                            frontier = frontier.and_then(|offset| {
                                packed_sidecar_range(
                                    base,
                                    offset,
                                    resident.storage.bytes(),
                                    resident.align,
                                )
                                .map(|(_, end)| end)
                            });
                        }
                    }
                    if let Some(end) = frontier
                        && best.as_ref().is_none_or(|(_, previous)| end < *previous)
                    {
                        best = Some((order, end));
                    }
                }
            }
        }
        best
    }

    pub(super) fn compact_live_sidecars(&self) {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let mut sidecars = self.live_sidecars();
        let (wanted, optimal_end) = crate::invariant_some(self.sidecar_packing_order(sidecars));
        self.sort_resident_sidecars(&mut sidecars);
        // First pack opaque bytes without padding, preserving physical order.
        // Typed roots are rebound only after every block is aligned again.
        let mut frontier = 0usize;
        let mut active = 0;
        for resident in &mut sidecars {
            let Some((source_start, _)) = self.sidecar_range(resident.storage) else {
                continue;
            };
            if frontier > source_start {
                crate::invariant();
            }
            let destination = unsafe { slab_ptr.add(frontier) };
            // SAFETY: left packing preserves source order and cannot overwrite
            // a later source. Copying opaque bytes also preserves padding.
            unsafe {
                core::ptr::copy(
                    resident.storage.ptr(),
                    destination,
                    resident.storage.bytes(),
                );
            }
            resident.storage = Sidecar::from_raw_parts(destination, resident.storage.bytes());
            frontier = crate::runtime_core::layout::add(frontier, resident.storage.bytes());
            active += 1;
        }
        let mut target = 0;
        for desired in wanted {
            if desired.storage.is_empty() {
                continue;
            }
            let selected = crate::invariant_some(
                (target..active).find(|&index| sidecars[index].kind == desired.kind),
            );
            if selected != target {
                let (start, _) =
                    crate::invariant_some(self.sidecar_range(sidecars[target].storage));
                let (_, end) =
                    crate::invariant_some(self.sidecar_range(sidecars[selected].storage));
                let moved = sidecars[selected].storage.bytes();
                // SAFETY: initialized blocks and their padding are moved as
                // MaybeUninit bytes, with no interpretation of pointer bytes or
                // callback. The contiguous span stays inside its old extent.
                unsafe {
                    core::slice::from_raw_parts_mut(
                        slab_ptr.add(start).cast::<core::mem::MaybeUninit<u8>>(),
                        end - start,
                    )
                    .rotate_right(moved);
                }
                sidecars[target..=selected].rotate_right(1);
                let mut offset = start;
                for resident in &mut sidecars[target..=selected] {
                    resident.storage = Sidecar::from_raw_parts(
                        unsafe { slab_ptr.add(offset) },
                        resident.storage.bytes(),
                    );
                    offset = crate::runtime_core::layout::add(offset, resident.storage.bytes());
                }
            }
            target += 1;
        }
        let mut destinations = [0; 3];
        frontier = 0;
        for (index, resident) in sidecars[..active].iter().enumerate() {
            let (start, end) = crate::invariant_some(packed_sidecar_range(
                base,
                frontier,
                resident.storage.bytes(),
                resident.align,
            ));
            destinations[index] = start;
            frontier = end;
        }
        if frontier != optimal_end || frontier > self.image_frontier.get() as usize {
            crate::invariant();
        }
        // Insert alignment gaps backwards so an expanding destination cannot
        // overwrite a not-yet-moved source. No heap or size-dependent temporary.
        for index in (0..active).rev() {
            let resident = &mut sidecars[index];
            let destination = unsafe { slab_ptr.add(destinations[index]) };
            unsafe {
                core::ptr::copy(
                    resident.storage.ptr(),
                    destination,
                    resident.storage.bytes(),
                );
            }
            resident.storage = Sidecar::from_raw_parts(destination, resident.storage.bytes());
        }
        for resident in sidecars {
            if resident.storage.is_empty() {
                continue;
            }
            let compacted = resident.storage;
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
                ResidentSidecarKind::Resolvers => {
                    /* SAFETY: resolver entries are moved intact and capacity is
                    unchanged; this only replaces the bucket's sidecar root. */
                    unsafe {
                        (&mut *self.resolver_bucket.get()).relocate_storage(compacted);
                    }
                }
            }
        }

        if frontier > self.endpoint_storage_floor() {
            crate::invariant();
        }
        self.set_image_frontier(crate::invariant_ok(u32::try_from(frontier)));
    }
}
