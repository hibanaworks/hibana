use super::{
    ClusterError, DynamicResolverEntry, DynamicResolverKey, RendezvousId, ResolverBucket,
    ResourceScope, SessionId, cluster_rendezvous_slot,
};
use crate::rendezvous::core::Sidecar;

const ROLE_BINDING_SLOTS: usize = crate::g::ROLE_DOMAIN_SIZE as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionRoleBinding {
    pub(crate) sid: SessionId,
    pub(crate) role: u8,
    pub(crate) rv: RendezvousId,
    refs: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct SessionRoleBindings<const MAX_RV: usize> {
    slots: [[Option<SessionRoleBinding>; ROLE_BINDING_SLOTS]; MAX_RV],
}

impl<const MAX_RV: usize> SessionRoleBindings<MAX_RV> {
    const UNBOUND_ROW: [Option<SessionRoleBinding>; ROLE_BINDING_SLOTS] =
        [None; ROLE_BINDING_SLOTS];

    pub(crate) const fn new() -> Self {
        Self {
            slots: [Self::UNBOUND_ROW; MAX_RV],
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            core::ptr::write(dst, Self::new());
        }
    }

    pub(crate) fn bind(
        &mut self,
        sid: SessionId,
        role: u8,
        rv: RendezvousId,
    ) -> Result<(), ClusterError> {
        if role >= crate::g::ROLE_DOMAIN_SIZE {
            crate::invariant();
        }

        let mut vacant_slot = None;
        let mut row = 0usize;
        while row < MAX_RV {
            let mut col = 0usize;
            while col < ROLE_BINDING_SLOTS {
                let slot = &mut self.slots[row][col];
                if let Some(binding) = slot {
                    if binding.sid == sid && binding.role == role {
                        if binding.rv != rv {
                            return Err(ClusterError::RendezvousMismatch {
                                expected: binding.rv.raw(),
                                actual: rv.raw(),
                            });
                        }
                        binding.refs =
                            binding
                                .refs
                                .checked_add(1)
                                .ok_or(ClusterError::resource_exhausted(
                                    ResourceScope::RendezvousTable,
                                ))?;
                        return Ok(());
                    }
                } else if vacant_slot.is_none() {
                    vacant_slot = Some((row, col));
                }
                col += 1;
            }
            row += 1;
        }

        let Some((row, col)) = vacant_slot else {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::RendezvousTable,
            ));
        };
        self.slots[row][col] = Some(SessionRoleBinding {
            sid,
            role,
            rv,
            refs: 1,
        });
        Ok(())
    }

    pub(crate) fn unbind(&mut self, sid: SessionId, role: u8, rv: RendezvousId) {
        let mut row = 0usize;
        while row < MAX_RV {
            let mut col = 0usize;
            while col < ROLE_BINDING_SLOTS {
                if let Some(binding) = &mut self.slots[row][col]
                    && binding.sid == sid
                    && binding.role == role
                    && binding.rv == rv
                {
                    binding.refs -= 1;
                    if binding.refs == 0 {
                        self.slots[row][col] = None;
                    }
                    return;
                }
                col += 1;
            }
            row += 1;
        }
    }
}

/// SessionCluster - owns caller-budgeted local rendezvous instances.
///
/// This is the top-level local session coordinator. It manages:
/// - Local rendezvous owners
/// - Session-role bindings for resident endpoints
/// - Dynamic route resolver storage
///
/// # Type Parameters
///
/// - `MAX_RV`: Caller-owned local rendezvous budget. It is not a protocol role,
///   node, member, or transport connection count.
///
/// Resident mutable state of SessionCluster.
///
/// # Safety Invariants
///
/// The following invariants MUST be maintained by all code accessing `SessionStorage`:
///
/// 1. **No duplicate lane leases**: At most one `LaneLease` exists per (rv_id, lane) pair
/// 2. **Lane exclusivity during lease**: While a lane is leased, only the lease guard may touch that lane's state
/// 3. **Rendezvous ownership**: Rendezvous instances are owned by the cluster and remain attached while leases exist
/// 4. **Resolver ownership**: dynamic resolvers are registered only for resident program sites
///
/// Violations of these invariants are guarded by the lease table where possible
/// and audited through TAP events and focused invariant tests.
pub(crate) struct SessionStorage<'cfg, T, const MAX_RV: usize>
where
    T: crate::transport::Transport,
{
    /// Owned local rendezvous instances.
    pub(crate) locals: crate::session::lease::core::RendezvousTable<'cfg, T, MAX_RV>,

    /// Attached session-role bindings.
    pub(crate) role_bindings: SessionRoleBindings<MAX_RV>,

    /// Number of active lane leases (affine witness count).
    pub(crate) active_leases: core::cell::Cell<u32>,
}

impl<'cfg, T, const MAX_RV: usize> SessionStorage<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            crate::session::lease::core::RendezvousTable::init_empty(core::ptr::addr_of_mut!(
                (*dst).locals
            ));
            SessionRoleBindings::init_empty(core::ptr::addr_of_mut!((*dst).role_bindings));
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
        mut free: FF,
    ) -> Result<(), ClusterError>
    where
        FA: FnOnce(usize, usize) -> Option<Sidecar<u8>>,
        FF: FnMut(Sidecar<u8>) -> Result<(), ResourceScope>,
    {
        if additional_entries == 0 {
            return Ok(());
        }
        let bucket = self
            .bucket_mut(rv_id)
            .ok_or(ClusterError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })?;
        let required = bucket.entry_count().checked_add(additional_entries).ok_or(
            ClusterError::resource_exhausted(ResourceScope::ResolverTable),
        )?;
        if bucket.capacity() >= required {
            return Ok(());
        }

        let source_storage = bucket.storage_sidecar();
        let storage = allocate(
            ResolverBucket::storage_bytes(required),
            ResolverBucket::storage_align(),
        )
        .ok_or(ClusterError::resource_exhausted(
            ResourceScope::ResolverTable,
        ))?;
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe {
            if source_storage.ptr().is_null() {
                bucket.bind_from_storage(storage.cast(), required);
            } else {
                bucket.init_replacement_storage(storage.cast(), required);
                if let Err(resource) = free(source_storage.cast()) {
                    if free(storage).is_err() {
                        crate::invariant();
                    }
                    return Err(ClusterError::resource_exhausted(resource));
                }
                bucket.commit_storage(storage.cast(), required);
            }
        }
        Ok(())
    }

    pub(crate) fn insert(
        &mut self,
        key: DynamicResolverKey,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), ClusterError> {
        self.bucket_mut(key.rv)
            .ok_or(ClusterError::RendezvousMismatch {
                expected: key.rv.raw(),
                actual: 0,
            })?
            .insert(key.eff_index, entry)
    }

    pub(crate) fn get(&self, key: DynamicResolverKey) -> Option<&DynamicResolverEntry<'cfg>> {
        self.bucket(key.rv)?.get(key.eff_index)
    }
}
