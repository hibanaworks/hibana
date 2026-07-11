use super::{
    ClusterError, DecisionArm, DynamicResolverKey, ErasedResolverRef, RendezvousId, ResolverRef,
    SessionCluster,
};
use crate::global::const_dsl::DynamicRouteResolver;
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
    ) -> Option<ErasedResolverRef<'cfg>> {
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
            for resolver in compiled.route_resolver_sites_for(RESOLVER) {
                matched_sites += 1;
                let key = DynamicResolverKey::new(rv_id, resolver);
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
            for binding in compiled.route_resolver_sites_for(RESOLVER) {
                self.commit_prepared_dynamic_resolver(
                    DynamicResolverKey::new(rv_id, binding),
                    resolver,
                );
            }
            Ok(())
        })
    }

    fn commit_prepared_dynamic_resolver<const RESOLVER: u16>(
        &self,
        key: DynamicResolverKey,
        resolver_ref: ResolverRef<'cfg, RESOLVER>,
    ) {
        if key.resolver().resolver_id() != RESOLVER {
            crate::invariant();
        }
        crate::invariant_ok(
            self.locals()
                .insert_dynamic_resolver(key, resolver_ref.erase()),
        );
    }

    pub(crate) fn resolve_dynamic_resolver(
        &self,
        rv_id: RendezvousId,
        resolver: DynamicRouteResolver,
    ) -> Result<DecisionArm, ClusterError> {
        let key = DynamicResolverKey::new(rv_id, resolver);
        let resolver_id = resolver.resolver_id();
        let Some(resolver_ref) = self.dynamic_resolver(key) else {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        };

        let arm = resolver_ref
            .resolve_decision()
            .map_err(|_| ClusterError::ResolverReject { resolver_id })?;
        Ok(arm)
    }
}
