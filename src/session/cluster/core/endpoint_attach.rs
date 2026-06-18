use super::{
    AttachError, ClusterError, EndpointInitArgs, EndpointLeaseId, Lane, RendezvousId,
    RoleImageSlice, SessionCluster, SessionId,
};

struct SecondaryEndpointLaneAttach<'lease, const ROLE: u8, T>
where
    T: crate::transport::Transport + 'lease,
{
    dst: *mut crate::endpoint::kernel::CursorEndpoint<'lease, ROLE, T>,
    rv_id: RendezvousId,
    sid: SessionId,
    role_count: u8,
    role_image: RoleImageSlice<ROLE>,
    logical_lane_count: usize,
    occupied_lane_index: usize,
}

impl<'cfg, T> SessionCluster<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline(never)]
    fn attach_secondary_endpoint_lanes<'lease, const ROLE: u8>(
        &'lease self,
        attach: SecondaryEndpointLaneAttach<'lease, ROLE, T>,
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
            /* SAFETY: secondary-lane attach owns the endpoint slot before
            publication. `logical_idx` selects initialized port/guard columns,
            and the lane lease has validated the physical lane for this session. */
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
        args: EndpointInitArgs<'r, ROLE, T>,
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
        let owner: crate::session::brand::Owner<'r> = /* SAFETY: the port guard
        owns the validated session brand for this attach operation; narrowing it
        to `'r` matches the endpoint lease lifetime that will hold the port. */ unsafe {
            core::mem::transmute(crate::session::brand::Owner::<'cfg>::new(session_access.brand))
        };
        let session: crate::endpoint::session::SessionCtx<'r, T> =
            /* SAFETY: endpoint attach owns the public endpoint slot for `'r`;
            the session context stores the validated lane and a cluster pointer
            that remains pinned for the session kit lifetime. */
            unsafe {
                core::mem::transmute(crate::endpoint::session::SessionCtx::<'cfg, T>::new(
                    session_wire_lane,
                    &*(self as *const Self),
                ))
            };

        /* SAFETY: the endpoint lease returned `dst` and `arena_storage` for a
        live slot/generation. `init_empty_from_compiled` initializes the whole
        endpoint image before the public handle can be returned. */
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
            /* SAFETY: the caller still owns the endpoint lease while attach is
            unpublished. Disable public Drop's release
            path so rollback remains single-owner and explicit. */
            unsafe {
                (*dst).public_generation = 0;
                (*dst).public_slot_ownership =
                    crate::endpoint::kernel::PublicSlotOwnership::Borrowed;
            }
            /* SAFETY: endpoint initialization failed before publication, so
            attach still owns `dst` and drops the partially initialized endpoint
            image exactly once. */
            unsafe {
                core::ptr::drop_in_place(dst);
            }
            return Err(err);
        }

        /* SAFETY: endpoint attach owns `dst` until this finalization call.
        Finish synchronizes initialized lane state before the public endpoint
        handle can be returned. */
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
    fn required_assoc_slots_for_endpoint<const ROLE: u8>(
        rv: &crate::rendezvous::core::Rendezvous<'cfg, 'cfg, T>,
        sid: SessionId,
        role_image: RoleImageSlice<ROLE>,
        logical_lane_count: usize,
    ) -> usize {
        let mut required = rv.active_lane_attachment_count();
        if !rv.has_lane_attachment(sid, Lane::new(0)) {
            required += 1;
        }
        let mut logical_idx = 1usize;
        while logical_idx < logical_lane_count {
            if role_image.has_active_lane(logical_idx)
                && !rv.has_lane_attachment(sid, Lane::new(logical_idx as u32))
            {
                required += 1;
            }
            logical_idx += 1;
        }
        required
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
            /* SAFETY: allocation preview leases the target rendezvous through
            session storage only long enough to compute endpoint storage
            requirements and reserve a live endpoint slot. */
            unsafe {
                let logical_lane_count = role_image.logical_lane_count().max(1);
                self.with_storage_mut(|core| {
                    let rv = core
                        .locals
                        .get_mut_checked(&rv_id)
                        .map_err(Self::map_rendezvous_access_error)
                        .map_err(AttachError::cluster)?;
                    let required_assoc_slots = Self::required_assoc_slots_for_endpoint(
                        rv,
                        sid,
                        role_image,
                        logical_lane_count,
                    );
                    rv.ensure_core_lane_storage_for_assoc_entries(
                        logical_lane_count,
                        required_assoc_slots,
                    )
                    .map_err(|scope| AttachError::cluster(ClusterError::resource_exhausted(scope)))
                })?;
                let storage_layout = Self::public_endpoint_storage_requirement(role_image);
                let resident_budget = Self::public_endpoint_resident_budget(role_image);
                let (slot, generation, dst) = self
                    .allocate_public_endpoint_storage_for_rv::<ROLE>(
                        rv_id,
                        sid,
                        storage_layout.total_bytes,
                        storage_layout.total_align,
                        resident_budget,
                    )?;
                let arena_storage = dst.cast::<u8>().add(storage_layout.arena_offset);
                let public_ops = crate::runtime::SessionKit::<'cfg, T>::endpoint_ops::<ROLE>();
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
                    let release_result = self.with_storage_mut(|core| {
                        if let Some(rv) = core.locals.get_mut(&rv_id) {
                            rv.release_endpoint_lease(slot, generation)?;
                        }
                        Ok(())
                    });
                    release_result.map_err(|scope| {
                        AttachError::cluster(ClusterError::resource_exhausted(scope))
                    })?;
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
