use super::{
    ClusterError, DecisionArm, DynamicResolverEntry, DynamicResolverKey, RendezvousId, ResolverRef,
    SessionCluster,
};
impl<'cfg, T> SessionCluster<'cfg, T>
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
        self.locals()
            .ensure_dynamic_resolver_capacity(rv_id, additional_entries)
    }

    pub(crate) fn dynamic_resolver(
        &self,
        key: DynamicResolverKey,
    ) -> Option<DynamicResolverEntry<'cfg>> {
        self.locals().dynamic_resolver(key)
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
            for site in compiled.route_resolver_sites_for(RESOLVER) {
                matched_sites += 1;
                let site_scope = site.scope();
                if site_scope.is_none() {
                    return Err(ClusterError::ResolverReject {
                        resolver_id: RESOLVER,
                    });
                }
                let key = DynamicResolverKey::new(rv_id, site_scope);
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
            for site in compiled.route_resolver_sites_for(RESOLVER) {
                self.register_dynamic_resolver_resolver(
                    rv_id,
                    site.resolver_id(),
                    site.scope(),
                    resolver,
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn register_dynamic_resolver_resolver<const RESOLVER: u16>(
        &self,
        rv_id: RendezvousId,
        resolver_id: u16,
        scope: crate::global::const_dsl::ScopeId,
        resolver_ref: ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), ClusterError> {
        let key = DynamicResolverKey::new(rv_id, scope);
        if resolver_id != RESOLVER {
            return Err(ClusterError::ResolverReject {
                resolver_id: RESOLVER,
            });
        }
        if scope.is_none() {
            return Err(ClusterError::ResolverReject {
                resolver_id: RESOLVER,
            });
        }
        let entry = DynamicResolverEntry {
            resolver_ref: resolver_ref.erase(),
            resolver_id,
        };
        if self.dynamic_resolver(key).is_none() {
            self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
        }
        self.locals().insert_dynamic_resolver(key, entry)
    }

    pub(crate) fn resolve_dynamic_resolver(
        &self,
        rv_id: RendezvousId,
        scope: crate::global::const_dsl::ScopeId,
        resolver_id: u16,
    ) -> Result<DecisionArm, ClusterError> {
        let key = DynamicResolverKey::new(rv_id, scope);
        let Some(entry) = self.dynamic_resolver(key) else {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        };
        if entry.resolver_id != resolver_id {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        }

        let arm = entry
            .resolver_ref
            .resolve_decision()
            .map_err(|_| ClusterError::ResolverReject { resolver_id })?;
        Ok(arm)
    }
}
