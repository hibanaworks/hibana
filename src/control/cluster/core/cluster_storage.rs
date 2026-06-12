mod begin_capacity;

use super::{
    CpError, DistributedTopologyState, DynamicResolverEntry, DynamicResolverKey, RendezvousId,
    ResolverBucket, ResourceScope, SessionId, TopologyError, cluster_rendezvous_slot,
};

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
    const EMPTY_ROW: [Option<SessionRoleBinding>; ROLE_BINDING_SLOTS] = [None; ROLE_BINDING_SLOTS];

    pub(crate) const fn new() -> Self {
        Self {
            slots: [Self::EMPTY_ROW; MAX_RV],
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
    ) -> Result<(), CpError> {
        if role >= crate::g::ROLE_DOMAIN_SIZE {
            return Err(CpError::Topology(TopologyError::InvalidState));
        }

        let mut empty = None;
        let mut row = 0usize;
        while row < MAX_RV {
            let mut col = 0usize;
            while col < ROLE_BINDING_SLOTS {
                match &mut self.slots[row][col] {
                    Some(binding) if binding.sid == sid && binding.role == role => {
                        if binding.rv != rv {
                            return Err(CpError::Topology(TopologyError::InvalidState));
                        }
                        binding.refs = binding
                            .refs
                            .checked_add(1)
                            .ok_or(CpError::resource_exhausted(ResourceScope::TopologyTable))?;
                        return Ok(());
                    }
                    None if empty.is_none() => {
                        empty = Some((row, col));
                    }
                    _ => {}
                }
                col += 1;
            }
            row += 1;
        }

        let Some((row, col)) = empty else {
            return Err(CpError::resource_exhausted(ResourceScope::TopologyTable));
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

    pub(crate) fn resolve(&self, sid: SessionId, role: u8) -> Option<SessionRoleBinding> {
        let mut row = 0usize;
        while row < MAX_RV {
            let mut col = 0usize;
            while col < ROLE_BINDING_SLOTS {
                if let Some(binding) = self.slots[row][col]
                    && binding.sid == sid
                    && binding.role == role
                {
                    return Some(binding);
                }
                col += 1;
            }
            row += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_role_bindings_refcount_unbinds_exact_session_role_rv() {
        let sid = SessionId::new(7);
        let rv = RendezvousId::new(1);
        let mut bindings = SessionRoleBindings::<1>::new();

        bindings.bind(sid, 3, rv).expect("first bind");
        bindings
            .bind(sid, 3, rv)
            .expect("second bind increments refs");
        assert_eq!(bindings.resolve(sid, 3).expect("binding").rv, rv);

        bindings.unbind(sid, 3, rv);
        assert_eq!(
            bindings
                .resolve(sid, 3)
                .expect("binding survives one unbind")
                .rv,
            rv
        );
        bindings.unbind(sid, 3, rv);
        assert!(
            bindings.resolve(sid, 3).is_none(),
            "last unbind must release the slot"
        );
        bindings.unbind(sid, 3, rv);
        assert!(
            bindings.resolve(sid, 3).is_none(),
            "extra unbind must not resurrect or corrupt a released binding"
        );
    }

    #[test]
    fn session_role_bindings_reject_conflicting_rendezvous_for_same_session_role() {
        let sid = SessionId::new(8);
        let rv1 = RendezvousId::new(1);
        let rv2 = RendezvousId::new(2);
        let mut bindings = SessionRoleBindings::<2>::new();

        bindings.bind(sid, 4, rv1).expect("initial bind");
        assert!(matches!(
            bindings.bind(sid, 4, rv2),
            Err(CpError::Topology(TopologyError::InvalidState))
        ));
        assert_eq!(bindings.resolve(sid, 4).expect("binding").rv, rv1);
    }

    #[test]
    fn session_role_bindings_capacity_fails_closed_without_fallback() {
        let mut bindings = SessionRoleBindings::<1>::new();
        for role in 0..crate::g::ROLE_DOMAIN_SIZE {
            bindings
                .bind(SessionId::new(role as u32 + 1), role, RendezvousId::new(1))
                .expect("capacity slot");
        }

        assert!(matches!(
            bindings.bind(
                SessionId::new(crate::g::ROLE_DOMAIN_SIZE as u32 + 1),
                0,
                RendezvousId::new(1)
            ),
            Err(CpError::ResourceExhausted {
                resource: ResourceScope::TopologyTable
            })
        ));
    }
}

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

    /// Attached session-role bindings used by choreography-authored topology controls.
    pub(crate) role_bindings: SessionRoleBindings<MAX_RV>,

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
