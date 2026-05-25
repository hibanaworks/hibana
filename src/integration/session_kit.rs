use super::AttachError;
use crate::control::cluster::error::{CpError, ResourceScope};

/// Protocol-neutral session kit facade for protocol implementors.
///
/// The runtime is intentionally local-only: `SessionKit` is neither `Send` nor
/// `Sync`, and mutation is centralised inside the single-thread integration
/// owner.
#[repr(transparent)]
pub struct SessionKit<'cfg, T, U, C, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) inner: crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    _cfg: core::marker::PhantomData<crate::endpoint::carrier::SessionCfg<Self>>,
    _local_only: crate::local::LocalOnly,
}

/// Owning storage for a short-lived or host-managed [`SessionKit`].
///
/// Resident substrates that deliberately leak their session owner may use
/// [`SessionKit::init_in_place`] directly. Host integrations should prefer this
/// guard-shaped owner: it keeps the initialized value tied to Rust lifetime
/// ownership and drops it exactly once when the storage is dropped.
pub struct SessionKitStorage<'cfg, T, U, C, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    storage: core::mem::MaybeUninit<SessionKit<'cfg, T, U, C, MAX_RV>>,
    initialized: bool,
}

/// Borrowed resident kit returned by [`SessionKitStorage::init`].
///
/// Endpoints borrowed through this guard cannot outlive the guard borrow, so
/// host teardown does not rely on remembering the raw `MaybeUninit` protocol.
pub struct ResidentSessionKit<'kit, 'cfg, T, U, C, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    kit: &'kit SessionKit<'cfg, T, U, C, MAX_RV>,
}

/// Rendezvous-scoped integration witness.
pub struct RendezvousKit<'kit, 'cfg, T, U, C, const HAS_SESSION: bool, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T, U, C, MAX_RV>,
    pub(super) rv: crate::integration::ids::RendezvousId,
    pub(super) sid: crate::integration::ids::SessionId,
}

/// Projected role witness within a rendezvous or one session attach.
pub struct RoleKit<
    'kit,
    'cfg,
    'prog,
    const ROLE: u8,
    T,
    U,
    C,
    const HAS_SESSION: bool,
    const MAX_RV: usize,
> where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T, U, C, MAX_RV>,
    pub(super) rv: crate::integration::ids::RendezvousId,
    pub(super) sid: crate::integration::ids::SessionId,
    pub(super) program: &'prog crate::integration::program::RoleProgram<ROLE>,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKitStorage<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Create uninitialized session-kit storage.
    pub const fn uninit() -> Self {
        Self {
            storage: core::mem::MaybeUninit::uninit(),
            initialized: false,
        }
    }

    /// Initialize the session kit and return a guard-shaped resident borrow.
    ///
    /// This method is safe because the returned guard keeps the storage
    /// mutably borrowed for its lifetime, and the storage owner drops the
    /// initialized kit exactly once.
    pub fn init(&mut self) -> ResidentSessionKit<'_, 'cfg, T, U, C, MAX_RV> {
        assert!(
            !self.initialized,
            "SessionKitStorage must not be initialized twice"
        );
        unsafe {
            // SAFETY: `self.storage` is exclusively borrowed through `&mut self`,
            // has not been initialized yet, and remains owned by this guard until
            // `Drop` runs exactly once.
            SessionKit::init_empty(self.storage.as_mut_ptr());
        }
        self.initialized = true;
        ResidentSessionKit {
            kit: unsafe {
                // SAFETY: `init_empty` has initialized the storage above and the
                // returned borrow is tied to the mutable borrow of this storage.
                &*self.storage.as_ptr()
            },
        }
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> Drop for SessionKitStorage<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                // SAFETY: `initialized` is set only after `init_empty` succeeds and
                // this storage owner drops the resident kit exactly once.
                core::ptr::drop_in_place(self.storage.as_mut_ptr());
            }
        }
    }
}

impl<'kit, 'cfg, T, U, C, const MAX_RV: usize> core::ops::Deref
    for ResidentSessionKit<'kit, 'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    type Target = SessionKit<'cfg, T, U, C, MAX_RV>;

    fn deref(&self) -> &Self::Target {
        self.kit
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Initialize an empty kit directly in caller-owned resident storage.
    ///
    /// This keeps the session/control owner at a stable address supplied by the
    /// integration. Resident embedded images use this to avoid materialising
    /// the session kit on the worker stack before entering projected roles.
    /// Short-lived host integrations should prefer [`SessionKitStorage`].
    ///
    /// # Safety
    ///
    /// This initializes `storage` and returns a resident borrow without
    /// creating an owning guard. The caller owns the resident lifecycle:
    /// `storage` must remain pinned and initialized for `'cfg`, and every
    /// endpoint borrowed from the kit must be dropped before the resident image
    /// is torn down. Host integrations that need owned teardown should use
    /// [`SessionKitStorage`] instead.
    pub unsafe fn init_in_place(storage: &'cfg mut core::mem::MaybeUninit<Self>) -> &'cfg Self {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            Self::init_empty(storage.as_mut_ptr());
            &*storage.as_ptr()
        }
    }

    #[inline]
    /// Select a registered rendezvous before attaching roles or resolvers.
    pub fn rendezvous(
        &self,
        rv: crate::integration::ids::RendezvousId,
    ) -> RendezvousKit<'_, 'cfg, T, U, C, false, MAX_RV> {
        RendezvousKit {
            kit: self,
            rv,
            sid: crate::integration::ids::SessionId::new(0),
        }
    }

    unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            crate::control::cluster::core::SessionCluster::init_empty(core::ptr::addr_of_mut!(
                (*dst).inner
            ));
            core::ptr::addr_of_mut!((*dst)._cfg).write(core::marker::PhantomData);
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    /// Add one rendezvous runtime from caller-provided config and transport.
    ///
    /// The config owns only the tap buffer, slab, and clock envelope used by
    /// the rendezvous. Lane storage and endpoint leases are derived when a
    /// resident projected role descriptor attaches. The transport owns I/O
    /// state.
    #[track_caller]
    pub fn add_rendezvous_from_config(
        &self,
        config: crate::integration::runtime::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<crate::integration::ids::RendezvousId, AttachError> {
        let location = crate::control::cluster::error::ErrorLocation::caller();
        self.inner
            .add_rendezvous_from_config(config, transport)
            .map_err(|error| {
                AttachError::control(error).with_operation(
                    crate::control::cluster::error::AttachOp::AddRendezvous,
                    location,
                )
            })
    }

    #[inline(never)]
    #[track_caller]
    pub(super) fn enter_attached<'r, const ROLE: u8, B>(
        &'r self,
        rv: crate::integration::ids::RendezvousId,
        sid: crate::integration::ids::SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
        binding: B,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        B: crate::binding::BindingArg<'r>,
        'cfg: 'r,
    {
        let location = crate::control::cluster::error::ErrorLocation::caller();
        let binding = binding.into_binding_handle();
        Self::enter_with_binding(self, rv, sid, program, binding).map_err(|error| {
            error.with_operation(crate::control::cluster::error::AttachOp::Enter, location)
        })
    }

    #[inline(never)]
    fn enter_with_binding<'r, const ROLE: u8>(
        &'r self,
        rv: crate::integration::ids::RendezvousId,
        sid: crate::integration::ids::SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        let (slot, generation) = self.inner.enter::<ROLE>(rv, sid, program, binding)?;
        let ptr = self
            .inner
            .public_endpoint_header_ptr(rv, slot, generation)
            .ok_or(AttachError::control(CpError::resource_exhausted(
                ResourceScope::EndpointHeader,
            )))?;
        let handle = crate::endpoint::carrier::PackedEndpointHandle::new(rv, slot, generation);
        Ok(crate::endpoint::Endpoint::from_handle(ptr, handle))
    }

    #[inline]
    #[track_caller]
    pub(super) fn set_role_resolver<const POLICY: u16, const ROLE: u8>(
        &self,
        rv: crate::integration::ids::RendezvousId,
        program: &crate::integration::program::RoleProgram<ROLE>,
        resolver: crate::integration::policy::ResolverRef<'cfg>,
    ) -> Result<(), crate::integration::policy::ResolverError> {
        let location = crate::control::cluster::core::ResolverErrorLocation::caller();
        self.inner
            .set_resolver::<POLICY, ROLE>(rv, program, resolver)
            .map_err(|error| {
                crate::integration::policy::ResolverError::control(error).with_operation_at(
                    crate::control::cluster::core::ResolverOp::SetResolver,
                    location,
                )
            })
    }
}
