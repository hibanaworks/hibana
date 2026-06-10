mod begin_capacity;

use super::{
    CpError, DistributedTopologyState, DynamicResolverEntry, DynamicResolverKey, RendezvousId,
    ResolverBucket, ResourceScope, cluster_rendezvous_slot,
};

/// SessionCluster - Coordinates multiple Rendezvous instances.
///
/// This is the top-level local control-plane coordinator. It manages:
/// - Local Rendezvous instances
/// - Distributed topology coordination across registered local rendezvous
/// - Intent/Ack routing
///
/// # Type Parameters
///
/// - `MAX_RV`: Maximum number of Rendezvous instances
///
/// Internal mutable state of SessionCluster.
///
/// # Safety Invariants
///
/// The following invariants MUST be maintained by all code accessing `ControlCore`:
///
/// 1. **No duplicate lane leases**: At most one `LaneLease` exists per (rv_id, lane) pair
/// 2. **Lane exclusivity during lease**: While a lane is leased, only the lease guard may touch that lane's state
/// 3. **Rendezvous ownership**: Rendezvous instances are owned by the cluster and must not be removed while leases exist
/// 4. **Topology state consistency**: distributed topology operations must maintain Begin→Ack→Commit ordering
///
/// Violations of these invariants are guarded by the lease table where possible
/// and audited through TAP events and focused invariant tests.
pub(crate) struct ControlCore<'cfg, T, U, C, E, const MAX_RV: usize>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Owned local Rendezvous instances (same process/node).
    pub(crate) locals: crate::control::lease::core::ControlCore<'cfg, T, U, C, E, MAX_RV>,

    /// Distributed topology state tracking.
    pub(crate) topology_state: DistributedTopologyState<MAX_RV>,

    /// Number of active lane leases (affine witness count).
    pub(crate) active_leases: core::cell::Cell<u32>,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            crate::control::lease::core::ControlCore::init_empty(core::ptr::addr_of_mut!(
                (*dst).locals
            ));
            DistributedTopologyState::init_empty(core::ptr::addr_of_mut!((*dst).topology_state));
            core::ptr::addr_of_mut!((*dst).active_leases).write(core::cell::Cell::new(0));
        }
    }
}

pub(crate) struct ResolverCore<'cfg, const MAX_RV: usize> {
    buckets: [ResolverBucket<'cfg>; MAX_RV],
}

impl<'cfg, const MAX_RV: usize> ResolverCore<'cfg, MAX_RV> {
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            let mut slot = 0usize;
            while slot < MAX_RV {
                ResolverBucket::init_empty(core::ptr::addr_of_mut!((*dst).buckets[slot]));
                slot += 1;
            }
        }
    }

    pub(crate) fn bucket(&self, rv_id: RendezvousId) -> Option<&ResolverBucket<'cfg>> {
        let slot = cluster_rendezvous_slot::<MAX_RV>(rv_id)?;
        Some(&self.buckets[slot])
    }

    fn bucket_mut(&mut self, rv_id: RendezvousId) -> Option<&mut ResolverBucket<'cfg>> {
        let slot = cluster_rendezvous_slot::<MAX_RV>(rv_id)?;
        Some(&mut self.buckets[slot])
    }

    pub(crate) fn ensure_capacity<FA, FF>(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
        allocate: FA,
        free: FF,
    ) -> Result<(), CpError>
    where
        FA: FnOnce(usize, usize) -> Option<(*mut u8, usize)>,
        FF: FnOnce(*mut u8, usize, usize),
    {
        if additional_entries == 0 {
            return Ok(());
        }
        let bucket = self.bucket_mut(rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let required = bucket
            .occupied_len()
            .checked_add(additional_entries)
            .ok_or(CpError::resource_exhausted(ResourceScope::ResolverTable))?;
        if bucket.capacity() >= required {
            return Ok(());
        }

        let old_ptr = bucket.storage_ptr();
        let old_len = bucket.storage_len();
        let old_reclaim_delta = bucket.storage_reclaim_delta();
        let (storage, reclaim_delta) = allocate(
            ResolverBucket::storage_bytes(required),
            ResolverBucket::storage_align(),
        )
        .ok_or(CpError::resource_exhausted(ResourceScope::ResolverTable))?;
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe {
            if old_ptr.is_null() {
                bucket.bind_from_storage(storage, required, reclaim_delta);
            } else {
                bucket.rebind_from_storage(storage, required, reclaim_delta);
                free(old_ptr, old_len, old_reclaim_delta);
            }
        }
        Ok(())
    }

    pub(crate) fn insert(
        &mut self,
        key: DynamicResolverKey,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), CpError> {
        self.bucket_mut(key.rv)
            .ok_or(CpError::RendezvousMismatch {
                expected: key.rv.raw(),
                actual: 0,
            })?
            .insert(key.eff_index, key.subject, entry)
    }

    pub(crate) fn get(&self, key: DynamicResolverKey) -> Option<&DynamicResolverEntry<'cfg>> {
        self.bucket(key.rv)?.get(key.eff_index, key.subject)
    }
}
