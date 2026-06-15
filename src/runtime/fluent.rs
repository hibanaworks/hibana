use super::{AttachError, RendezvousKit, RoleKit};
impl<'kit, 'cfg, T, const MAX_RV: usize> RendezvousKit<'kit, 'cfg, T, false, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub fn session(
        &self,
        sid: crate::runtime::ids::SessionId,
    ) -> RendezvousKit<'kit, 'cfg, T, true, MAX_RV> {
        RendezvousKit {
            kit: self.kit,
            rv: self.rv,
            sid,
        }
    }
}

impl<'kit, 'cfg, T, const HAS_SESSION: bool, const MAX_RV: usize>
    RendezvousKit<'kit, 'cfg, T, HAS_SESSION, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub fn tap(&self) -> crate::runtime::tap::TapPort<'_> {
        crate::invariant_some(self.kit.inner.get_local(&self.rv))
            .tap()
            .port()
    }

    #[inline]
    pub fn role<'prog, const ROLE: u8>(
        &self,
        program: &'prog crate::runtime::program::RoleProgram<ROLE>,
    ) -> RoleKit<'kit, 'cfg, 'prog, ROLE, T, HAS_SESSION, MAX_RV> {
        RoleKit {
            kit: self.kit,
            rv: self.rv,
            sid: self.sid,
            program,
        }
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, const MAX_RV: usize>
    RoleKit<'kit, 'cfg, 'prog, ROLE, T, true, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    'cfg: 'kit,
{
    /// Attach this projected role program as an endpoint.
    #[inline]
    #[track_caller]
    pub fn enter(self) -> Result<crate::Endpoint<'kit, ROLE>, AttachError> {
        self.kit.enter_attached(self.rv, self.sid, self.program)
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, const MAX_RV: usize>
    RoleKit<'kit, 'cfg, 'prog, ROLE, T, false, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    /// Install a resolver for an explicit route resolution site on this role.
    ///
    /// Dynamic resolution exists only where projection produced a matching
    /// resolver site.
    #[track_caller]
    pub fn set_resolver<const RESOLVER: u16>(
        self,
        resolver: crate::runtime::resolver::ResolverRef<'cfg, RESOLVER>,
    ) -> Result<(), crate::runtime::resolver::ResolverError> {
        self.kit
            .set_role_resolver::<RESOLVER, ROLE>(self.rv, self.program, resolver)
    }
}
