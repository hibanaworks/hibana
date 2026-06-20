use core::{ptr, ptr::NonNull};

use super::{EndpointLeaseId, EndpointResidentBudget, RendezvousEntry, RendezvousTable};
use crate::{
    rendezvous::core::Rendezvous,
    session::{
        cluster::error::ClusterError,
        types::{RendezvousId, SessionId},
    },
    transport::Transport,
};

impl<'cfg, T> RendezvousTable<'cfg, T>
where
    T: Transport,
{
    fn contains_key(&self, key: &RendezvousId) -> bool {
        self.entry_ref(key).is_some()
    }

    fn next_available_rendezvous_id(&self) -> Option<RendezvousId> {
        let mut raw = 1u16;
        while raw != 0 {
            let id = RendezvousId::new(raw);
            if !self.contains_key(&id) {
                return Some(id);
            }
            raw = raw.wrapping_add(1);
        }
        None
    }

    pub(crate) fn register_local_from_resources_auto(
        &mut self,
        resources: crate::runtime_core::resources::RuntimeResources<'cfg>,
        transport: T,
    ) -> Result<RendezvousId, RegisterRendezvousError> {
        let id = self
            .next_available_rendezvous_id()
            .ok_or(RegisterRendezvousError::CapacityExceeded)?;
        let rendezvous = /* SAFETY: the returned pointer is backed by the caller slab for 'cfg. */ unsafe {
            Rendezvous::init_in_slab_auto(id, resources, transport)
                .ok_or(RegisterRendezvousError::StorageExhausted)?
        };
        let entry_ptr = /* SAFETY: rendezvous has just been initialized and is not linked into the table yet. */ unsafe {
            let rv = &mut *rendezvous;
            match rv.allocate_external_persistent_sidecar_bytes(
                core::mem::size_of::<RendezvousEntry<'cfg, T>>(),
                core::mem::align_of::<RendezvousEntry<'cfg, T>>(),
            ) {
                Some(sidecar) => sidecar.ptr().cast::<RendezvousEntry<'cfg, T>>(),
                None => {
                    ptr::drop_in_place(rendezvous);
                    return Err(RegisterRendezvousError::StorageExhausted);
                }
            }
        };
        /* SAFETY: `rendezvous` is the non-null, slab-pinned pointer returned by
         * `Rendezvous::init_in_slab_auto`; `entry_ptr` was allocated from the same
         * rendezvous persistent sidecar with `RendezvousEntry` size/align and is
         * not published in the registry; `self.head` is the existing initialized
         * list head, and the entry is published to `self.head` only after initialization.
         */
        unsafe {
            RendezvousEntry::init_from_parts(
                entry_ptr,
                id,
                NonNull::new_unchecked(rendezvous),
                self.head,
            );
        }
        self.head = NonNull::new(entry_ptr);
        self.len = self
            .len
            .checked_add(1)
            .ok_or(RegisterRendezvousError::CapacityExceeded)?;
        Ok(id)
    }

    pub(crate) fn allocate_endpoint_lease_for_session_role(
        &mut self,
        rv: RendezvousId,
        sid: SessionId,
        role: u8,
        bytes: usize,
        align: usize,
        resident_budget: EndpointResidentBudget,
    ) -> Result<(EndpointLeaseId, u32, usize, usize), ClusterError> {
        if role >= crate::g::ROLE_DOMAIN_SIZE {
            crate::invariant();
        }

        let mut target = None;
        let mut current = self.head;
        while let Some(mut entry_ptr) = current {
            let entry = /* SAFETY: registry links are initialized slab nodes and remain pinned until table drop. */ unsafe {
                entry_ptr.as_mut()
            };
            if entry.id == rv {
                target = Some(entry_ptr);
            }
            if entry.is_active() {
                return Err(ClusterError::RendezvousBusy { id: entry.id.raw() });
            }
            if crate::invariant_some(entry.rendezvous_ref())
                .has_live_endpoint_session_role(sid, role)
            {
                if entry.id != rv {
                    return Err(ClusterError::RendezvousMismatch {
                        expected: entry.id.raw(),
                        actual: rv.raw(),
                    });
                }
                return Err(ClusterError::RendezvousBusy { id: rv.raw() });
            }
            current = entry.next;
        }

        let Some(mut target) = target else {
            return Err(ClusterError::RendezvousUnregistered { id: rv.raw() });
        };
        let entry = /* SAFETY: target was discovered in the initialized registry list above. */ unsafe {
            target.as_mut()
        };
        let Some(rendezvous) = entry.rendezvous_mut() else {
            return Err(ClusterError::RendezvousBusy { id: rv.raw() });
        };
        /* SAFETY: duplicate session-role ownership has been rejected by scanning
        every registered rendezvous entry above; the selected rendezvous now owns
        the endpoint lease slot and writes the live `(sid, role)` identity atomically
        with the endpoint storage reservation. */
        unsafe { rendezvous.allocate_endpoint_lease(sid, role, bytes, align, resident_budget) }
            .map_err(ClusterError::resource_exhausted)
    }

    pub(crate) fn ensure_dynamic_resolver_capacity(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        if additional_entries == 0 {
            return Ok(());
        }
        let entry = self.entry_mut(&rv_id).ok_or(
            crate::session::cluster::error::ClusterError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            },
        )?;
        let rv = entry.rendezvous_mut().ok_or(
            crate::session::cluster::error::ClusterError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            },
        )?;
        let rv_ptr = core::ptr::from_mut(rv);
        entry.resolver_bucket.ensure_capacity(
            additional_entries,
            |bytes, align| {
                /* SAFETY: rv_ptr comes from the selected registry entry and remains pinned for the duration of this capacity update. */
                unsafe { (&mut *rv_ptr).allocate_external_persistent_sidecar_bytes(bytes, align) }
            },
            |sidecar| {
                /* SAFETY: rv_ptr comes from the selected registry entry and remains pinned for the duration of this capacity update. */
                unsafe { (&mut *rv_ptr).release_external_persistent_sidecar(sidecar) }
            },
        )
    }

    pub(crate) fn insert_dynamic_resolver(
        &mut self,
        key: crate::session::cluster::core::DynamicResolverKey,
        entry: crate::session::cluster::core::DynamicResolverEntry<'cfg>,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        self.entry_mut(&key.rv)
            .ok_or(
                crate::session::cluster::error::ClusterError::RendezvousMismatch {
                    expected: key.rv.raw(),
                    actual: 0,
                },
            )?
            .resolver_bucket
            .insert(key.scope, entry)
    }

    pub(crate) fn dynamic_resolver(
        &self,
        key: crate::session::cluster::core::DynamicResolverKey,
    ) -> Option<&crate::session::cluster::core::DynamicResolverEntry<'cfg>> {
        self.entry_ref(&key.rv)?.resolver_bucket.get(key.scope)
    }
}

impl<'cfg, T> Drop for RendezvousTable<'cfg, T>
where
    T: Transport,
{
    fn drop(&mut self) {
        let mut current = self.head;
        while let Some(entry_ptr) = current {
            let entry = entry_ptr.as_ptr();
            current = /* SAFETY: registry links are initialized slab nodes and remain pinned until table drop. */ unsafe {
                (*entry).next
            };
            /* SAFETY: each entry is visited once through the intrusive list and owns its rendezvous header. */
            unsafe {
                ptr::drop_in_place(entry);
            }
        }
    }
}

/// Failure modes for rendezvous registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegisterRendezvousError {
    /// Attempted to register more rendezvous than the identifier space allows.
    CapacityExceeded,
    /// Caller-provided slab cannot fit the rendezvous resident header or registry node.
    StorageExhausted,
}
