use super::*;

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) fn run_effect_step(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<PendingEffect, CpError> {
        let envelope = match envelope.effect {
            ControlOp::CapDelegate => envelope.canonicalize_delegate()?,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                envelope.canonicalize_topology()?
            }
            _ => envelope,
        };

        if let Some(operands) = envelope.topology {
            Self::validate_topology_target(envelope.effect, target, operands)?;
        }

        if self.get_local(&target).is_some() {
            match envelope.effect {
                ControlOp::TopologyBegin => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.preflight_topology_begin(sid, operands)?;
                    self.ensure_local_topology_storage(target, operands.src_lane)?;
                    let seed = operands.intent(sid);
                    let dst_rv = seed.dst_rv;

                    let begin_needs = facets_caps_topology();

                    let drive_result = self.drive::<TopologyBeginAutomaton, _, _>(
                        target,
                        seed,
                        move |core, rv| {
                            let mut ctx =
                                Self::init_bundle_context_with_needs(core, rv, begin_needs);
                            ctx.set_topology(TopologyGraphContext::new(Some(seed)));
                            ctx
                        },
                        |core, graph| {
                            if dst_rv != target && begin_needs.requires_topology() {
                                graph.add_child_with_bundle_config(
                                    &mut core.locals,
                                    target,
                                    dst_rv,
                                    |child_ctx| {
                                        child_ctx.set_topology(TopologyGraphContext::default());
                                    },
                                )?;
                            }
                            Ok(())
                        },
                    );

                    if let Err(err) = drive_result {
                        return Err(match err {
                            DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_) => {
                                CpError::Topology(TopologyError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }
                    return self.after_local_effect(envelope);
                }
                ControlOp::TopologyAck => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.preflight_topology_ack(sid, operands)?;
                    self.ensure_local_topology_storage(target, operands.dst_lane)?;
                    return self.with_control_mut(|core| {
                        let ack = match core.locals.get_mut(&operands.dst_rv) {
                            Some(rv) => match rv.acknowledge_topology_intent(&operands.intent(sid))
                            {
                                Ok(ack) => ack,
                                Err(err) => {
                                    let err = CpError::Topology(err.into());
                                    let _ = Self::abort_inflight_topology_entry(
                                        core,
                                        sid,
                                        operands.src_rv,
                                    );
                                    return Err(err);
                                }
                            },
                            None => {
                                return Err(CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                });
                            }
                        };
                        if ack != operands.ack(sid) {
                            let err = CpError::Topology(TopologyError::GenerationMismatch);
                            let _ = Self::abort_inflight_topology_entry(core, sid, operands.src_rv);
                            return Err(err);
                        }
                        let recorded = core
                            .topology_state
                            .acknowledge(sid, operands.src_rv)
                            .expect(
                                "topology ack bookkeeping was preflighted before local mutation",
                            );
                        debug_assert_eq!(recorded, ack);
                        Ok(PendingEffect::None)
                    });
                }
                ControlOp::TopologyCommit => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.ensure_local_topology_storage(target, operands.src_lane)?;
                    return self.with_control_mut(|core| {
                        let tracked = core
                            .topology_state
                            .get(sid)
                            .copied()
                            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                        debug_assert_eq!(tracked.src_rv, operands.src_rv);

                        if let Err(err) = core.topology_state.preflight_commit(
                            sid,
                            operands.src_rv,
                            Some(operands.ack(sid)),
                        ) {
                            let _ = Self::abort_inflight_topology_entry(core, sid, operands.src_rv);
                            return Err(err);
                        }

                        let source_lane = match core.locals.get_mut(&operands.src_rv) {
                            Some(rv) => match rv.validate_topology_commit_operands(sid, operands) {
                                Ok(lane) => lane,
                                Err(err) => {
                                    let err = CpError::Topology(err.into());
                                    let _ = Self::abort_inflight_topology_entry(
                                        core,
                                        sid,
                                        tracked.src_rv,
                                    );
                                    return Err(err);
                                }
                            },
                            None => {
                                return Err(CpError::RendezvousMismatch {
                                    expected: operands.src_rv.raw(),
                                    actual: 0,
                                });
                            }
                        };

                        {
                            let rv = core.locals.get_mut(&operands.dst_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) =
                                rv.preflight_destination_topology_commit(sid, operands.dst_lane)
                            {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        {
                            let rv = core.locals.get_mut(&operands.dst_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) =
                                rv.finalize_destination_topology_commit(sid, operands.dst_lane)
                            {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        {
                            let rv = core.locals.get_mut(&operands.src_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.src_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) = rv.topology_commit(sid, source_lane) {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        let committed = core
                            .topology_state
                            .topology_commit(sid, operands.src_rv, Some(operands.ack(sid)))
                            .expect(
                                "topology commit bookkeeping was preflighted before local mutation",
                            );
                        debug_assert_eq!(committed, operands);
                        Ok(PendingEffect::None)
                    });
                }
                _ => {
                    if self.get_local(&target).is_some() {
                        self.with_control_mut(|core| {
                            let rv = core
                                .locals
                                .get_mut(&target)
                                .expect("local rendezvous must remain available");
                            EffectRunner::run_effect(rv, envelope.clone())
                        })?;
                        return self.after_local_effect(envelope);
                    }
                }
            }
        }

        Err(CpError::RendezvousMismatch {
            expected: target.raw(),
            actual: 0,
        })
    }

    #[inline]
    fn validate_topology_target(
        effect: ControlOp,
        target: RendezvousId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        let expected = match effect {
            ControlOp::TopologyBegin | ControlOp::TopologyCommit => operands.src_rv,
            ControlOp::TopologyAck => operands.dst_rv,
            _ => return Ok(()),
        };

        if target != expected {
            return Err(CpError::RendezvousMismatch {
                expected: expected.raw(),
                actual: target.raw(),
            });
        }

        Ok(())
    }

    pub(crate) fn run_effect(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<(), CpError> {
        self.run_effect_step(target, envelope)?;
        Ok(())
    }

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

    #[cfg(test)]
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
        input: [u32; 4],
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
                    RouteResolution::Arm(arm) if arm <= 1 => {
                        Ok(DynamicPolicyResolution::RouteArm { arm })
                    }
                    RouteResolution::Arm(_) => Err(CpError::PolicyAbort { reason: policy_id }),
                    RouteResolution::Defer => Ok(DynamicPolicyResolution::Defer),
                }
            }
            ControlOp::LoopContinue | ControlOp::LoopBreak => {
                let resolution = entry
                    .resolver
                    .resolve_loop(ctx)
                    .map_err(|_| CpError::PolicyAbort { reason: policy_id })?;
                if policy_scope.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                match resolution {
                    LoopResolution::Continue => {
                        Ok(DynamicPolicyResolution::Loop { decision: true })
                    }
                    LoopResolution::Break => Ok(DynamicPolicyResolution::Loop { decision: false }),
                    LoopResolution::Defer => Ok(DynamicPolicyResolution::Defer),
                }
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
