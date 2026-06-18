/// SessionCluster - owns resident local rendezvous instances.
///
/// This is the top-level local session coordinator. It manages:
/// - Local rendezvous owners
/// - Session-role endpoint leases for resident endpoints
/// - Dynamic route resolver storage
///
/// Resident mutable state of SessionCluster.
///
/// # Safety Invariants
///
/// The following invariants MUST be maintained by all code accessing `SessionStorage`:
///
/// 1. **No duplicate attach mutation**: at most one `LaneLease` mutates a rendezvous at a time
/// 2. **Session-role exclusivity**: live public endpoints hold unique `(rendezvous, sid, role)` endpoint leases
/// 3. **Rendezvous ownership**: Rendezvous instances are owned by the cluster and remain attached while leases exist
/// 4. **Resolver ownership**: dynamic resolvers are registered only for resident program sites
///
/// Violations of these invariants are guarded by the lease table where possible
/// and audited through TAP events and focused invariant tests.
pub(crate) struct SessionStorage<'cfg, T>
where
    T: crate::transport::Transport,
{
    /// Owned local rendezvous instances.
    pub(crate) locals: crate::session::lease::core::RendezvousTable<'cfg, T>,

    /// Number of active lane leases (affine witness count).
    pub(crate) active_leases: core::cell::Cell<u32>,
}

impl<'cfg, T> SessionStorage<'cfg, T>
where
    T: crate::transport::Transport,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: session cluster initialization passes an unpublished
        `SessionStorage` cell. The rendezvous registry and active lease counter
        are both initialized before the cluster can expose storage access. */
        unsafe {
            crate::session::lease::core::RendezvousTable::init_empty(core::ptr::addr_of_mut!(
                (*dst).locals
            ));
            core::ptr::addr_of_mut!((*dst).active_leases).write(core::cell::Cell::new(0));
        }
    }
}
