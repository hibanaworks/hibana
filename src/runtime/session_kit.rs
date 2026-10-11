use super::AttachError;

/// Protocol-neutral session kit facade for protocol implementors.
///
/// The runtime is intentionally local-only: `SessionKit` is neither `Send` nor
/// `Sync`, and mutation is centralised inside the single-thread runtime
/// owner.
#[repr(transparent)]
pub struct SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) inner: crate::session::cluster::core::SessionCluster<'cfg, T>,
    _local_only: crate::local::LocalOnly,
}

/// In-place storage owner for a [`SessionKit`].
///
/// The storage is caller-owned and heapless. Initialization writes the kit in
/// place and returns the stable borrow tied to the storage owner.
pub struct SessionKitStorage<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    storage: core::mem::MaybeUninit<SessionKit<'cfg, T>>,
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

pub(super) struct RendezvousBase<'kit, 'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T>,
    pub(super) rv: crate::session::types::RendezvousId,
}

impl<'kit, 'cfg, T> Copy for RendezvousBase<'kit, 'cfg, T> where
    T: crate::transport::Transport + 'cfg
{
}

impl<'kit, 'cfg, T> Clone for RendezvousBase<'kit, 'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'kit, 'cfg, T> RendezvousBase<'kit, 'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub(super) fn tap_port(&self) -> crate::runtime::tap::TapPort<'_> {
        crate::invariant_some(self.kit.inner.get_local(&self.rv))
            .tap()
            .port()
    }
}

/// Registered rendezvous witness.
pub struct RendezvousKit<'kit, 'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) base: RendezvousBase<'kit, 'cfg, T>,
}

impl<'cfg, T> SessionKitStorage<'cfg, T>
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
    pub fn init(&mut self) -> &SessionKit<'cfg, T> {
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

impl<'cfg, T> Drop for SessionKitStorage<'cfg, T>
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

impl<'kit, 'cfg, T> RendezvousKit<'kit, 'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub fn tap(&self) -> crate::runtime::tap::TapPort<'_> {
        self.base.tap_port()
    }

    /// Attach this projected role program as an endpoint for `sid`.
    #[inline]
    pub fn enter<const ROLE: u8>(
        &self,
        sid: crate::runtime::ids::SessionId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<crate::Endpoint<'kit, ROLE>, AttachError>
    where
        'cfg: 'kit,
    {
        self.base.kit.enter_attached(self.base.rv, sid, program)
    }

    /// Install a resolver for an explicit route resolution site on this role.
    ///
    /// Dynamic resolution exists only where projection produced a matching
    /// resolver site.
    #[inline]
    pub fn set_resolver<const ROLE: u8, const RESOLVER: u16>(
        &self,
        program: &crate::runtime::program::RoleProgram<ROLE>,
        resolver: crate::runtime::resolver::ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), crate::runtime::resolver::ResolverError> {
        self.base
            .kit
            .set_role_resolver::<RESOLVER, ROLE>(self.base.rv, program, resolver)
    }
}

impl<'cfg, T> SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    unsafe fn init_unregistered(dst: *mut Self) {
        /* SAFETY: `SessionKitStorage::init` calls this with its single
        `MaybeUninit<SessionKit<T>>` slot under `&mut self`. `SessionCluster`
        initializes its registry before exposure, `_local_only` is written
        exactly once, and no `SessionKit` reference exists until this function
        returns. */
        unsafe {
            crate::session::cluster::core::SessionCluster::init_empty(core::ptr::addr_of_mut!(
                (*dst).inner
            ));
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    /// Obtain one registered rendezvous witness from caller-provided slab and transport.
    ///
    /// Runtime capacity is carved or derived by Hibana; the transport owns only
    /// I/O state.
    pub fn rendezvous(
        &self,
        slab: &'cfg mut [u8],
        transport: T,
    ) -> Result<RendezvousKit<'_, 'cfg, T>, AttachError> {
        let resources = crate::runtime_core::resources::RuntimeResources::new(slab);
        let rv = self
            .inner
            .register_rendezvous(resources, transport)
            .map_err(|error| {
                AttachError::cluster(error)
                    .with_operation(crate::session::cluster::error::AttachOp::Rendezvous)
            })?;
        Ok(RendezvousKit {
            base: RendezvousBase { kit: self, rv },
        })
    }

    #[inline(never)]
    pub(super) fn enter_attached<'r, const ROLE: u8>(
        &'r self,
        rv: crate::session::types::RendezvousId,
        sid: crate::runtime::ids::SessionId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        Self::enter_endpoint(self, rv, sid, program)
            .map_err(|error| error.with_operation(crate::session::cluster::error::AttachOp::Enter))
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
        let ptr = self.inner.public_endpoint_header_ptr(rv, slot, generation);
        let handle = crate::endpoint::carrier::PackedEndpointHandle::new(generation);
        Ok(crate::endpoint::Endpoint::from_handle(ptr, handle))
    }

    #[inline]
    pub(super) fn set_role_resolver<const RESOLVER: u16, const ROLE: u8>(
        &self,
        rv: crate::session::types::RendezvousId,
        program: &crate::runtime::program::RoleProgram<ROLE>,
        resolver: crate::runtime::resolver::ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), crate::runtime::resolver::ResolverError> {
        self.inner
            .set_resolver::<RESOLVER, ROLE>(rv, program, resolver)
            .map_err(|error| {
                crate::runtime::resolver::ResolverError::cluster(error)
                    .with_operation(crate::session::cluster::core::ResolverOp::SetResolver)
            })
    }
}
