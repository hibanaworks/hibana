use super::{RoleProgramView, private};

pub struct RoleProgram<const ROLE: u8> {
    _private: (),
    image: &'static crate::global::compiled::images::CompiledRoleImage,
}

impl<const ROLE: u8> RoleProgram<ROLE> {
    #[inline(always)]
    pub(crate) const fn compiled_role_image(
        &self,
    ) -> &'static crate::global::compiled::images::CompiledRoleImage {
        self.image
    }
}

pub(crate) const fn role_program_from_image<const ROLE: u8>(
    image: &'static crate::global::compiled::images::CompiledRoleImage,
) -> RoleProgram<ROLE> {
    RoleProgram {
        _private: (),
        image,
    }
}

impl<const ROLE: u8> private::RoleProgramViewSeal for RoleProgram<ROLE> {}

impl<const ROLE: u8> RoleProgramView<ROLE> for RoleProgram<ROLE> {
    #[inline(always)]
    fn compiled_role_image(&self) -> &'static crate::global::compiled::images::CompiledRoleImage {
        RoleProgram::compiled_role_image(self)
    }
}

/// Project a typed program into the local view for `ROLE`.
pub fn project<const ROLE: u8, P>(program: &P) -> RoleProgram<ROLE>
where
    P: crate::global::program::Projectable + ?Sized,
{
    crate::global::program::project_unnamed(program)
}
