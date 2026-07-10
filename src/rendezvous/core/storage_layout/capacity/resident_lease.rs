use super::{Rendezvous, Sidecar, Transport};

pub(super) const fn sidecar_ranges_overlap(
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    left_start < right_end && left_end > right_start
}

pub(in crate::rendezvous::core) struct PersistentSidecarLease {
    storage: Sidecar<u8>,
    image_frontier: u32,
}

impl PersistentSidecarLease {
    #[inline]
    pub(super) const fn storage(&self) -> Sidecar<u8> {
        self.storage
    }
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(super) fn plan_persistent_sidecar_bytes(
        &self,
        bytes: usize,
        align: usize,
    ) -> Option<PersistentSidecarLease> {
        if bytes == 0 {
            crate::invariant();
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let mut start = Self::align_up(base, align).checked_sub(base)?;
        loop {
            let end = start.checked_add(bytes)?;
            let mut conflict_end: Option<usize> = None;
            for resident in self.live_sidecars() {
                let Some((live_start, live_end)) = self.sidecar_range(resident.storage) else {
                    continue;
                };
                if sidecar_ranges_overlap(start, end, live_start, live_end) {
                    conflict_end = Some(match conflict_end {
                        Some(seen) => seen.max(live_end),
                        None => live_end,
                    });
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
            let ptr = /* SAFETY: `start..end` is disjoint from every live
            sidecar and `reserved_end` stays below endpoint storage. */ unsafe {
                slab_ptr.add(start)
            };
            return Some(PersistentSidecarLease {
                storage: Sidecar::from_raw_parts(ptr, bytes),
                image_frontier: self
                    .image_frontier
                    .get()
                    .max(crate::invariant_ok(u32::try_from(end))),
            });
        }
    }

    #[inline]
    pub(super) fn commit_persistent_sidecar_lease(
        &self,
        lease: PersistentSidecarLease,
    ) -> Sidecar<u8> {
        self.set_image_frontier(lease.image_frontier);
        lease.storage
    }

    #[inline]
    pub(super) fn allocate_persistent_sidecar_bytes(
        &self,
        bytes: usize,
        align: usize,
    ) -> Option<Sidecar<u8>> {
        let lease = self.plan_persistent_sidecar_bytes(bytes, align)?;
        Some(self.commit_persistent_sidecar_lease(lease))
    }
}
