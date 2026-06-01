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

/// In-place storage owner for a [`SessionKit`].
///
/// The storage is caller-owned and heapless. Initialization writes the kit in
/// place and returns the stable borrow tied to the storage owner.
pub struct SessionKitStorage<
    'cfg,
    T,
    U = crate::runtime::consts::DefaultLabelUniverse,
    C = crate::runtime::config::CounterClock,
    const MAX_RV: usize = 4,
> where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    storage: core::mem::MaybeUninit<SessionKit<'cfg, T, U, C, MAX_RV>>,
    initialized: bool,
}

/// Rendezvous-scoped integration witness.
pub struct RendezvousKit<'kit, 'cfg, T, U, C, const HAS_SESSION: bool, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) kit: &'kit SessionKit<'cfg, T, U, C, MAX_RV>,
    pub(super) rv: crate::control::types::RendezvousId,
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
    pub(super) rv: crate::control::types::RendezvousId,
    pub(super) sid: crate::integration::ids::SessionId,
    pub(super) program: &'prog crate::integration::program::RoleProgram<ROLE>,
    pub(super) binding: Option<&'kit mut dyn crate::integration::binding::EndpointSlot>,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKitStorage<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Create uninitialized kit storage.
    pub const fn uninit() -> Self {
        Self {
            storage: core::mem::MaybeUninit::uninit(),
            initialized: false,
        }
    }

    /// Initialize the kit in place.
    pub fn init(&mut self) -> &SessionKit<'cfg, T, U, C, MAX_RV> {
        assert!(
            !self.initialized,
            "SessionKitStorage must not be initialized twice"
        );
        unsafe {
            // SAFETY: `self.storage` is exclusively borrowed through `&mut self`,
            // has not been initialized yet, and remains owned by this storage
            // object until `Drop` runs exactly once.
            SessionKit::init_empty(self.storage.as_mut_ptr());
        }
        self.initialized = true;
        unsafe {
            // SAFETY: `init_empty` has initialized `storage`; the returned
            // shared borrow is tied to the mutable borrow of this owner.
            &*self.storage.as_ptr()
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
                // SAFETY: `initialized` is set only after `init_empty` succeeds;
                // this storage owner drops the initialized kit exactly once.
                core::ptr::drop_in_place(self.storage.as_mut_ptr());
            }
        }
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
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
    /// Obtain one registered rendezvous witness from caller-provided config and transport.
    ///
    /// The config owns only the tap buffer, slab, and clock envelope used by
    /// the rendezvous. Lane storage and endpoint leases are derived when a
    /// projected role descriptor attaches. The transport owns I/O
    /// state.
    #[track_caller]
    pub fn rendezvous(
        &self,
        config: crate::integration::runtime::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousKit<'_, 'cfg, T, U, C, false, MAX_RV>, AttachError> {
        let location = crate::control::cluster::error::ErrorLocation::caller();
        let rv = self
            .inner
            .register_rendezvous(config, transport)
            .map_err(|error| {
                AttachError::control(error).with_operation(
                    crate::control::cluster::error::AttachOp::Rendezvous,
                    location,
                )
            })?;
        Ok(RendezvousKit {
            kit: self,
            rv,
            sid: crate::integration::ids::SessionId::new(0),
        })
    }

    #[inline(never)]
    #[track_caller]
    pub(super) fn enter_attached<'r, const ROLE: u8>(
        &'r self,
        rv: crate::control::types::RendezvousId,
        sid: crate::integration::ids::SessionId,
        program: &crate::integration::program::RoleProgram<ROLE>,
        binding: Option<&'r mut dyn crate::binding::EndpointSlot>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        let location = crate::control::cluster::error::ErrorLocation::caller();
        let binding = match binding {
            Some(binding) => crate::binding::BindingHandle::Borrowed(binding),
            None => crate::binding::BindingHandle::None(crate::binding::NoBinding),
        };
        Self::enter_endpoint(self, rv, sid, program, binding).map_err(|error| {
            error.with_operation(crate::control::cluster::error::AttachOp::Enter, location)
        })
    }

    #[inline(never)]
    fn enter_endpoint<'r, const ROLE: u8>(
        &'r self,
        rv: crate::control::types::RendezvousId,
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
        rv: crate::control::types::RendezvousId,
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
