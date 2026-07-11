use super::{
    AttachError, ClusterError, CompiledProgramRef, EndpointLeaseId, Lane, LaneLease, LeaseError,
    PublicEndpointStorageLayout, PublicEndpointStorageRequest, RegisterRendezvousError, Rendezvous,
    RendezvousError, RendezvousId, ResourceScope, RoleImageSlice, SessionId, SessionStorage,
};

// # Unsafe Owner Contract
//
// This file owns the `SessionCluster` interior-mutability boundary. The
// cluster is local-only and uses explicit interior owners so resident endpoint
// leases can hold shared cluster references without any later `&mut
// SessionStorage`. Registry transitions are serialized by affine leases and
// owner cells, and raw resident storage pointers are tied to cluster generation,
// endpoint-slot, and graph-storage metadata before they are exposed to endpoint
// construction.

/// SessionCluster - Local session coordinator with interior mutability.
///
/// Uses interior owner cells to allow `&self` methods while preserving the
/// pinned references held by live endpoints.
///
/// # Safety
///
/// No safe operation reconstructs a mutable reference to the whole session
/// storage or rendezvous after publication.
pub(crate) struct SessionCluster<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    /// Session state guarded by interior mutability.
    pub(crate) storage: core::cell::UnsafeCell<SessionStorage<'cfg, T>>,
    _local_only: crate::local::LocalOnly,
}

impl<'cfg, T> SessionCluster<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: `SessionKitStorage::init` passes an unpublished
        `SessionCluster` cell. Session storage and the local-only marker are
        written before the cluster reference is returned. */
        unsafe {
            SessionStorage::<T>::init_empty(core::ptr::addr_of_mut!((*dst).storage).cast());
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    pub(crate) fn storage_ref_ptr(&self) -> *const SessionStorage<'cfg, T> {
        self.storage.get() as *const SessionStorage<'cfg, T>
    }

    #[inline]
    fn storage_ref(&self) -> &SessionStorage<'cfg, T> {
        /* SAFETY: session storage is initialized before the cluster is exposed
        and never mutably referenced after publication. */
        unsafe { &*self.storage_ref_ptr() }
    }

    #[inline]
    pub(in crate::session::cluster::core) fn locals(
        &self,
    ) -> &crate::session::lease::core::RendezvousTable<'cfg, T> {
        /* SAFETY: the registry is the dedicated interior owner inside pinned
        session storage; its public operations use affine leases and cells. */
        unsafe { &*self.storage_ref().locals.get() }
    }

    #[inline]
    pub(crate) fn map_rendezvous_access_error(error: LeaseError) -> ClusterError {
        match error {
            LeaseError::RendezvousUnregistered(id) => {
                ClusterError::RendezvousUnregistered { id: id.raw() }
            }
            LeaseError::AlreadyLeased(id) => ClusterError::RendezvousBusy { id: id.raw() },
        }
    }

    #[inline]
    pub(in crate::session::cluster::core) fn public_endpoint_storage_requirement<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
    ) -> PublicEndpointStorageLayout {
        let arena_layout = role_image.endpoint_arena_layout();
        let storage_layout = crate::endpoint::kernel::cursor_endpoint_storage_layout::<0, T>(
            &arena_layout,
            role_image.endpoint_lane_slot_count(),
        );
        PublicEndpointStorageLayout {
            total_bytes: storage_layout.total_bytes(),
            total_align: storage_layout.total_align(),
            arena_offset: storage_layout.arena_offset(),
        }
    }

    #[inline]
    pub(crate) fn public_endpoint_resident_budget<const ROLE: u8>(
        compiled_role: RoleImageSlice<ROLE>,
    ) -> crate::rendezvous::core::EndpointResidentBudget {
        crate::rendezvous::core::EndpointResidentBudget::with_route_storage(
            compiled_role.route_table_frame_slots(),
            compiled_role.route_table_lane_slots(),
            crate::rendezvous::core::Rendezvous::<T>::frontier_workspace_guard_bytes(
                compiled_role.descriptor().frontier_scratch_layout(),
            ),
        )
    }

    #[inline(never)]
    pub(in crate::session::cluster::core) fn allocate_public_endpoint_storage_for_rv<
        'r,
        const ROLE: u8,
    >(
        &self,
        request: PublicEndpointStorageRequest,
    ) -> Result<
        (
            EndpointLeaseId,
            u32,
            *mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>,
        ),
        ClusterError,
    >
    where
        'cfg: 'r,
    {
        let PublicEndpointStorageRequest {
            rv_id,
            sid,
            required_bytes,
            required_align,
            logical_lane_count,
            required_assoc_slots,
            resident_budget,
        } = request;
        let (slot, generation, offset, _len) =
            self.locals().allocate_endpoint_lease_for_session_role(
                rv_id,
                sid,
                ROLE,
                required_bytes,
                required_align,
                resident_budget,
            )?;
        let rv = self
            .locals()
            .get_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)?;
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        let storage_end = crate::invariant_some(offset.checked_add(required_bytes));
        if storage_end > slab_len {
            crate::invariant();
        }
        if let Err(resource) = rv.ensure_endpoint_resident_capacity() {
            rv.abort_endpoint_lease_reservation(slot, generation);
            return Err(ClusterError::resource_exhausted(resource));
        }
        if let Err(resource) =
            rv.ensure_core_lane_storage_for_assoc_entries(logical_lane_count, required_assoc_slots)
        {
            rv.abort_endpoint_lease_reservation(slot, generation);
            return Err(ClusterError::resource_exhausted(resource));
        }
        Ok((
            slot,
            generation,
            /* SAFETY: `offset..offset+required_bytes` was checked against the
            rendezvous slab length and the endpoint lease was confirmed live. */
            unsafe { slab_ptr.add(offset) }
                .cast::<crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>>(),
        ))
    }

    pub(crate) fn public_endpoint_header_ptr(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> core::ptr::NonNull<crate::endpoint::carrier::KernelEndpointHeader<'cfg>> {
        let rv = crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        );
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        let (offset, len) = crate::invariant_some(rv.endpoint_lease_storage(slot, generation));
        let storage_end = crate::invariant_some(offset.checked_add(len));
        if len == 0 || storage_end > slab_len {
            crate::invariant();
        }
        let ptr = /* SAFETY: the live endpoint lease owns `offset..storage_end`,
        which was checked inside the pinned rendezvous slab. */ unsafe {
            slab_ptr
                .add(offset)
                .cast::<crate::endpoint::carrier::KernelEndpointHeader<'cfg>>()
        };
        crate::invariant_some(core::ptr::NonNull::new(ptr))
    }

    #[inline]
    pub(crate) fn session_fault(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
        sid: SessionId,
    ) -> Option<crate::rendezvous::SessionFaultKind> {
        crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        )
        .session_fault(sid)
    }

    #[inline]
    pub(crate) fn poison_session<const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
        sid: SessionId,
        cause: crate::rendezvous::SessionFaultKind,
    ) -> crate::rendezvous::SessionFaultKind {
        let rv = crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        );
        rv.poison_session(sid, ROLE, cause)
    }

    fn ensure_compiled_program_ref<const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<&'static CompiledProgramRef, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let compiled = program.role_image_ref().program;
        self.locals()
            .get_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)
            .map_err(AttachError::cluster)?;
        Ok(compiled)
    }

    #[inline(never)]
    pub(crate) fn ensure_role_image_slice<const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<RoleImageSlice<ROLE>, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let compiled = program.role_image_ref();
        self.locals()
            .get_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)
            .map_err(AttachError::cluster)?;
        Ok(RoleImageSlice::from_resident(compiled))
    }

    pub(crate) fn release_public_endpoint_slot_owned(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        )
        .release_endpoint_lease(slot, generation);
    }

    pub(crate) fn replace_public_endpoint_waiter(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
        replacement: core::task::Waker,
    ) -> Option<core::task::Waker> {
        crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        )
        .replace_endpoint_waiter(slot, generation, replacement)
    }

    pub(crate) fn take_public_endpoint_waiter(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<core::task::Waker> {
        crate::invariant_some(
            self.locals()
                .published_endpoint_owner(rv_id, slot, generation),
        )
        .take_endpoint_waiter(slot, generation)
    }

    pub(crate) fn publish_public_endpoint_slot(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        crate::invariant_some(self.get_local(&rv_id)).publish_endpoint_lease(slot, generation);
    }

    pub(crate) fn with_resident_program_ref<const ROLE: u8, P, F, R, E>(
        &self,
        rv_id: RendezvousId,
        program: &P,
        f: F,
    ) -> Result<R, E>
    where
        E: From<ClusterError>,
        P: crate::global::RoleProgramView<ROLE>,
        F: FnOnce(&'static CompiledProgramRef) -> Result<R, E>,
    {
        let compiled = self
            .ensure_compiled_program_ref(rv_id, program)
            .map_err(|err| E::from(crate::invariant_some(err.cluster_cause())))?;
        f(compiled)
    }

    /// Register a local rendezvous in its caller-owned slab.
    ///
    /// The registry owns the initialized intrusive header and transport for the
    /// slab lifetime. Endpoint and port borrows must end before registry teardown.
    ///
    /// Returns the RendezvousId on success.
    ///
    /// # Errors
    ///
    /// Returns `ClusterError::resource_exhausted(ResourceScope::RendezvousTable)`
    /// when the identifier space or caller slab cannot hold another rendezvous.
    pub(crate) fn register_rendezvous(
        &self,
        resources: crate::runtime_core::resources::RuntimeResources<'cfg>,
        transport: T,
    ) -> Result<RendezvousId, ClusterError> {
        match self
            .locals()
            .register_local_from_resources_auto(resources, transport)
        {
            Ok(id) => Ok(id),
            Err(
                RegisterRendezvousError::CapacityExceeded
                | RegisterRendezvousError::StorageExhausted,
            ) => Err(ClusterError::resource_exhausted(
                ResourceScope::RendezvousTable,
            )),
        }
    }

    /// Get an available local rendezvous by ID.
    pub(crate) fn get_local(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        self.locals().get(id)
    }

    /// **Acquire a lane lease (RAII handle bound to this cluster).**
    ///
    /// Returns a `LaneLease` that borrows this cluster and automatically releases
    /// the lane on Drop.
    ///
    /// # Safety Invariants
    ///
    /// - the lease core marks the rendezvous entry active while the lease lives
    /// - conflicting registry operations fail while that active marker is set
    /// - callback-capable endpoint cleanup must not run while this lease is held
    ///
    pub(crate) fn lease_port<'lease>(
        &'lease self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
    ) -> Result<LaneLease<'lease, 'cfg, T>, RendezvousError>
    where
        'cfg: 'lease,
    {
        let lease = match self.locals().lease(rv_id) {
            Ok(lease) => lease,
            Err(LeaseError::RendezvousUnregistered(_)) => {
                return Err(RendezvousError::LaneOutOfRange { lane });
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };
        lease.with_rendezvous(|rv| rv.activate_lane_attachment(sid, lane))?;

        let active = &self.storage_ref().active_leases;

        let next = crate::invariant_some(active.get().checked_add(1));
        active.set(next);

        let brand = lease.brand();

        Ok(LaneLease::new(
            lease, sid, lane, role, role_count, active, brand,
        ))
    }
}
