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
    role: u8,
    image: &'static RoleImageRef,
}

impl CompiledRoleImage {
    #[inline(always)]
    pub(crate) const fn new(role: u8, image: &'static RoleImageRef) -> Self {
        Self { role, image }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.image.program
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
        *self.image
    }
}
