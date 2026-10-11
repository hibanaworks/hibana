use super::{
    ClusterError, DecisionArm, DynamicResolverKey, ErasedResolverRef, RendezvousId, ResolverRef,
    ResolverRegistrationKey, SessionCluster,
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
            if compiled.route_resolver_sites_for(RESOLVER).next().is_none() {
                return Err(ClusterError::ResolverReject {
                    resolver_id: RESOLVER,
                });
            }
            let key =
                DynamicResolverKey::new(rv_id, ResolverRegistrationKey::new(compiled, RESOLVER));
            if self.dynamic_resolver(key).is_none() {
                self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
            }
            self.commit_prepared_dynamic_resolver(key, resolver);
            Ok(())
        })
    }

    fn commit_prepared_dynamic_resolver<const RESOLVER: u16>(
        &self,
        key: DynamicResolverKey,
        resolver_ref: ResolverRef<'cfg, RESOLVER>,
    ) {
        if key.registration().resolver_id() != RESOLVER {
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
        program: &'static crate::global::compiled::images::CompiledProgramRef,
        resolver: DynamicRouteResolver,
    ) -> Result<DecisionArm, ClusterError> {
        let resolver_id = resolver.resolver_id();
        let key =
            DynamicResolverKey::new(rv_id, ResolverRegistrationKey::new(program, resolver_id));
        let Some(resolver_ref) = self.dynamic_resolver(key) else {
            return Err(ClusterError::DynamicResolverInvariant { resolver_id });
        };

        let arm = resolver_ref
            .resolve_decision()
            .map_err(|_| ClusterError::ResolverReject { resolver_id })?;
        Ok(arm)
    }
}
