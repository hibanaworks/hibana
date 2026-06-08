use super::{AttachError, RendezvousKit, RoleKit};
impl<'kit, 'cfg, T, U, C, const MAX_RV: usize> RendezvousKit<'kit, 'cfg, T, U, C, false, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    pub fn session(
        &self,
        sid: crate::integration::ids::SessionId,
    ) -> RendezvousKit<'kit, 'cfg, T, U, C, true, MAX_RV> {
        RendezvousKit {
            kit: self.kit,
            rv: self.rv,
            sid,
        }
    }
}

impl<'kit, 'cfg, T, U, C, const HAS_SESSION: bool, const MAX_RV: usize>
    RendezvousKit<'kit, 'cfg, T, U, C, HAS_SESSION, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    pub fn tap(&self) -> crate::integration::tap::TapPort<'_> {
        self.kit
            .inner
            .get_local(&self.rv)
            .expect("rendezvous witness must reference a registered rendezvous")
            .tap()
            .port()
    }

    #[inline]
    pub fn role<'prog, const ROLE: u8>(
        &self,
        program: &'prog crate::integration::program::RoleProgram<ROLE>,
    ) -> RoleKit<'kit, 'cfg, 'prog, ROLE, T, U, C, HAS_SESSION, MAX_RV> {
        RoleKit {
            kit: self.kit,
            rv: self.rv,
            sid: self.sid,
            program,
        }
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, U, C, const MAX_RV: usize>
    RoleKit<'kit, 'cfg, 'prog, ROLE, T, U, C, true, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    'cfg: 'kit,
{
    /// Attach this projected role program as an endpoint.
    #[inline]
    #[track_caller]
    pub fn enter(self) -> Result<crate::Endpoint<'kit, ROLE>, AttachError> {
        self.kit.enter_attached(self.rv, self.sid, self.program)
    }
}

impl<'kit, 'cfg, 'prog, const ROLE: u8, T, U, C, const MAX_RV: usize>
    RoleKit<'kit, 'cfg, 'prog, ROLE, T, U, C, false, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    /// Install a resolver for an explicit route or loop resolution site on this role.
    ///
    /// Dynamic resolution exists only where projection produced a matching
    /// resolver site.
    #[track_caller]
    pub fn set_resolver<const POLICY: u16>(
        self,
        resolver: crate::integration::resolver::ResolverRef<'cfg, POLICY>,
    ) -> Result<(), crate::integration::resolver::ResolverError> {
        self.kit
            .set_role_resolver::<POLICY, ROLE>(self.rv, self.program, resolver)
    }
}
