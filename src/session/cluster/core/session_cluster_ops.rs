use super::{
    AttachError, ClusterError, CompiledProgramRef, EndpointLeaseId, Lane, LaneLease, LeaseError,
    PublicEndpointStorageLayout, RegisterRendezvousError, Rendezvous, RendezvousError,
    RendezvousId, ResolverCore, ResourceScope, RoleImageSlice, SessionId, SessionStorage,
};

// # Unsafe Owner Contract
//
// This file owns the `SessionCluster` interior-mutability boundary. The
// cluster is local-only and uses `UnsafeCell` so resident endpoint leases can
// hold shared cluster references while cluster operations still mutate the
// session table and dynamic resolver table. All mutable access must pass through
// the local `with_storage_mut` / `with_resolvers_mut` closures, and raw resident
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
/// All mutable access to the storage or resolver tables goes through
/// `with_storage_mut()` / `with_resolvers_mut()`, which centralize:
/// - the single mutable access boundary for the selected table;
/// - documented invariants (see `SessionStorage`);
/// - TAP event monitoring for lane lifecycle.
///
/// These helpers are not reentrant guards. Crate-internal callers must keep
/// endpoint cleanup, descriptor publication, and other callback-capable work in
/// their typed post-mutation phases so no nested mutable borrow is created.
pub(crate) struct SessionCluster<'cfg, T, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    /// Session state guarded by interior mutability.
    pub(crate) storage: core::cell::UnsafeCell<SessionStorage<'cfg, T, C, MAX_RV>>,
    /// Dynamic resolver table separated from core storage state.
    resolvers: core::cell::UnsafeCell<ResolverCore<'cfg, MAX_RV>>,
    _local_only: crate::local::LocalOnly,
}

impl<'cfg, T, C, const MAX_RV: usize> SessionCluster<'cfg, T, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            SessionStorage::<T, C, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).storage).cast(),
            );
            ResolverCore::<'cfg, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).resolvers).cast(),
            );
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    fn storage_ptr(&self) -> *mut SessionStorage<'cfg, T, C, MAX_RV> {
        self.storage.get()
    }

    #[inline]
    pub(crate) fn storage_ref_ptr(&self) -> *const SessionStorage<'cfg, T, C, MAX_RV> {
        self.storage.get() as *const SessionStorage<'cfg, T, C, MAX_RV>
    }

    #[inline]
    pub(crate) fn resolvers_ptr(&self) -> *mut ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get()
    }

    #[inline]
    pub(crate) fn resolvers_ref_ptr(&self) -> *const ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get() as *const ResolverCore<'cfg, MAX_RV>
    }

    /// Internal helper to access mutable session state (NOT PUBLIC).
    pub(crate) fn with_storage_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut SessionStorage<'cfg, T, C, MAX_RV>) -> R,
    {
        /* SAFETY: the session table lives in pinned cluster-owned storage, and crate-internal mutation phases are structured so no nested session borrow is active for the closure duration. */
        unsafe { f(&mut *self.storage_ptr()) }
    }

    /// Internal helper to access mutable resolver state.
    pub(crate) fn with_resolvers_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ResolverCore<'cfg, MAX_RV>) -> R,
    {
        /* SAFETY: the resolver table lives in pinned cluster-owned storage, and crate-internal mutation phases are structured so no nested resolver borrow is active for the closure duration. */
        unsafe { f(&mut *self.resolvers_ptr()) }
    }

    #[inline]
    pub(crate) fn map_rendezvous_access_error(error: LeaseError) -> ClusterError {
        match error {
            LeaseError::UnknownRendezvous(id) => ClusterError::RendezvousMissing { id: id.raw() },
            LeaseError::AlreadyLeased(id) => ClusterError::RendezvousBusy { id: id.raw() },
        }
    }

    #[inline]
    pub(in crate::session::cluster::core) fn public_endpoint_storage_requirement<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
    ) -> PublicEndpointStorageLayout {
        let arena_layout = role_image.endpoint_arena_layout();
        let storage_layout =
            crate::endpoint::kernel::cursor_endpoint_storage_layout::<0, T, C, MAX_RV>(
                &arena_layout,
                role_image.endpoint_lane_slot_count(),
            );
        PublicEndpointStorageLayout {
            total_bytes: storage_layout.total_bytes(),
            total_align: storage_layout.total_align(),
            header_bytes: storage_layout.header_bytes(),
            port_slots_bytes: storage_layout.port_slots_bytes(),
            guard_slots_bytes: storage_layout.guard_slots_bytes(),
            header_padding_bytes: storage_layout
                .arena_offset()
                .checked_sub(
                    storage_layout
                        .header_bytes()
                        .checked_add(storage_layout.port_slots_bytes())
                        .and_then(|bytes| bytes.checked_add(storage_layout.guard_slots_bytes()))
                        .expect("invariant"),
                )
                .expect("invariant"),
            arena_offset: storage_layout.arena_offset(),
            arena_bytes: storage_layout.arena_bytes(),
            arena_align: storage_layout.arena_align(),
        }
    }

    #[inline]
    pub(crate) fn public_endpoint_resident_budget<const ROLE: u8>(
        compiled_role: RoleImageSlice<ROLE>,
    ) -> crate::rendezvous::core::EndpointResidentBudget {
        crate::rendezvous::core::EndpointResidentBudget::with_route_storage(
            compiled_role.route_table_frame_slots(),
            compiled_role.route_table_lane_slots(),
            crate::rendezvous::core::Rendezvous::<T, C>::frontier_workspace_guard_bytes(
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
            *mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
        ),
        ClusterError,
    >
    where
        'cfg: 'r,
    {
        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach phase checks the rendezvous active marker before mutating endpoint lease storage. */ unsafe { &mut *self.storage_ptr() };
        let rv = core
            .locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)?;
        if let Err(resource) = rv.ensure_endpoint_resident_budget(resident_budget) {
            return Err(ClusterError::resource_exhausted(resource));
        }
        let Some((slot, generation, offset, _len)) = (/* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */unsafe {
            rv.allocate_endpoint_lease(required_bytes, required_align, resident_budget)
        }) else {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::EndpointLease,
            ));
        };
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        if offset + required_bytes > slab_len {
            rv.release_endpoint_lease(slot, generation);
            return Err(ClusterError::resource_exhausted(
                ResourceScope::EndpointBounds,
            ));
        }
        if !rv.mark_public_endpoint_lease(slot, generation) {
            rv.release_endpoint_lease(slot, generation);
            return Err(ClusterError::resource_exhausted(
                ResourceScope::EndpointMark,
            ));
        }
        Ok((
            slot,
            generation,
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { slab_ptr.add(offset) }.cast::<crate::endpoint::kernel::CursorEndpoint<
                'r,
                ROLE,
                T,
                C,
                MAX_RV,
            >>(),
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
        if len == 0 || offset + len > slab_len {
            return None;
        }
        Some(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
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
        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach validation only observes a rendezvous entry after the active-marker check. */ unsafe { &mut *self.storage_ptr() };
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
        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach validation only observes a rendezvous entry after the active-marker check. */ unsafe { &mut *self.storage_ptr() };
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
    ) {
        self.with_storage_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.release_endpoint_lease(slot, generation);
            }
        });
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
            self.release_public_endpoint_slot(rv_id, slot, generation);
        }
    }

    #[inline]
    pub(crate) fn bind_session_role(
        &self,
        sid: SessionId,
        role: u8,
        rv_id: RendezvousId,
    ) -> Result<(), ClusterError> {
        self.with_storage_mut(|core| core.role_bindings.bind(sid, role, rv_id))
    }

    #[inline]
    pub(crate) fn unbind_session_role(&self, sid: SessionId, role: u8, rv_id: RendezvousId) {
        self.with_storage_mut(|core| core.role_bindings.unbind(sid, role, rv_id));
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
            .map_err(|err| {
                E::from(
                    err.cluster_cause()
                        .expect("resident program validation errors must carry storage cause"),
                )
            })?;
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
        config: crate::runtime_core::config::Config<'cfg, C>,
        transport: T,
    ) -> Result<RendezvousId, ClusterError> {
        self.with_storage_mut(|core| {
            match core
                .locals
                .register_local_from_config_auto(config, transport)
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
    pub(crate) fn get_local(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T, C>> {
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
    /// # Tap Events
    ///
    /// Emits `LANE_ACQUIRE` with:
    /// - `arg0`: Rendezvous ID (u32)
    /// - `arg1`: Packed session/lane (u32)
    pub(crate) fn lease_port<'lease>(
        &'lease self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
    ) -> Result<LaneLease<'lease, 'cfg, T, C, MAX_RV>, RendezvousError>
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
            Err(LeaseError::UnknownRendezvous(_)) => {
                return Err(RendezvousError::LaneOutOfRange { lane });
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };

        let active = &core.active_leases;

        let current = active.get();
        active.set(current + 1);

        // Extract rendezvous brand before moving lease into guard and emit acquire tap.
        let brand = lease.brand();
        lease.emit_lane_acquire(rv_id, sid, lane);

        Ok(LaneLease::new(
            lease, sid, lane, role, role_count, active, brand,
        ))
    }
}
