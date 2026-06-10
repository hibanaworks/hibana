use super::{
    CpError, DecisionResolution, DecisionSubject, DynamicPolicyResolution, DynamicResolverEntry,
    DynamicResolverKey, EffIndex, RendezvousId, ResolverMode, ResolverRef, SessionCluster,
};
impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
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
        resolver: ResolverRef<'cfg, POLICY>,
    ) -> Result<(), CpError> {
        self.with_resident_program_ref(rv_id, program, |compiled| {
            let mut matched_sites = 0usize;
            let mut missing_sites = 0usize;
            let mut decision_scope = None;
            for site in compiled.dynamic_policy_sites_for(POLICY) {
                matched_sites += 1;
                let site_scope = site.policy().scope();
                if site_scope.is_none() {
                    return Err(CpError::PolicyAbort { reason: POLICY });
                }
                match decision_scope {
                    Some(scope) if scope != site_scope => {
                        return Err(CpError::PolicyAbort { reason: POLICY });
                    }
                    None => decision_scope = Some(site_scope),
                    _ => {}
                }
                let subject = site
                    .subject()
                    .ok_or(CpError::UnsupportedEffect(site.logical_label()))?;
                let key = DynamicResolverKey::new(rv_id, site.eff_index(), subject);
                if self.dynamic_resolver(key).is_none() {
                    missing_sites += 1;
                }
            }
            if matched_sites == 0 {
                return Err(CpError::PolicyAbort { reason: POLICY });
            }
            self.ensure_dynamic_resolver_capacity(rv_id, missing_sites)?;
            for site in compiled.dynamic_policy_sites_for(POLICY) {
                let subject = site
                    .subject()
                    .ok_or(CpError::UnsupportedEffect(site.logical_label()))?;
                self.register_dynamic_policy_resolver(
                    rv_id,
                    site.eff_index(),
                    site.logical_label(),
                    site.policy(),
                    subject,
                    resolver,
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn register_dynamic_policy_resolver<const POLICY: u16>(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        label: u8,
        policy: ResolverMode,
        subject: DecisionSubject,
        resolver: ResolverRef<'cfg, POLICY>,
    ) -> Result<(), CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, subject);
        let policy = match policy {
            ResolverMode::Dynamic { .. } => {
                let policy_id = policy
                    .dynamic_policy_id()
                    .ok_or(CpError::UnsupportedEffect(label))?;
                if policy_id != POLICY {
                    return Err(CpError::PolicyAbort { reason: POLICY });
                }
                if policy.scope().is_none() {
                    return Err(CpError::PolicyAbort { reason: POLICY });
                }
                if !resolver.accepts_subject(subject) {
                    return Err(CpError::UnsupportedEffect(subject.as_error_code()));
                }
                policy
            }
            _ => return Err(CpError::UnsupportedEffect(label)),
        };
        let entry = DynamicResolverEntry {
            resolver: resolver.erase(),
            policy,
        };
        if self.dynamic_resolver(key).is_none() {
            self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
        }
        self.with_resolvers_mut(|core| core.insert(key, entry))
    }

    pub(crate) fn resolve_dynamic_policy(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        subject: DecisionSubject,
    ) -> Result<DynamicPolicyResolution, CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, subject);
        let entry = self
            .dynamic_resolver(key)
            .ok_or_else(|| CpError::PolicyAbort { reason: 0 })?;
        let policy = entry.policy;

        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(CpError::PolicyAbort { reason: 6 })?;

        let policy_scope = policy.scope();
        let resolution = entry
            .resolver
            .resolve_decision()
            .map_err(|_| CpError::PolicyAbort { reason: policy_id })?;
        if policy_scope.is_none() {
            return Err(CpError::PolicyAbort { reason: policy_id });
        }
        match resolution {
            DecisionResolution::Arm(arm) => {
                Ok(DynamicPolicyResolution::DecisionArm { arm: arm.index() })
            }
            DecisionResolution::Defer => Ok(DynamicPolicyResolution::Defer),
        }
    }
}
