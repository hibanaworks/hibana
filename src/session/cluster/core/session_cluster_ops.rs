use super::{
    AttachError, ClusterError, CompiledProgramRef, EndpointLeaseId, Lane, LaneLease, LeaseError,
    PublicEndpointStorageLayout, RegisterRendezvousError, Rendezvous, RendezvousError,
    RendezvousId, ResourceScope, RoleClaimError, RoleImageSlice, SessionId, SessionStorage,
};

// # Unsafe Owner Contract
//
// This file owns the `SessionCluster` interior-mutability boundary. The
// cluster is local-only and uses `UnsafeCell` so resident endpoint leases can
// hold shared cluster references while cluster operations still mutate the
// session table. All mutable access must pass through
// the local `with_storage_mut` closure, and raw resident
// storage pointers returned here are tied to cluster-owned generation,
// endpoint-slot, and graph-storage metadata before they are exposed to endpoint
// construction.

/// SessionCluster - Local session coordinator with interior mutability.
///
/// Uses `UnsafeCell` to allow `&self` methods while maintaining mutable internal state.
/// This enables `LaneLease` to hold `PhantomData<&'cluster SessionCluster>` (shared reference)
/// without blocking other cluster operations.
///
/// # Safety
///
/// All mutable access to the storage table goes through `with_storage_mut()`,
/// which centralizes:
/// - the single mutable access boundary for resident registry state;
/// - documented invariants (see `SessionStorage`);
/// - TAP event monitoring for lane lifecycle.
///
/// These helpers are not reentrant guards. Crate-internal callers must keep
/// endpoint cleanup, descriptor publication, and other callback-capable work in
/// their typed post-mutation phases so no nested mutable borrow is created.
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
    fn storage_ptr(&self) -> *mut SessionStorage<'cfg, T> {
        self.storage.get()
    }

    #[inline]
    pub(crate) fn storage_ref_ptr(&self) -> *const SessionStorage<'cfg, T> {
        self.storage.get() as *const SessionStorage<'cfg, T>
    }

    /// Runs a mutation inside the session storage owner.
    pub(crate) fn with_storage_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut SessionStorage<'cfg, T>) -> R,
    {
        /* SAFETY: the session table lives in pinned cluster-owned storage, and crate-internal mutation phases are structured so no nested session borrow is active for the closure duration. */
        unsafe { f(&mut *self.storage_ptr()) }
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
    pub(crate) fn allocate_public_endpoint_storage_for_rv<'r, const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        required_bytes: usize,
        required_align: usize,
        resident_budget: crate::rendezvous::core::EndpointResidentBudget,
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
        let core = /* SAFETY: `SessionCluster` owns the pinned
        `SessionStorage` cell. This attach allocation takes the cluster storage
        mutation path before leasing the target rendezvous entry. */ unsafe { &mut *self.storage_ptr() };
        let rv = core
            .locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)?;
        if let Err(resource) = rv.ensure_endpoint_resident_budget(resident_budget) {
            return Err(ClusterError::resource_exhausted(resource));
        }
        let (slot, generation, offset, _len) =
            (/* SAFETY: the leased rendezvous owns endpoint lease allocation.
            `required_bytes/align` and `resident_budget` are checked before a
            live endpoint slot and storage offset are returned. */unsafe {
                rv.allocate_endpoint_lease(required_bytes, required_align, resident_budget)
            })
            .map_err(ClusterError::resource_exhausted)?;
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        let Some(storage_end) = offset.checked_add(required_bytes) else {
            rv.release_endpoint_lease(slot, generation)
                .map_err(ClusterError::resource_exhausted)?;
            return Err(ClusterError::resource_exhausted(
                ResourceScope::EndpointBounds,
            ));
        };
        if storage_end > slab_len {
            rv.release_endpoint_lease(slot, generation)
                .map_err(ClusterError::resource_exhausted)?;
            return Err(ClusterError::resource_exhausted(
                ResourceScope::EndpointBounds,
            ));
        }
        if let Err(resource) = rv.ensure_endpoint_lease_live(slot, generation) {
            rv.release_endpoint_lease(slot, generation)
                .map_err(ClusterError::resource_exhausted)?;
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

    fn public_endpoint_storage_raw_ptr(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<*mut ()> {
        let rv = self.get_local(&rv_id)?;
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        let (offset, len) = rv.endpoint_lease_storage(slot, generation)?;
        let storage_end = offset.checked_add(len)?;
        if len == 0 || storage_end > slab_len {
            return None;
        }
        Some(
            /* SAFETY: endpoint lease lookup returned `offset,len` for the
            requested live slot/generation, and the computed range is inside
            the rendezvous slab. */
            unsafe { slab_ptr.add(offset).cast() },
        )
    }

    pub(crate) fn public_endpoint_header_ptr(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<core::ptr::NonNull<crate::endpoint::carrier::KernelEndpointHeader<'cfg>>> {
        core::ptr::NonNull::new(
            self.public_endpoint_storage_raw_ptr(rv_id, slot, generation)?
                .cast::<crate::endpoint::carrier::KernelEndpointHeader<'cfg>>(),
        )
    }

    #[inline]
    pub(crate) fn session_fault(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
    ) -> Option<crate::rendezvous::SessionFaultKind> {
        self.with_storage_mut(|core| {
            core.locals
                .get_mut(&rv_id)
                .and_then(|rv| rv.session_fault(sid))
        })
    }

    #[inline]
    pub(crate) fn poison_session(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        cause: crate::rendezvous::SessionFaultKind,
    ) -> crate::rendezvous::SessionFaultKind {
        self.with_storage_mut(|core| {
            let Some(rv) = core.locals.get_mut(&rv_id) else {
                crate::invariant();
            };
            rv.poison_session(sid, cause)
        })
    }

    #[inline]
    pub(crate) fn register_session_waiter(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        waker: &core::task::Waker,
    ) {
        self.with_storage_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.register_session_waiter(sid, lane, waker);
            }
        });
    }

    #[inline]
    pub(crate) fn clear_session_waiter(&self, rv_id: RendezvousId, sid: SessionId, lane: Lane) {
        self.with_storage_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.clear_session_waiter(sid, lane);
            }
        });
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
        let core = /* SAFETY: the cluster owns the pinned session storage; this
        validation borrows it mutably only long enough to check the rendezvous
        lease state for `rv_id`. */ unsafe { &mut *self.storage_ptr() };
        core.locals
            .get_mut_checked(&rv_id)
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
        let core = /* SAFETY: the cluster owns the pinned session storage; this
        role-image validation mutably borrows it only to reject missing or
        already leased rendezvous entries. */ unsafe { &mut *self.storage_ptr() };
        core.locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)
            .map_err(AttachError::cluster)?;
        Ok(RoleImageSlice::from_resident(compiled))
    }

    pub(crate) unsafe fn release_public_endpoint_slot(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Result<(), ResourceScope> {
        self.with_storage_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.release_endpoint_lease(slot, generation)?;
            }
            Ok(())
        })
    }

    #[inline]
    pub(crate) fn release_public_endpoint_slot_owned(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe {
            if self
                .release_public_endpoint_slot(rv_id, slot, generation)
                .is_err()
            {
                crate::invariant();
            }
        }
    }

    #[inline]
    pub(crate) fn claim_session_role(
        &self,
        sid: SessionId,
        role: u8,
        rv_id: RendezvousId,
    ) -> Result<(), ClusterError> {
        self.with_storage_mut(|core| {
            core.locals
                .claim_session_role(sid, role, rv_id)
                .map_err(|error| match error {
                    RoleClaimError::RendezvousUnregistered(id) => {
                        ClusterError::RendezvousUnregistered { id: id.raw() }
                    }
                    RoleClaimError::RendezvousMismatch { expected, actual } => {
                        ClusterError::RendezvousMismatch { expected, actual }
                    }
                    RoleClaimError::AlreadyClaimed(id) => {
                        ClusterError::RendezvousBusy { id: id.raw() }
                    }
                    RoleClaimError::CapacityExceeded => {
                        ClusterError::resource_exhausted(ResourceScope::RendezvousTable)
                    }
                })
        })
    }

    #[inline]
    pub(crate) fn release_session_role_claim(&self, sid: SessionId, role: u8, rv_id: RendezvousId) {
        let released =
            self.with_storage_mut(|core| core.locals.release_session_role_claim(sid, role, rv_id));
        if !released {
            crate::invariant();
        }
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
        F: FnOnce(&CompiledProgramRef) -> Result<R, E>,
    {
        let compiled = self
            .ensure_compiled_program_ref(rv_id, program)
            .map_err(|err| E::from(crate::invariant_some(err.cluster_cause())))?;
        f(compiled)
    }

    /// Register a local Rendezvous instance to the cluster (takes ownership).
    ///
    /// SessionCluster takes ownership of the Rendezvous, ensuring proper RAII:
    /// - Drop order: SessionCluster → Rendezvous → LaneLease
    /// - No self-referential lifetime issues
    /// - Type-level proof of affine resource management
    ///
    /// Returns the RendezvousId on success.
    ///
    /// # Errors
    ///
    /// Returns `ClusterError::resource_exhausted(ResourceScope::RendezvousTable)` if the cluster is full.
    /// Build and register a local rendezvous from runtime config + transport.
    ///
    /// Public callers should use this entrypoint instead of constructing
    /// rendezvous internals directly.
    pub(crate) fn register_rendezvous(
        &self,
        resources: crate::runtime_core::resources::RuntimeResources<'cfg>,
        transport: T,
    ) -> Result<RendezvousId, ClusterError> {
        self.with_storage_mut(|core| {
            match core
                .locals
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
        })
    }

    /// Get a local Rendezvous by ID.
    ///
    /// # Safety
    ///
    /// Returns a shared reference from cluster-owned storage. Callers must keep
    /// this borrow out of phases that can enter `with_storage_mut`.
    pub(crate) fn get_local(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        // SAFETY: We're returning a shared reference from UnsafeCell.
        // This is safe because:
        // - The reference is borrowed from `&self`, so it can't outlive the cluster
        // - Caller must not call mutable methods while holding this reference
        // - This pattern is documented in SessionCluster's safety contract
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe { (*self.storage_ref_ptr()).locals.get(id) }
    }

    /// **Acquire a lane lease (RAII handle bound to this cluster).**
    ///
    /// Returns a `LaneLease` that borrows this cluster and automatically releases
    /// the lane on Drop.
    ///
    /// # Safety Invariants
    ///
    /// - the lease core marks the rendezvous entry active while the lease lives
    /// - normal mutable rendezvous lookups fail while that active marker is set
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
        // SAFETY: `SessionCluster` is local-only. This phase enters the
        // cluster-owned session table solely to mark one rendezvous entry active
        // and move the resulting lease out; callers must not nest this in a
        // `with_storage_mut` phase.
        let core = unsafe { &mut *self.storage_ptr() };

        let mut lease = match core.locals.lease(rv_id) {
            Ok(lease) => lease,
            Err(LeaseError::RendezvousUnregistered(_)) => {
                return Err(RendezvousError::LaneOutOfRange { lane });
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };

        let active = &core.active_leases;

        let current = active.get();
        active.set(current + 1);

        let brand = lease.brand();

        Ok(LaneLease::new(
            lease, sid, lane, role, role_count, active, brand,
        ))
    }
}
