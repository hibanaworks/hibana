use crate::global::{
    compiled::images::CompiledProgramRef,
    role_program::{RoleFootprint, RoleImageRef},
};

/// Crate-private resident image for role-local immutable compiled facts.
///
/// Runtime attach consumes this descriptor by reference. It is owned by the
/// projected `RoleProgram` before attach; attach never constructs it from
/// lowering scratch and never copies it into the runtime slab.
#[derive(Clone, Copy)]
pub(crate) struct CompiledRoleImage {
    program: CompiledProgramRef,
    role: u8,
    image: RoleImageRef,
}

impl CompiledRoleImage {
    #[inline(always)]
    pub(crate) const fn new(program: CompiledProgramRef, role: u8, image: RoleImageRef) -> Self {
        Self {
            program,
            role,
            image,
        }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.program
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> RoleFootprint {
        self.image.footprint()
    }

    #[inline(always)]
    pub(crate) const fn role_image(&self) -> RoleImageRef {
        self.image
    }
}
