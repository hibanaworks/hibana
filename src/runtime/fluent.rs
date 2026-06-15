use super::{AttachError, RendezvousKit, RoleKit, SessionRendezvousKit, SessionRoleKit};

impl<'kit, 'cfg, T, const MAX_RV: usize> RendezvousKit<'kit, 'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub fn session(
        &self,
        sid: crate::runtime::ids::SessionId,
    ) -> SessionRendezvousKit<'kit, 'cfg, T, MAX_RV> {
        SessionRendezvousKit {
            base: self.base,
            sid,
        }
    }

    #[inline]
    pub fn tap(&self) -> crate::runtime::tap::TapPort<'_> {
        self.base.tap_port()
    }

    #[inline]
    pub fn role<'prog, const ROLE: u8>(
        &self,
        program: &'prog crate::runtime::program::RoleProgram<ROLE>,
    ) -> RoleKit<'kit, 'cfg, 'prog, ROLE, T, MAX_RV> {
        RoleKit {
            base: self.base,
            program,
        }
    }
}

impl<'kit, 'cfg, T, const MAX_RV: usize> SessionRendezvousKit<'kit, 'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    #[inline]
    pub fn tap(&self) -> crate::runtime::tap::TapPort<'_> {
        self.base.tap_port()
    }

    #[inline]
    pub fn role<'prog, const ROLE: u8>(
        &self,
        program: &'prog crate::runtime::program::RoleProgram<ROLE>,
    ) -> SessionRoleKit<'kit, 'cfg, 'prog, ROLE, T, MAX_RV> {
        SessionRoleKit {
            base: self.base,
            sid: self.sid,
            program,
        }
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, const MAX_RV: usize>
    SessionRoleKit<'kit, 'cfg, 'prog, ROLE, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    'cfg: 'kit,
{
    /// Attach this projected role program as an endpoint.
    #[inline]
    #[track_caller]
    pub fn enter(self) -> Result<crate::Endpoint<'kit, ROLE>, AttachError> {
        self.base
            .kit
            .enter_attached(self.base.rv, self.sid, self.program)
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, const MAX_RV: usize>
    RoleKit<'kit, 'cfg, 'prog, ROLE, T, MAX_RV>
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
        self.base
            .kit
            .set_role_resolver::<RESOLVER, ROLE>(self.base.rv, self.program, resolver)
    }
}
