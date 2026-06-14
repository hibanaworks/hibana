use super::{
    AttachError, ClusterError, EndpointInitArgs, EndpointLeaseId, Lane, RendezvousId,
    RoleImageSlice, SessionCluster, SessionId,
};

struct SecondaryEndpointLaneAttach<'lease, const ROLE: u8, T, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'lease,
    C: crate::runtime_core::config::Clock + 'lease,
{
    dst: *mut crate::endpoint::kernel::CursorEndpoint<'lease, ROLE, T, C, MAX_RV>,
    rv_id: RendezvousId,
    sid: SessionId,
    role_count: u8,
    role_image: RoleImageSlice<ROLE>,
    logical_lane_count: usize,
    occupied_lane_index: usize,
}

impl<'cfg, T, C, const MAX_RV: usize> SessionCluster<'cfg, T, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    #[inline(never)]
    fn attach_secondary_endpoint_lanes<'lease, const ROLE: u8>(
        &'lease self,
        attach: SecondaryEndpointLaneAttach<'lease, ROLE, T, C, MAX_RV>,
    ) -> Result<(), AttachError>
    where
        'cfg: 'lease,
    {
        let SecondaryEndpointLaneAttach {
            dst,
            rv_id,
            sid,
            role_count,
            role_image,
            logical_lane_count,
            occupied_lane_index,
        } = attach;
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
                    .map_err(AttachError::from)
            })?;
            let access = lease.into_port_guard();
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                crate::endpoint::kernel::endpoint_init::write_port_slot(
                    dst,
                    logical_idx,
                    access.port,
                );
                crate::endpoint::kernel::endpoint_init::write_guard_slot(
                    dst,
                    logical_idx,
                    access.lane_guard,
                );
            }
            logical_idx += 1;
        }
        Ok(())
    }

    #[inline(never)]
    unsafe fn init_endpoint_with_compiled_into<'r, const ROLE: u8>(
        &'r self,
        args: EndpointInitArgs<'r, ROLE, T, C, MAX_RV>,
    ) -> Result<(), AttachError>
    where
        'cfg: 'r,
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
            public_slot_ownership,
        } = args;
        let program_image = role_image.program();
        let role_count = crate::invariant_ok(u8::try_from(program_image.role_count()));
        let logical_lane_count = role_image.logical_lane_count().max(1);
        let primary_lane_index = Self::primary_endpoint_lane_index(role_image, logical_lane_count);
        let session_lane_index = 0usize;
        let session_wire_lane = Lane::new(session_lane_index as u32);
        let mut session_lease = self
            .lease_port(rv_id, sid, session_wire_lane, ROLE, role_count)
            .map_err(AttachError::from)?;
        session_lease.with_rendezvous_mut(|rv| -> Result<(), AttachError> {
            rv.activate_lane_attachment(sid, session_wire_lane)
                .map_err(AttachError::from)
        })?;
        let session_access = session_lease.into_port_guard();
        let owner: crate::session::brand::Owner<'r> = /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */ unsafe {
            core::mem::transmute(crate::session::brand::Owner::<'cfg>::new(session_access.brand))
        };
        let session: crate::endpoint::session::SessionCtx<
            'r,
            T,
            C,
            MAX_RV,
        > = /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */ unsafe {
            core::mem::transmute(crate::endpoint::session::SessionCtx::<
                'cfg,
                T,
                C,
                MAX_RV,
            >::new(session_wire_lane, &*(self as *const Self)))
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
                    role_descriptor: role_image.descriptor(),
                    public_rv: rv_id,
                    public_slot,
                    public_generation,
                    public_ops,
                    public_slot_ownership,
                    session,
                },
            );
            crate::endpoint::kernel::endpoint_init::write_port_slot(
                dst,
                session_lane_index,
                session_access.port,
            );
            crate::endpoint::kernel::endpoint_init::write_guard_slot(
                dst,
                session_lane_index,
                session_access.lane_guard,
            );
        }
        let init_result =
            self.attach_secondary_endpoint_lanes::<ROLE>(SecondaryEndpointLaneAttach {
                dst,
                rv_id,
                sid,
                role_count,
                role_image,
                logical_lane_count,
                occupied_lane_index: session_lane_index,
            });

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
            return Err(AttachError::cluster(err));
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
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint_inner(rv_id, sid, program)
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
        match self.ensure_role_image_slice(rv_id, program) {
            Ok(role_image) =>
            /* SAFETY: endpoint attach owns the resident slot being projected and checks lane/generation identity before raw access. */
            unsafe {
                self.with_storage_mut(|core| {
                    core.locals
                        .get_mut_checked(&rv_id)
                        .map_err(Self::map_rendezvous_access_error)
                        .map_err(AttachError::cluster)?
                        .ensure_core_lane_storage_for_lane_slots(
                            role_image.logical_lane_count().max(1),
                        )
                        .map_err(|scope| {
                            AttachError::cluster(ClusterError::resource_exhausted(scope))
                        })
                })?;
                let storage_layout = Self::public_endpoint_storage_requirement(role_image);
                let resident_budget = Self::public_endpoint_resident_budget(role_image);
                let (slot, generation, dst) = self
                    .allocate_public_endpoint_storage_for_rv::<ROLE>(
                        rv_id,
                        storage_layout.total_bytes,
                        storage_layout.total_align,
                        resident_budget,
                    )?;
                let arena_storage = dst.cast::<u8>().add(storage_layout.arena_offset);
                let public_ops =
                    crate::runtime::SessionKit::<'cfg, T, C, MAX_RV>::endpoint_ops::<ROLE>();
                if let Err(err) = self.init_endpoint_with_compiled_into::<ROLE>(EndpointInitArgs {
                    dst,
                    arena_storage,
                    rv_id,
                    sid,
                    role_image,
                    public_slot: slot,
                    public_generation: generation,
                    public_ops,
                    public_slot_ownership: crate::endpoint::kernel::PublicSlotOwnership::Owned,
                }) {
                    self.with_storage_mut(|core| {
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
        program: &crate::runtime::program::RoleProgram<ROLE>,
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
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint::<ROLE>(rv_id, sid, program)
    }
}
