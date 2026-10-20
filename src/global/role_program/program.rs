use super::{RoleProgramView, private};

/// The compiled local protocol for `ROLE`.
///
/// Created by [`project`] and passed to [`crate::runtime::RendezvousKit::enter`].
/// It describes permitted communication; the attached Endpoint tracks execution.
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
///
/// Projection validates associated constants during code generation. Use
/// `cargo build` or `cargo test` to check a changed choreography; `cargo check`
/// alone need not evaluate this validation. [`crate::g::diagnose`] exposes a
/// projection diagnostic without attaching an Endpoint. Participant functions
/// still have their sends and receives checked at runtime.
pub fn project<const ROLE: u8>(
    program: &impl crate::global::program::Projectable,
) -> RoleProgram<ROLE> {
    // Keep the evaluated constant in this public entry point so rustc's
    // instantiation note reaches the user's project call. The PhantomData in
    // its result lets the input infer Steps without another public type argument.
    crate::global::program::bind_projection(program, const { crate::g::project_image::<ROLE, _>() })
}
