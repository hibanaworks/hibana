use super::{
    AttachError, EndpointInitArgs, EndpointLeaseId, Lane, PublicEndpointStorageRequest,
    RendezvousId, RoleImageSlice, SessionCluster, SessionId,
};

struct SecondaryEndpointLaneAttach<'lease, const ROLE: u8, T>
where
    T: crate::transport::Transport + 'lease,
{
    dst: *mut crate::endpoint::kernel::ResidentActor<'lease, T>,
    rv_id: RendezvousId,
    sid: SessionId,
    role_image: RoleImageSlice<ROLE>,
    logical_lane_count: usize,
    occupied_lane_index: usize,
    logical_role: u8,
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
            role_image,
            logical_lane_count,
            occupied_lane_index,
            logical_role,
        } = attach;
        let mut logical_idx = 0usize;
        while logical_idx < logical_lane_count {
            if logical_idx == occupied_lane_index
                || !role_image
                    .descriptor()
                    .has_view_lane(logical_role, logical_idx)
            {
                logical_idx += 1;
                continue;
            }

            let physical_lane = Lane::new(logical_idx as u32);
            let lease = self
                .lease_port(rv_id, sid, physical_lane, logical_role)
                .map_err(AttachError::from)?;
            let access = lease.into_port_guard().map_err(AttachError::from)?;
            /* SAFETY: secondary-lane attach owns the endpoint slot before
            publication. `logical_idx` selects initialized port/guard columns,
            and the lane lease has validated the physical lane for this session. */
            unsafe {
                crate::endpoint::kernel::endpoint_init::write_port_slot(
                    dst,
                    logical_role,
                    logical_idx,
                    access.port,
                );
                crate::endpoint::kernel::endpoint_init::write_guard_slot(
                    dst,
                    logical_role,
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
        let logical_lane_count = role_image.descriptor().view_lane_slot_count(ROLE);
        let primary_lane_index = Self::primary_endpoint_lane_index(role_image, logical_lane_count);
        let session_lane_index = 0usize;
        let session_wire_lane = Lane::new(session_lane_index as u32);
        let session_lease = self
            .lease_port(rv_id, sid, session_wire_lane, ROLE)
            .map_err(AttachError::from)?;
        let session_access = session_lease.into_port_guard().map_err(AttachError::from)?;
        let session: crate::endpoint::session::SessionCtx<'r, T> =
            /* SAFETY: endpoint attach owns the public endpoint slot for `'r`;
            both the cluster and rendezvous remain pinned for the session kit
            lifetime. Only their borrow lifetimes are narrowed. */
            unsafe {
                let cluster = &*core::ptr::from_ref(self).cast::<
                    crate::session::cluster::core::SessionCluster<'r, T>,
                >();
                let rendezvous = &*core::ptr::from_ref(crate::invariant_some(self.get_local(&rv_id)))
                    .cast::<crate::rendezvous::core::Rendezvous<'r, 'r, T>>();
                crate::endpoint::session::SessionCtx::new(cluster, rendezvous)
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
                    initial_role: ROLE,
                    sid,
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
                ROLE,
                session_lane_index,
                session_access.port,
            );
            crate::endpoint::kernel::endpoint_init::write_guard_slot(
                dst,
                ROLE,
                session_lane_index,
                session_access.lane_guard,
            );
        }
        let init_result = (|| {
            // Ports are physical storage, not operation rights. Initialize
            // every projected candidate before publication; issuing another
            // initial view then needs no callback or partial attach.
            for logical_role in role_image.descriptor().logical_views().roles() {
                if logical_role != ROLE {
                    let access = self
                        .lease_port(rv_id, sid, session_wire_lane, logical_role)?
                        .into_port_guard()?;
                    // SAFETY: all view records are initialized and unpublished.
                    unsafe {
                        crate::endpoint::kernel::endpoint_init::write_port_slot(
                            dst,
                            logical_role,
                            session_lane_index,
                            access.port,
                        );
                        crate::endpoint::kernel::endpoint_init::write_guard_slot(
                            dst,
                            logical_role,
                            session_lane_index,
                            access.lane_guard,
                        );
                    }
                }
                self.attach_secondary_endpoint_lanes::<ROLE>(SecondaryEndpointLaneAttach {
                    dst,
                    rv_id,
                    sid,
                    role_image,
                    logical_lane_count: role_image.descriptor().view_lane_slot_count(logical_role),
                    occupied_lane_index: session_lane_index,
                    logical_role,
                })?;
            }
            Ok::<(), AttachError>(())
        })();

        if let Err(err) = init_result {
            /* SAFETY: the caller still owns the endpoint lease while attach is
            unpublished. Disable public Drop's release
            path so reservation abort remains single-owner and explicit. */
            unsafe {
                (*dst).context.public_generation.set(0);
                (*dst)
                    .context
                    .public_slot_ownership
                    .set(crate::endpoint::kernel::PublicSlotOwnership::Borrowed);
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
            crate::endpoint::kernel::endpoint_init::finish_init(dst, ROLE);
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
        let initial_actor = program.role_image_ref().program().initial_actor_for(ROLE);
        // Reserve at the physical owner before allocation or transport
        // callbacks can reenter attachment. This is independent of the local
        // session/actor registry, so another runtime cannot issue the same
        // binding. Only an unpublished failure returns this reservation.
        let initial_binding = if let Some(actor) = initial_actor {
            let rv = self
                .locals()
                .get_checked(&rv_id)
                .map_err(Self::map_rendezvous_access_error)
                .map_err(AttachError::cluster)?;
            if rv.published_actor_for(sid, actor).is_none() {
                return Err(AttachError::initial_actor_unavailable(ROLE, actor));
            }
            Some(
                rv.transport()
                    .initial_endpoint_binding(sid, ROLE)
                    .and_then(crate::transport::EndpointBinding::reserve)
                    .ok_or(AttachError::endpoint_binding_unavailable(ROLE))?,
            )
        } else {
            None
        };
        match self.ensure_role_image_slice(rv_id, program) {
            Ok(role_image) =>
            /* SAFETY: allocation preview leases the target rendezvous through
            session storage only long enough to compute endpoint storage
            requirements and reserve a live endpoint slot. */
            unsafe {
                let logical_lane_count = role_image.logical_lane_count();
                let rv = self
                    .locals()
                    .get_checked(&rv_id)
                    .map_err(Self::map_rendezvous_access_error)
                    .map_err(AttachError::cluster)?;
                if let Some((slot, generation)) =
                    rv.published_actor_for(sid, role_image.descriptor().role())
                {
                    if rv.session_fault(sid).is_some() {
                        return Err(crate::rendezvous::error::RendezvousError::SessionPoisoned {
                            sid,
                        }
                        .into());
                    }
                    let bound = crate::invariant_some(rv.endpoint_session_program(sid));
                    if !bound.same_image(role_image.descriptor().program()) {
                        return Err(
                            super::ClusterError::SessionProgramMismatch { sid: sid.raw() }.into(),
                        );
                    }
                    if rv.session_membership_is_sealed(sid) {
                        return Err(super::ClusterError::SessionMembershipSealed {
                            sid: sid.raw(),
                        }
                        .into());
                    }
                    let actor = self.public_actor_ptr(rv_id, slot, generation);
                    if !actor.as_ref().issue_initial_view(ROLE) {
                        return Err(super::ClusterError::RendezvousBusy { id: rv_id.raw() }.into());
                    }
                    if let Some(reservation) = initial_binding {
                        reservation.commit();
                    }
                    return Ok((slot, generation));
                }
                // A carrier callback may have dropped the initial actor after
                // the preflight. A resource view must join that existing actor,
                // never create a replacement allocation on its own. Dropping
                // the unpublished reservation preserves the original issuer.
                if let Some(actor) = initial_actor {
                    return Err(AttachError::initial_actor_unavailable(ROLE, actor));
                }
                let required_assoc_slots = Self::required_assoc_slots_for_endpoint(
                    rv,
                    sid,
                    role_image,
                    logical_lane_count,
                );
                let storage_layout = Self::public_endpoint_storage_requirement(role_image);
                let (slot, generation, dst) =
                    self.allocate_public_endpoint_storage_for_rv(PublicEndpointStorageRequest {
                        rv_id,
                        sid,
                        required_bytes: storage_layout.total_bytes,
                        required_align: storage_layout.total_align,
                        logical_lane_count,
                        required_assoc_slots,
                        role_descriptor: role_image.descriptor(),
                    })?;
                let arena_storage = dst.cast::<u8>().add(storage_layout.arena_offset);
                let public_ops = crate::runtime::SessionKit::<'cfg, T>::endpoint_ops();
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
                    crate::invariant_some(self.get_local(&rv_id))
                        .abort_endpoint_lease_reservation(slot, generation);
                    return Err(err);
                }
                self.publish_public_endpoint_slot(rv_id, slot, generation);
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
