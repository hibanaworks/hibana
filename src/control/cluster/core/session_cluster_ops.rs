use super::{
    AttachError, CompiledProgramRef, ControlCore, CpError, EndpointLeaseId, FullSpec, Lane,
    LaneLease, LeaseError, PublicEndpointStorageLayout, RegisterRendezvousError, Rendezvous,
    RendezvousError, RendezvousId, ResolverCore, ResourceScope, RoleImageSlice, SessionId,
};
use crate::control::{
    cap::{
        atomic_codecs::TopologyHandle,
        mint::{CAP_HANDLE_LEN, ControlOp},
    },
    types::Generation,
};

// Public unit topology ControlMsg carries no external transport sequence
// witness; topology ordering authority is the choreography event plus the
// generated lane transition. Explicit nonzero sequence fences stay on the
// descriptor-token path that supplies its own handle.
const CHOREOGRAPHY_TOPOLOGY_SEQ_TX: u32 = 0;
const CHOREOGRAPHY_TOPOLOGY_SEQ_RX: u32 = 0;

// # Unsafe Owner Contract
//
// This file owns the `SessionCluster` interior-mutability boundary. The
// cluster is local-only and uses `UnsafeCell` so resident endpoint leases can
// hold shared cluster references while cluster operations still mutate the
// control core and dynamic resolver table. All mutable access must pass through
// the local `with_control_mut` / `with_resolvers_mut` closures, and raw resident
// storage pointers returned here are tied to cluster-owned generation,
// endpoint-slot, and graph-storage metadata before they are exposed to endpoint
// construction.

/// SessionCluster - Distributed control-plane coordinator with interior mutability.
///
/// Uses `UnsafeCell` to allow `&self` methods while maintaining mutable internal state.
/// This enables `LaneLease` to hold `PhantomData<&'cluster SessionCluster>` (shared reference)
/// without blocking other cluster operations.
///
/// # Safety
///
/// All mutable access to the control or resolver tables goes through
/// `with_control_mut()` / `with_resolvers_mut()`, which centralize:
/// - the single mutable access boundary for the selected table;
/// - documented invariants (see `ControlCore`);
/// - TAP event monitoring for lane lifecycle.
///
/// These helpers are not reentrant guards. Crate-internal callers must keep
/// endpoint cleanup, descriptor publication, and other callback-capable work in
/// their typed post-mutation phases so no nested mutable borrow is created.
pub(crate) struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Control-plane state guarded by interior mutability.
    pub(crate) control: core::cell::UnsafeCell<
        ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
    >,
    /// Dynamic resolver table separated from core control state.
    resolvers: core::cell::UnsafeCell<ResolverCore<'cfg, MAX_RV>>,
    _local_only: crate::local::LocalOnly,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            ControlCore::<T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).control).cast(),
            );
            ResolverCore::<'cfg, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).resolvers).cast(),
            );
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    fn control_ptr(
        &self,
    ) -> *mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
    }

    #[inline]
    pub(crate) fn control_ref_ptr(
        &self,
    ) -> *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
            as *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
    }

    #[inline]
    pub(crate) fn resolvers_ptr(&self) -> *mut ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get()
    }

    #[inline]
    pub(crate) fn resolvers_ref_ptr(&self) -> *const ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get() as *const ResolverCore<'cfg, MAX_RV>
    }

    /// Internal helper to access mutable control core (NOT PUBLIC).
    pub(crate) fn with_control_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        ) -> R,
    {
        /* SAFETY: the control table lives in pinned cluster-owned storage, and crate-internal mutation phases are structured so no nested control borrow is active for the closure duration. */
        unsafe { f(&mut *self.control_ptr()) }
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
    pub(crate) fn map_rendezvous_access_error(error: LeaseError) -> CpError {
        match error {
            LeaseError::UnknownRendezvous(id) => CpError::RendezvousMissing { id: id.raw() },
            LeaseError::AlreadyLeased(id) => CpError::RendezvousBusy { id: id.raw() },
        }
    }

    #[inline]
    pub(in crate::control::cluster::core) fn public_endpoint_storage_requirement<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
    ) -> PublicEndpointStorageLayout {
        let arena_layout = role_image.endpoint_arena_layout();
        let storage_layout = crate::endpoint::kernel::cursor_endpoint_storage_layout::<
            0,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            crate::control::cap::mint::MintConfig,
        >(&arena_layout, role_image.endpoint_lane_slot_count());
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
            compiled_role.loop_table_slots(),
            compiled_role.resident_cap_entries(),
            crate::rendezvous::core::Rendezvous::<T, U, C>::frontier_workspace_guard_bytes(
                compiled_role.descriptor().frontier_scratch_layout(),
            ),
        )
    }

    #[inline(never)]
    pub(crate) fn allocate_public_endpoint_storage_for_rv<'r, const ROLE: u8, Mint>(
        &self,
        rv_id: RendezvousId,
        required_bytes: usize,
        required_align: usize,
        resident_budget: crate::rendezvous::core::EndpointResidentBudget,
    ) -> Result<
        (
            EndpointLeaseId,
            u32,
            *mut crate::endpoint::kernel::CursorEndpoint<
                'r,
                ROLE,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
                Mint,
            >,
        ),
        CpError,
    >
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
        'cfg: 'r,
    {
        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach phase checks the rendezvous active marker before mutating endpoint lease storage. */ unsafe { &mut *self.control_ptr() };
        let rv = core
            .locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)?;
        if let Err(resource) = rv.ensure_endpoint_resident_budget(resident_budget) {
            return Err(CpError::resource_exhausted(resource));
        }
        let Some((slot, generation, offset, _len)) = (/* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */unsafe {
            rv.allocate_endpoint_lease(required_bytes, required_align, resident_budget)
        }) else {
            return Err(CpError::resource_exhausted(ResourceScope::EndpointLease));
        };
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        if offset + required_bytes > slab_len {
            rv.release_endpoint_lease(slot, generation);
            return Err(CpError::resource_exhausted(ResourceScope::EndpointBounds));
        }
        if !rv.mark_public_endpoint_lease(slot, generation) {
            rv.release_endpoint_lease(slot, generation);
            return Err(CpError::resource_exhausted(ResourceScope::EndpointMark));
        }
        Ok((
            slot,
            generation,
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { slab_ptr.add(offset) }.cast::<crate::endpoint::kernel::CursorEndpoint<
                'r,
                ROLE,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
                Mint,
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
        self.with_control_mut(|core| {
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
        self.with_control_mut(|core| {
            core.locals
                .get_mut(&rv_id)
                .map(|rv| rv.poison_session(sid, cause))
                .unwrap_or(cause)
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
        self.with_control_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.register_session_waiter(sid, lane, waker);
            }
        });
    }

    #[inline]
    pub(crate) fn clear_session_waiter(&self, rv_id: RendezvousId, sid: SessionId, lane: Lane) {
        self.with_control_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.clear_session_waiter(sid, lane);
            }
        });
    }

    fn ensure_compiled_program_ref<'prog, const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<&'static CompiledProgramRef, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let compiled = program.role_image_ref().program;
        compiled
            .validate_label_universe(U::MAX_LABEL)
            .map_err(|err| {
                AttachError::control(CpError::LabelOutOfUniverse {
                    max: err.max,
                    actual: err.actual,
                })
            })?;

        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach validation only observes a rendezvous entry after the active-marker check. */ unsafe { &mut *self.control_ptr() };
        core.locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)
            .map_err(AttachError::control)?;
        Ok(compiled)
    }

    #[inline(never)]
    pub(crate) fn ensure_role_image_slice<'prog, const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<RoleImageSlice<ROLE>, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let compiled = program.role_image_ref();
        (*compiled.program)
            .validate_label_universe(U::MAX_LABEL)
            .map_err(|err| {
                AttachError::control(CpError::LabelOutOfUniverse {
                    max: err.max,
                    actual: err.actual,
                })
            })?;

        let core = /* SAFETY: the pointer comes from pinned owner storage; this attach validation only observes a rendezvous entry after the active-marker check. */ unsafe { &mut *self.control_ptr() };
        core.locals
            .get_mut_checked(&rv_id)
            .map_err(Self::map_rendezvous_access_error)
            .map_err(AttachError::control)?;
        Ok(RoleImageSlice::from_resident(compiled))
    }

    pub(crate) unsafe fn release_public_endpoint_slot(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        self.with_control_mut(|core| {
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
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| core.role_bindings.bind(sid, role, rv_id))
    }

    #[inline]
    pub(crate) fn unbind_session_role(&self, sid: SessionId, role: u8, rv_id: RendezvousId) {
        self.with_control_mut(|core| core.role_bindings.unbind(sid, role, rv_id));
    }

    pub(crate) fn topology_control_handle<const ROLE: u8>(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        peer_role: u8,
        lane: Lane,
        op: ControlOp,
    ) -> Result<[u8; CAP_HANDLE_LEN], CpError> {
        self.with_control_mut(|core| {
            let source = core
                .role_bindings
                .resolve(sid, ROLE)
                .ok_or(CpError::Topology(super::TopologyError::InvalidSession))?;
            if source.rv != src_rv {
                return Err(CpError::RendezvousMismatch {
                    expected: source.rv.raw(),
                    actual: src_rv.raw(),
                });
            }
            let peer = core
                .role_bindings
                .resolve(sid, peer_role)
                .ok_or(CpError::Topology(super::TopologyError::InvalidSession))?;
            let operands = match op {
                ControlOp::TopologyBegin => {
                    let rv = core
                        .locals
                        .get_mut(&src_rv)
                        .ok_or(CpError::RendezvousMismatch {
                            expected: src_rv.raw(),
                            actual: 0,
                        })?;
                    let old_gen = rv.lane_generation(lane);
                    let Some(new_gen) = old_gen.raw().checked_add(1).map(Generation::new) else {
                        return Err(CpError::GenerationViolation {
                            expected: old_gen.raw(),
                            actual: old_gen.raw(),
                        });
                    };
                    super::TopologyOperands {
                        src_rv,
                        dst_rv: peer.rv,
                        src_lane: lane,
                        dst_lane: Lane::try_new(
                            lane.raw()
                                .checked_add(1)
                                .ok_or(CpError::resource_exhausted(ResourceScope::TopologyTable))?,
                        )
                        .ok_or(CpError::resource_exhausted(ResourceScope::TopologyTable))?,
                        old_gen,
                        new_gen,
                        seq_tx: CHOREOGRAPHY_TOPOLOGY_SEQ_TX,
                        seq_rx: CHOREOGRAPHY_TOPOLOGY_SEQ_RX,
                    }
                }
                ControlOp::TopologyAck | ControlOp::TopologyCommit => *core
                    .topology_state
                    .get(sid)
                    .ok_or(CpError::Topology(super::TopologyError::InvalidSession))?,
                ControlOp::LoopContinue
                | ControlOp::LoopBreak
                | ControlOp::StateSnapshot
                | ControlOp::StateRestore
                | ControlOp::TxCommit
                | ControlOp::TxAbort => {
                    return Err(CpError::Authorisation {
                        operation: op as u8,
                    });
                }
            };

            let expected_source = match op {
                ControlOp::TopologyAck => peer.rv,
                ControlOp::TopologyBegin | ControlOp::TopologyCommit => src_rv,
                _ => src_rv,
            };
            let expected_destination = match op {
                ControlOp::TopologyAck => src_rv,
                ControlOp::TopologyBegin | ControlOp::TopologyCommit => peer.rv,
                _ => peer.rv,
            };
            if operands.src_rv != expected_source || operands.dst_rv != expected_destination {
                return Err(CpError::Authorisation {
                    operation: op as u8,
                });
            }
            match op {
                ControlOp::TopologyAck => {}
                ControlOp::TopologyBegin | ControlOp::TopologyCommit => {
                    if operands.src_lane != lane || operands.dst_rv == operands.src_rv {
                        return Err(CpError::Authorisation {
                            operation: op as u8,
                        });
                    }
                }
                _ => {
                    return Err(CpError::Authorisation {
                        operation: op as u8,
                    });
                }
            }

            Ok(TopologyHandle {
                src_rv: operands.src_rv.raw(),
                dst_rv: operands.dst_rv.raw(),
                src_lane: operands.src_lane.raw() as u16,
                dst_lane: operands.dst_lane.raw() as u16,
                old_gen: operands.old_gen.raw(),
                new_gen: operands.new_gen.raw(),
                seq_tx: operands.seq_tx,
                seq_rx: operands.seq_rx,
            }
            .to_bytes())
        })
    }

    pub(crate) fn with_resident_program_ref<const ROLE: u8, P, F, R, E>(
        &self,
        rv_id: RendezvousId,
        program: &P,
        f: F,
    ) -> Result<R, E>
    where
        E: From<CpError>,
        P: crate::global::RoleProgramView<ROLE>,
        F: FnOnce(&CompiledProgramRef) -> Result<R, E>,
    {
        let compiled = self
            .ensure_compiled_program_ref(rv_id, program)
            .map_err(|err| {
                E::from(
                    err.control_cause()
                        .expect("resident program validation errors must carry control cause"),
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
    /// Returns `CpError::resource_exhausted(ResourceScope::RendezvousTable)` if the cluster is full.
    /// Build and register a local rendezvous from runtime config + transport.
    ///
    /// Public callers should use this entrypoint instead of constructing
    /// rendezvous internals directly.
    pub(crate) fn register_rendezvous(
        &self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| {
            match core
                .locals
                .register_local_from_config_auto(config, transport)
            {
                Ok(id) => Ok(id),
                Err(
                    RegisterRendezvousError::CapacityExceeded
                    | RegisterRendezvousError::StorageExhausted,
                ) => Err(CpError::resource_exhausted(ResourceScope::RendezvousTable)),
            }
        })
    }

    /// Get a local Rendezvous by ID.
    ///
    /// # Safety
    ///
    /// Returns a shared reference from cluster-owned storage. Callers must keep
    /// this borrow out of phases that can enter `with_control_mut`.
    pub(crate) fn get_local(
        &self,
        id: &RendezvousId,
    ) -> Option<&Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>> {
        // SAFETY: We're returning a shared reference from UnsafeCell.
        // This is safe because:
        // - The reference is borrowed from `&self`, so it can't outlive the cluster
        // - Caller must not call mutable methods while holding this reference
        // - This pattern is documented in SessionCluster's safety contract
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe { (*self.control_ref_ptr()).locals.get(id) }
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
    ) -> Result<LaneLease<'lease, 'cfg, T, U, C, MAX_RV>, RendezvousError>
    where
        'cfg: 'lease,
    {
        // SAFETY: `SessionCluster` is local-only. This phase enters the
        // cluster-owned control table solely to mark one rendezvous entry active
        // and move the resulting lease out; callers must not nest this in a
        // `with_control_mut` phase.
        let core = unsafe { &mut *self.control_ptr() };

        let mut lease = match core.locals.lease::<FullSpec>(rv_id) {
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
