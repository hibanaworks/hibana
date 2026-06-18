use core::{ptr, ptr::NonNull};

use super::{ROLE_CLAIM_SLOTS, RendezvousEntry, RendezvousTable, SessionRoleClaim};
use crate::{
    rendezvous::core::Rendezvous,
    session::types::{RendezvousId, SessionId},
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

    pub(crate) fn claim_session_role(
        &mut self,
        sid: SessionId,
        role: u8,
        rv: RendezvousId,
    ) -> Result<(), RoleClaimError> {
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
            let mut col = 0usize;
            while col < ROLE_CLAIM_SLOTS {
                if let Some(claim) = &entry.role_claims[col]
                    && claim.sid == sid
                    && claim.role == role
                {
                    if entry.id != rv {
                        return Err(RoleClaimError::RendezvousMismatch {
                            expected: entry.id.raw(),
                            actual: rv.raw(),
                        });
                    }
                    return Err(RoleClaimError::AlreadyClaimed(rv));
                }
                col += 1;
            }
            current = entry.next;
        }

        let Some(mut target) = target else {
            return Err(RoleClaimError::RendezvousUnregistered(rv));
        };
        let entry = /* SAFETY: target was discovered in the initialized registry list above. */ unsafe {
            target.as_mut()
        };
        let mut col = 0usize;
        while col < ROLE_CLAIM_SLOTS {
            if entry.role_claims[col].is_none() {
                entry.role_claims[col] = Some(SessionRoleClaim { sid, role });
                return Ok(());
            }
            col += 1;
        }
        Err(RoleClaimError::CapacityExceeded)
    }

    pub(crate) fn release_session_role_claim(
        &mut self,
        sid: SessionId,
        role: u8,
        rv: RendezvousId,
    ) -> bool {
        let mut current = self.head;
        while let Some(mut entry_ptr) = current {
            let entry = /* SAFETY: registry links are initialized slab nodes and remain pinned until table drop. */ unsafe {
                entry_ptr.as_mut()
            };
            if entry.id == rv {
                let mut col = 0usize;
                while col < ROLE_CLAIM_SLOTS {
                    if let Some(claim) = &entry.role_claims[col]
                        && claim.sid == sid
                        && claim.role == role
                    {
                        entry.role_claims[col] = None;
                        return true;
                    }
                    col += 1;
                }
                return false;
            }
            current = entry.next;
        }
        false
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
                unsafe {
                    (&mut *rv_ptr).free_external_persistent_sidecar(
                        sidecar,
                        crate::session::cluster::error::ResourceScope::ResolverTable,
                    )
                }
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
            .insert(key.eff_index, entry)
    }

    pub(crate) fn dynamic_resolver(
        &self,
        key: crate::session::cluster::core::DynamicResolverKey,
    ) -> Option<&crate::session::cluster::core::DynamicResolverEntry<'cfg>> {
        self.entry_ref(&key.rv)?.resolver_bucket.get(key.eff_index)
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

/// Session-role claim failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoleClaimError {
    /// No rendezvous with the requested identifier exists.
    RendezvousUnregistered(RendezvousId),
    /// A session role is already attached to another rendezvous.
    RendezvousMismatch { expected: u16, actual: u16 },
    /// A live endpoint already owns this session role on the selected rendezvous.
    AlreadyClaimed(RendezvousId),
    /// The selected rendezvous has no remaining role claim row capacity.
    CapacityExceeded,
}
