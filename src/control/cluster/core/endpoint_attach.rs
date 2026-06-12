use super::{
    AttachError, CpError, EffectEnvelopeRef, EndpointInitArgs, EndpointLeaseId, Lane,
    MintConfigMarker, RendezvousId, ResourceScope, RoleImageSlice, SessionCluster, SessionId,
    TopologyError, TopologySessionState,
};
impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline(never)]
    fn attach_secondary_endpoint_lanes<'lease, const ROLE: u8, Mint>(
        &'lease self,
        dst: *mut crate::endpoint::kernel::CursorEndpoint<
            'lease,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
        >,
        rv_id: RendezvousId,
        sid: SessionId,
        role_count: u8,
        effect_envelope: EffectEnvelopeRef<'_>,
        role_image: RoleImageSlice<ROLE>,
        logical_lane_count: usize,
        occupied_lane_index: usize,
    ) -> Result<(), AttachError>
    where
        'cfg: 'lease,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let mut logical_idx = 0usize;
        while logical_idx < logical_lane_count {
            if logical_idx == occupied_lane_index || !role_image.has_active_lane(logical_idx) {
                logical_idx += 1;
                continue;
            }

            let physical_lane = Lane::new(logical_idx as u32);
            let mut lease = self
                .lease_port(rv_id, sid, physical_lane, ROLE, role_count)
                .map_err(AttachError::from)?;
            lease.with_rendezvous_mut(|rv| -> Result<(), AttachError> {
                rv.activate_lane_attachment(sid, physical_lane)
                    .map_err(AttachError::from)?;
                Self::init_session_effects_for_lane(rv, sid, physical_lane, effect_envelope)
                    .map_err(AttachError::from)
            })?;
            let (port, guard, _brand) = lease.into_port_guard().map_err(AttachError::from)?;
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                crate::endpoint::kernel::endpoint_init::write_port_slot(dst, logical_idx, port);
                crate::endpoint::kernel::endpoint_init::write_guard_slot(dst, logical_idx, guard);
            }
            logical_idx += 1;
        }
        Ok(())
    }

    #[inline(never)]
    unsafe fn init_endpoint_with_compiled_into<'r, const ROLE: u8, Mint>(
        &'r self,
        args: EndpointInitArgs<'r, ROLE, T, U, C, MAX_RV, Mint>,
    ) -> Result<(), AttachError>
    where
        'cfg: 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let EndpointInitArgs {
            dst,
            arena_storage,
            rv_id,
            sid,
            role_image,
            public_slot,
            public_generation,
            public_ops,
            public_slot_owned,
            mint,
        } = args;
        let program_image = role_image.program();
        let effect_envelope = program_image.effect_envelope();
        let role_count = u8::try_from(program_image.role_count()).expect("invariant");
        let logical_lane_count = role_image.logical_lane_count().max(1);
        let primary_lane_index = Self::primary_endpoint_lane_index(role_image, logical_lane_count);
        let control_lane_index = 0usize;
        let control_wire_lane = Lane::new(control_lane_index as u32);
        let mut control_lease = self
            .lease_port(rv_id, sid, control_wire_lane, ROLE, role_count)
            .map_err(AttachError::from)?;
        control_lease.with_rendezvous_mut(|rv| -> Result<(), AttachError> {
            rv.activate_lane_attachment(sid, control_wire_lane)
                .map_err(AttachError::from)?;
            Self::init_session_effects_for_lane(rv, sid, control_wire_lane, effect_envelope)
                .map_err(AttachError::from)
        })?;
        let (control_port, control_guard, control_brand) =
            control_lease.into_port_guard().map_err(AttachError::from)?;
        let owner: crate::control::cap::mint::Owner<'r, crate::control::cap::mint::E0> = /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */ unsafe {
            core::mem::transmute(crate::control::cap::mint::Owner::<
                'cfg,
                crate::control::cap::mint::E0,
            >::new(control_brand))
        };
        let epoch = crate::control::cap::mint::EndpointEpoch::new();
        let offer_progress_policy = self.with_control_mut(|core| {
            core.locals
                .get_mut(&rv_id)
                .map(|rv| rv.offer_progress_policy())
                .expect("invariant")
        });
        let control: crate::endpoint::control::SessionControlCtx<
            'r,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
        > = /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */ unsafe {
            core::mem::transmute(crate::endpoint::control::SessionControlCtx::<
                'cfg,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
            >::new(
                control_wire_lane, Some(&*(self as *const Self)), None
            ))
        };

        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            crate::endpoint::kernel::endpoint_init::init_empty_from_compiled(
                crate::endpoint::kernel::endpoint_init::CompiledEndpointInit {
                    dst,
                    arena_storage,
                    primary_lane: primary_lane_index,
                    sid,
                    owner,
                    epoch,
                    role_descriptor: role_image.descriptor(),
                    public_rv: rv_id,
                    public_slot,
                    public_generation,
                    public_ops,
                    public_slot_owned,
                    offer_progress_policy,
                    control,
                    mint,
                },
            );
            crate::endpoint::kernel::endpoint_init::write_port_slot(
                dst,
                control_lane_index,
                control_port,
            );
            crate::endpoint::kernel::endpoint_init::write_guard_slot(
                dst,
                control_lane_index,
                control_guard,
            );
        }
        let init_result = self.attach_secondary_endpoint_lanes::<ROLE, Mint>(
            dst,
            rv_id,
            sid,
            role_count,
            effect_envelope,
            role_image,
            logical_lane_count,
            control_lane_index,
        );

        if let Err(err) = init_result {
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                core::ptr::drop_in_place(dst);
            }
            return Err(err);
        }

        if let Err(err) = self.bind_session_role(sid, ROLE, rv_id) {
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                core::ptr::drop_in_place(dst);
            }
            return Err(AttachError::control(err));
        }

        /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
        unsafe {
            crate::endpoint::kernel::endpoint_init::finish_init(dst);
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn attach_public_endpoint<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint_inner(rv_id, sid, program)
    }

    #[inline]
    fn role_image_attaches_lane<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
        lane: Lane,
    ) -> bool {
        let lane_idx = lane.raw() as usize;
        lane_idx == 0
            || (lane_idx < role_image.logical_lane_count() && role_image.has_active_lane(lane_idx))
    }

    #[inline]
    fn primary_endpoint_lane_index<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
        logical_lane_count: usize,
    ) -> usize {
        let Some(primary_lane_index) = role_image.first_active_lane() else {
            return 0;
        };
        if primary_lane_index >= logical_lane_count {
            crate::invariant();
        }
        primary_lane_index
    }

    #[inline]
    fn attach_public_endpoint_inner<'r, 'prog, const ROLE: u8, P>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &P,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
        'cfg: 'r,
    {
        self.with_control_mut(|core| {
            let topology_phase = core.topology_state.phase(sid);

            if topology_phase.is_some() {
                let operands =
                    core.topology_state
                        .get(sid)
                        .copied()
                        .ok_or(AttachError::control(CpError::Topology(
                            TopologyError::InvalidSession,
                        )))?;
                if operands.src_rv == rv_id || operands.dst_rv == rv_id {
                    Self::abort_inflight_topology_entry(core, sid, operands.src_rv)
                        .map_err(AttachError::control)?;
                }
                return Err(AttachError::control(CpError::Topology(
                    TopologyError::InvalidState,
                )));
            }

            let rv = core
                .locals
                .get_mut_checked(&rv_id)
                .map_err(Self::map_rendezvous_access_error)
                .map_err(AttachError::control)?;
            let topology_session_state = rv.topology_session_state(sid);
            match topology_session_state {
                None | Some(TopologySessionState::DestinationAttachReady { .. }) => {}
                Some(TopologySessionState::SourcePending { .. }) => {
                    return Err(AttachError::control(CpError::Topology(
                        TopologyError::InvalidState,
                    )));
                }
                Some(TopologySessionState::DestinationPending { .. }) => {
                    return Err(AttachError::control(CpError::Topology(
                        TopologyError::InvalidState,
                    )));
                }
            }
            Ok(())
        })?;

        match self.ensure_role_image_slice(rv_id, program) {
            Ok(role_image) =>
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                self.with_control_mut(|core| {
                    let topology_session_state = core
                        .locals
                        .get_mut_checked(&rv_id)
                        .map_err(Self::map_rendezvous_access_error)
                        .map_err(AttachError::control)
                        .and_then(|rv| {
                            rv.ensure_core_lane_storage_for_lane_slots(
                                role_image.logical_lane_count().max(1),
                            )
                            .ok_or_else(|| {
                                AttachError::control(CpError::resource_exhausted(
                                    ResourceScope::LaneStorage,
                                ))
                            })?;
                            Ok(rv.topology_session_state(sid))
                        })?;

                    if let Some(TopologySessionState::DestinationAttachReady { lane }) =
                        topology_session_state
                        && !Self::role_image_attaches_lane(role_image, lane)
                    {
                        return Err(AttachError::control(CpError::Topology(
                            TopologyError::InvalidState,
                        )));
                    }

                    Ok(())
                })?;
                let storage_layout = Self::public_endpoint_storage_requirement(role_image);
                let resident_budget = Self::public_endpoint_resident_budget(role_image);
                let (slot, generation, dst) = self.allocate_public_endpoint_storage_for_rv::<
                    ROLE,
                    crate::control::cap::mint::MintConfig,
                >(
                    rv_id,
                    storage_layout.total_bytes,
                    storage_layout.total_align,
                    resident_budget,
                )?;
                let arena_storage = dst.cast::<u8>().add(storage_layout.arena_offset);
                let public_ops =
                    crate::integration::SessionKit::<'cfg, T, U, C, MAX_RV>::endpoint_ops::<ROLE>();
                if let Err(err) = self.init_endpoint_with_compiled_into::<
                    ROLE,
                    crate::control::cap::mint::MintConfig,
                >(EndpointInitArgs {
                    dst,
                    arena_storage,
                    rv_id,
                    sid,
                    role_image,
                    public_slot: slot,
                    public_generation: generation,
                    public_ops,
                    public_slot_owned: true,
                    mint: crate::control::cap::mint::MintConfig::INSTANCE,
                }) {
                    self.with_control_mut(|core| {
                        if let Some(rv) = core.locals.get_mut(&rv_id) {
                            rv.release_endpoint_lease(slot, generation);
                        }
                    });
                    return Err(err);
                }
                Ok((slot, generation))
            },
            Err(err) => Err(err),
        }
    }

    #[inline]
    pub(crate) fn enter<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.enter_endpoint::<ROLE>(rv_id, sid, program)
    }

    #[inline]
    fn enter_endpoint<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint::<ROLE>(rv_id, sid, program)
    }
}
