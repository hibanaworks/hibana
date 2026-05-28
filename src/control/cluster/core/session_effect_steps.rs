use super::{
    ControlOp, CpError, DynamicPolicyResolution, DynamicResolverEntry, DynamicResolverKey,
    EffIndex, Lane, PolicyMode, RendezvousId, ResolverContext, ResolverRef, RouteResolution,
    ScopeTrace, SessionCluster, SessionId, TopologyOperands, is_dynamic_control_op,
};
use crate::transport::context::PolicyInput;
impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) fn distributed_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| {
            core.topology_state
                .get(sid)
                .copied()
                .or_else(|| core.cached_operands_get(sid).copied())
        })
    }

    pub(crate) fn cached_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| core.cached_operands_get(sid).copied())
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn cache_topology_operands(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| core.cached_operands_insert(sid, operands))
    }

    fn ensure_dynamic_resolver_capacity(
        &self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), CpError> {
        if additional_entries == 0 {
            return Ok(());
        }
        self.with_control_mut(|core| {
            let rv = core
                .locals
                .get_mut(&rv_id)
                .ok_or(CpError::RendezvousMismatch {
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
                |ptr, bytes, reclaim_delta| /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */ unsafe {
                    (&mut *rv_ptr).free_external_persistent_sidecar_bytes(ptr, bytes, reclaim_delta)
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

    pub(crate) fn set_resolver<'prog, const POLICY: u16, const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        program: &crate::integration::program::RoleProgram<ROLE>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        self.with_resident_program_ref(rv_id, program, |compiled| {
            self.ensure_dynamic_resolver_capacity(
                rv_id,
                compiled.dynamic_policy_sites_for(POLICY).count(),
            )?;
            for site in compiled.dynamic_policy_sites_for(POLICY) {
                let tag = site
                    .resource_tag()
                    .ok_or(CpError::UnsupportedEffect(site.logical_label()))?;
                let op = site
                    .op()
                    .ok_or(CpError::UnsupportedEffect(site.logical_label()))?;
                self.register_dynamic_policy_resolver(
                    rv_id,
                    site.eff_index(),
                    site.logical_label(),
                    site.policy(),
                    tag,
                    op,
                    None,
                    resolver,
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn register_dynamic_policy_resolver(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        label: u8,
        policy: PolicyMode,
        _tag: u8,
        op: ControlOp,
        scope_trace: Option<ScopeTrace>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
        let policy = match policy {
            PolicyMode::Dynamic { .. } => {
                let _ = policy
                    .dynamic_policy_id()
                    .ok_or(CpError::UnsupportedEffect(label))?;
                if !is_dynamic_control_op(op) {
                    return Err(CpError::UnsupportedEffect(op as u8));
                }
                if !resolver.accepts_op(op) {
                    return Err(CpError::UnsupportedEffect(op as u8));
                }
                policy
            }
            _ => return Err(CpError::UnsupportedEffect(label)),
        };
        let entry = DynamicResolverEntry {
            resolver,
            policy,
            scope_trace,
        };
        self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
        self.with_resolvers_mut(|core| core.insert(key, entry))
    }

    pub(crate) fn resolve_dynamic_policy(
        &self,
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        op: ControlOp,
        input: PolicyInput,
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Result<DynamicPolicyResolution, CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
        let entry = self
            .dynamic_resolver(key)
            .ok_or_else(|| CpError::PolicyAbort { reason: 0 })?;
        let policy = entry.policy;

        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(CpError::PolicyAbort { reason: 6 })?;

        let policy_scope = policy.scope();

        let ctx = ResolverContext::new(
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            policy_scope,
            entry.scope_trace,
            input,
            attrs,
        );

        match op {
            ControlOp::RouteDecision => {
                let resolution = entry
                    .resolver
                    .resolve_route(ctx)
                    .map_err(|_| CpError::PolicyAbort { reason: policy_id })?;
                if policy_scope.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                match resolution {
                    RouteResolution::Arm(arm) => {
                        Ok(DynamicPolicyResolution::RouteArm { arm: arm.index() })
                    }
                    RouteResolution::Defer => Ok(DynamicPolicyResolution::Defer),
                }
            }
            ControlOp::LoopContinue | ControlOp::LoopBreak => {
                let _ = (entry, ctx);
                Err(CpError::PolicyAbort { reason: policy_id })
            }
            _ => Err(CpError::PolicyAbort { reason: policy_id }),
        }
    }

    pub(crate) fn policy_mode_for(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        op: ControlOp,
    ) -> Result<PolicyMode, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let lane_rv = Lane::new(lane.raw());
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
        let policy = rv
            .policy(lane_rv, eff_index, tag)
            .or_else(|| self.dynamic_resolver(key).map(|entry| entry.policy));
        Ok(policy.unwrap_or(PolicyMode::Static))
    }
}
