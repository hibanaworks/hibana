use core::{ptr, ptr::NonNull};

use super::{EndpointLeaseId, EndpointResidentBudget, RendezvousTable};
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
        self.node_ref(key).is_some()
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
        &self,
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
        let rendezvous_ptr = /* SAFETY: `init_in_slab_auto` returned a non-null
        initialized header pinned in the caller's slab. */ unsafe {
            NonNull::new_unchecked(rendezvous)
        };
        let rendezvous_ref = /* SAFETY: the new rendezvous is still unpublished
        and remains owned by this registry insertion. */ unsafe { rendezvous_ptr.as_ref() };
        rendezvous_ref.link_registry_next(self.head.get());
        self.head.set(Some(rendezvous_ptr));
        Ok(id)
    }

    pub(crate) fn allocate_endpoint_lease_for_session_role(
        &self,
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
        let mut current = self.head.get();
        while let Some(rendezvous_ptr) = current {
            let rendezvous = /* SAFETY: registry links are initialized rendezvous
            headers and remain pinned until table drop. */ unsafe {
                rendezvous_ptr.as_ref()
            };
            if rendezvous.registry_id() == rv {
                target = Some(rendezvous_ptr);
            }
            if rendezvous.access_is_busy() {
                return Err(ClusterError::RendezvousBusy {
                    id: rendezvous.registry_id().raw(),
                });
            }
            if rendezvous.has_endpoint_session_role(sid, role) {
                if rendezvous.registry_id() != rv {
                    return Err(ClusterError::RendezvousMismatch {
                        expected: rendezvous.registry_id().raw(),
                        actual: rv.raw(),
                    });
                }
                return Err(ClusterError::RendezvousBusy { id: rv.raw() });
            }
            current = rendezvous.registry_next();
        }

        let Some(target) = target else {
            return Err(ClusterError::RendezvousUnregistered { id: rv.raw() });
        };
        let rendezvous = /* SAFETY: target was discovered in the initialized registry list above. */ unsafe {
            target.as_ref()
        };
        rendezvous
            .allocate_endpoint_lease(sid, role, bytes, align, resident_budget)
            .map_err(ClusterError::resource_exhausted)
    }

    /// Resolve the rendezvous authority carried by one published endpoint.
    ///
    /// Unlike registry access, endpoint-owned cleanup must remain available
    /// while a transport callback runs under an attach lease. The live
    /// slot/generation pair is the authority for bypassing the registry-busy
    /// rejection; stale or reserved handles remain fail closed.
    pub(crate) fn published_endpoint_owner(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        let rendezvous = self.node_ref(&rv_id)?;
        rendezvous.endpoint_lease_storage(slot, generation)?;
        Some(rendezvous)
    }

    pub(crate) fn ensure_dynamic_resolver_capacity(
        &self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        if additional_entries == 0 {
            return Ok(());
        }
        let rendezvous = self.node_ref(&rv_id).ok_or(
            crate::session::cluster::error::ClusterError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            },
        )?;
        if rendezvous.access_is_busy() {
            return Err(
                crate::session::cluster::error::ClusterError::RendezvousBusy { id: rv_id.raw() },
            );
        }
        rendezvous.ensure_dynamic_resolver_capacity(additional_entries)
    }

    pub(crate) fn insert_dynamic_resolver(
        &self,
        key: crate::session::cluster::core::DynamicResolverKey,
        entry: crate::session::cluster::core::DynamicResolverEntry<'cfg>,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        let rv = key.rendezvous();
        let rendezvous = self.node_ref(&rv).ok_or(
            crate::session::cluster::error::ClusterError::RendezvousMismatch {
                expected: rv.raw(),
                actual: 0,
            },
        )?;
        if rendezvous.access_is_busy() {
            return Err(
                crate::session::cluster::error::ClusterError::RendezvousBusy { id: rv.raw() },
            );
        }
        rendezvous.insert_dynamic_resolver(key.scope(), entry)
    }

    pub(crate) fn dynamic_resolver(
        &self,
        key: crate::session::cluster::core::DynamicResolverKey,
    ) -> Option<crate::session::cluster::core::DynamicResolverEntry<'cfg>> {
        self.node_ref(&key.rendezvous())?
            .dynamic_resolver(key.scope())
    }
}

impl<'cfg, T> Drop for RendezvousTable<'cfg, T>
where
    T: Transport,
{
    fn drop(&mut self) {
        let mut current = self.head.get();
        while let Some(rendezvous_ptr) = current {
            let rendezvous = rendezvous_ptr.as_ptr();
            current = /* SAFETY: registry links are initialized rendezvous headers and remain pinned until table drop. */ unsafe {
                (*rendezvous).registry_next()
            };
            /* SAFETY: each intrusive rendezvous header is visited and dropped exactly once. */
            unsafe {
                ptr::drop_in_place(rendezvous);
            }
        }
    }
}

/// Failure modes for rendezvous registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegisterRendezvousError {
    /// Attempted to register more rendezvous than the identifier space allows.
    CapacityExceeded,
    /// Caller-provided slab cannot fit the rendezvous header and tap ring.
    StorageExhausted,
}
