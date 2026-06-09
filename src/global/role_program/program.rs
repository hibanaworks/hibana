use super::{RoleProgramView, private};

pub struct RoleProgram<const ROLE: u8> {
    _private: (),
    image: &'static crate::global::role_program::RoleImageRef,
}

impl<const ROLE: u8> RoleProgram<ROLE> {
    #[inline(always)]
    pub(crate) const fn role_image_ref(
        &self,
    ) -> &'static crate::global::role_program::RoleImageRef {
        self.image
    }
}

pub(crate) const fn role_program_from_image<const ROLE: u8>(
    image: &'static crate::global::role_program::RoleImageRef,
) -> RoleProgram<ROLE> {
    RoleProgram {
        _private: (),
        image,
    }
}

impl<const ROLE: u8> private::RoleProgramViewSeal for RoleProgram<ROLE> {}

impl<const ROLE: u8> RoleProgramView<ROLE> for RoleProgram<ROLE> {
    #[inline(always)]
    fn role_image_ref(&self) -> &'static crate::global::role_program::RoleImageRef {
        RoleProgram::role_image_ref(self)
    }
}

/// Project a typed program into the local view for `ROLE`.
pub fn project<const ROLE: u8, P>(program: &P) -> RoleProgram<ROLE>
where
    P: crate::global::program::Projectable + ?Sized,
{
    crate::global::program::project_unnamed(program)
}
