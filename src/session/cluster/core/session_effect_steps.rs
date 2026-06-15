use super::{
    ClusterError, DecisionResolution, DynamicResolverEntry, DynamicResolverKey,
    DynamicResolverResolution, EffIndex, RendezvousId, ResolverRef, ResourceScope, SessionCluster,
};
impl<'cfg, T, const MAX_RV: usize> SessionCluster<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    fn ensure_dynamic_resolver_capacity(
        &self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), ClusterError> {
        if additional_entries == 0 {
            return Ok(());
        }
        self.with_storage_mut(|core| {
            let rv = core
                .locals
                .get_mut(&rv_id)
                .ok_or(ClusterError::RendezvousMismatch {
                    expected: rv_id.raw(),
                    actual: 0,
                })?;
            let rv_ptr = ::core::ptr::from_mut(rv);
            /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.resolvers_ptr() }.ensure_capacity(
                rv_id,
                additional_entries,
                |bytes, align| /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */ unsafe {
                    (&mut *rv_ptr).allocate_external_persistent_sidecar_bytes(bytes, align)
                },
                |sidecar| /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */ unsafe {
                    (&mut *rv_ptr).free_external_persistent_sidecar(
                        sidecar,
                        ResourceScope::ResolverTable,
                    )
                },
            )
        })
    }

    pub(crate) fn dynamic_resolver(
        &self,
        key: DynamicResolverKey,
    ) -> Option<&DynamicResolverEntry<'cfg>> {
        /* SAFETY: resolver references are read through the cluster-owned resolver table after key validation. */
        unsafe { (*self.resolvers_ref_ptr()).get(key) }
    }

    pub(crate) fn set_resolver<const RESOLVER: u16, const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
        resolver: ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), ClusterError> {
        self.with_resident_program_ref(rv_id, program, |compiled| {
            let mut matched_sites = 0usize;
            let mut missing_sites = 0usize;
            let mut decision_scope = None;
            for site in compiled.dynamic_resolver_sites_for(RESOLVER) {
                matched_sites += 1;
                let site_scope = site.scope();
                if site_scope.is_none() {
                    return Err(ClusterError::ResolverReject {
                        resolver_id: RESOLVER,
                    });
                }
                match decision_scope {
                    None => decision_scope = Some(site_scope),
                    Some(scope) => {
                        if scope != site_scope {
                            return Err(ClusterError::ResolverReject {
                                resolver_id: RESOLVER,
                            });
                        }
                    }
                }
                let key = DynamicResolverKey::new(rv_id, site.eff_index());
                if self.dynamic_resolver(key).is_none() {
                    missing_sites += 1;
                }
            }
            if matched_sites == 0 {
                return Err(ClusterError::ResolverReject {
                    resolver_id: RESOLVER,
                });
            }
            self.ensure_dynamic_resolver_capacity(rv_id, missing_sites)?;
            for site in compiled.dynamic_resolver_sites_for(RESOLVER) {
                self.register_dynamic_resolver_resolver(
                    rv_id,
                    site.eff_index(),
                    site.resolver_id(),
                    site.resolver_scope(),
                    resolver,
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn register_dynamic_resolver_resolver<const RESOLVER: u16>(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        resolver_id: u16,
        scope: crate::global::const_dsl::CompactScopeId,
        resolver_ref: ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), ClusterError> {
        let key = DynamicResolverKey::new(rv_id, eff_index);
        if resolver_id != RESOLVER {
            return Err(ClusterError::ResolverReject {
                resolver_id: RESOLVER,
            });
        }
        if scope.to_scope_id().is_none() {
            return Err(ClusterError::ResolverReject {
                resolver_id: RESOLVER,
            });
        }
        let entry = DynamicResolverEntry {
            resolver_ref: resolver_ref.erase(),
            resolver_id,
            scope,
        };
        if self.dynamic_resolver(key).is_none() {
            self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
        }
        self.with_resolvers_mut(|core| core.insert(key, entry))
    }

    pub(crate) fn resolve_dynamic_resolver(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        resolver_id: u16,
    ) -> Result<DynamicResolverResolution, ClusterError> {
        let key = DynamicResolverKey::new(rv_id, eff_index);
        let Some(entry) = self.dynamic_resolver(key) else {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        };
        if entry.resolver_id != resolver_id {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        }

        let resolution = entry
            .resolver_ref
            .resolve_decision()
            .map_err(|_| ClusterError::ResolverReject { resolver_id })?;
        if entry.scope.to_scope_id().is_none() {
            return Err(ClusterError::ResolverReject { resolver_id });
        }
        match resolution {
            DecisionResolution::Arm(arm) => {
                Ok(DynamicResolverResolution::DecisionArm { arm: arm.index() })
            }
            DecisionResolution::Defer => Ok(DynamicResolverResolution::Defer),
        }
    }
}
