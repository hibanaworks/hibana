use super::AttachError;
use crate::{
    diag::Callsite,
    session::cluster::error::{ClusterError, ResourceScope},
};

/// Protocol-neutral session kit facade for protocol implementors.
///
/// The runtime is intentionally local-only: `SessionKit` is neither `Send` nor
/// `Sync`, and mutation is centralised inside the single-thread runtime
/// owner.
#[repr(transparent)]
pub struct SessionKit<'cfg, T, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) inner: crate::session::cluster::core::SessionCluster<'cfg, T, MAX_RV>,
    _local_only: crate::local::LocalOnly,
}

/// In-place storage owner for a [`SessionKit`].
///
/// The storage is caller-owned and heapless. Initialization writes the kit in
/// place and returns the stable borrow tied to the storage owner.
pub struct SessionKitStorage<'cfg, T, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
{
    storage: core::mem::MaybeUninit<SessionKit<'cfg, T, MAX_RV>>,
    state: SessionKitStorageState,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionKitStorageState {
    Uninitialized = 0,
    Initialized = 1,
}

impl SessionKitStorageState {
    #[inline]
    const fn is_initialized(self) -> bool {
        matches!(self, Self::Initialized)
    }
}

/// Rendezvous-scoped runtime witness.
pub struct RendezvousKit<'kit, 'cfg, T, const HAS_SESSION: bool, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T, MAX_RV>,
    pub(super) rv: crate::session::types::RendezvousId,
    pub(super) sid: crate::runtime::ids::SessionId,
}

/// Projected role witness within a rendezvous or one session attach.
pub struct RoleKit<
    'kit,
    'cfg,
    'prog,
    const ROLE: u8,
    T,
    const HAS_SESSION: bool,
    const MAX_RV: usize,
> where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T, MAX_RV>,
    pub(super) rv: crate::session::types::RendezvousId,
    pub(super) sid: crate::runtime::ids::SessionId,
    pub(super) program: &'prog crate::runtime::program::RoleProgram<ROLE>,
}

impl<'cfg, T, const MAX_RV: usize> SessionKitStorage<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    /// Create uninitialized kit storage.
    pub const fn uninit() -> Self {
        Self {
            storage: core::mem::MaybeUninit::uninit(),
            state: SessionKitStorageState::Uninitialized,
        }
    }

    /// Initialize the kit in place.
    pub fn init(&mut self) -> &SessionKit<'cfg, T, MAX_RV> {
        if self.state.is_initialized() {
            crate::invariant();
        }
        unsafe {
            // SAFETY: `self.storage` is exclusively borrowed through `&mut self`,
            // has not been initialized yet, and remains owned by this storage
            // object until `Drop` runs exactly once.
            SessionKit::init_unregistered(self.storage.as_mut_ptr());
        }
        self.state = SessionKitStorageState::Initialized;
        unsafe {
            // SAFETY: `init_unregistered` has initialized `storage`; the returned
            // shared borrow is tied to the mutable borrow of this owner.
            &*self.storage.as_ptr()
        }
    }
}

impl<'cfg, T, const MAX_RV: usize> Drop for SessionKitStorage<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    fn drop(&mut self) {
        if self.state.is_initialized() {
            unsafe {
                // SAFETY: `Initialized` is set only after initialization
                // succeeds; this storage owner drops the kit exactly once.
                core::ptr::drop_in_place(self.storage.as_mut_ptr());
            }
        }
    }
}

impl<'cfg, T, const MAX_RV: usize> SessionKit<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    unsafe fn init_unregistered(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            crate::session::cluster::core::SessionCluster::init_empty(core::ptr::addr_of_mut!(
                (*dst).inner
            ));
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    /// Obtain one registered rendezvous witness from caller-provided config and transport.
    ///
    /// The config borrows the single runtime slab. Runtime capacity is carved
    /// or derived by Hibana; the transport owns only I/O state.
    #[track_caller]
    pub fn rendezvous(
        &self,
        config: crate::runtime::Config<'cfg>,
        transport: T,
    ) -> Result<RendezvousKit<'_, 'cfg, T, false, MAX_RV>, AttachError> {
        let location = Callsite::caller();
        let rv = self
            .inner
            .register_rendezvous(config, transport)
            .map_err(|error| {
                AttachError::cluster(error).with_operation(
                    crate::session::cluster::error::AttachOp::Rendezvous,
                    location,
                )
            })?;
        Ok(RendezvousKit {
            kit: self,
            rv,
            sid: crate::runtime::ids::SessionId::new(0),
        })
    }

    #[inline(never)]
    #[track_caller]
    pub(super) fn enter_attached<'r, const ROLE: u8>(
        &'r self,
        rv: crate::session::types::RendezvousId,
        sid: crate::runtime::ids::SessionId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        let location = Callsite::caller();
        Self::enter_endpoint(self, rv, sid, program).map_err(|error| {
            error.with_operation(crate::session::cluster::error::AttachOp::Enter, location)
        })
    }

    #[inline(never)]
    fn enter_endpoint<'r, const ROLE: u8>(
        &'r self,
        rv: crate::session::types::RendezvousId,
        sid: crate::runtime::ids::SessionId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        let (slot, generation) = self.inner.enter::<ROLE>(rv, sid, program)?;
        let ptr = self
            .inner
            .public_endpoint_header_ptr(rv, slot, generation)
            .ok_or(AttachError::cluster(ClusterError::resource_exhausted(
                ResourceScope::EndpointHeader,
            )))?;
        let handle = crate::endpoint::carrier::PackedEndpointHandle::new(rv, slot, generation);
        Ok(crate::endpoint::Endpoint::from_handle(ptr, handle))
    }

    #[inline]
    #[track_caller]
    pub(super) fn set_role_resolver<const RESOLVER: u16, const ROLE: u8>(
        &self,
        rv: crate::session::types::RendezvousId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
        resolver: crate::runtime::resolver::ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), crate::runtime::resolver::ResolverError> {
        let location = Callsite::caller();
        self.inner
            .set_resolver::<RESOLVER, ROLE>(rv, program, resolver)
            .map_err(|error| {
                crate::runtime::resolver::ResolverError::cluster(error).with_operation_at(
                    crate::session::cluster::core::ResolverOp::SetResolver,
                    location,
                )
            })
    }
}
