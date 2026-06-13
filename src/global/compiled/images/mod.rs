pub(super) mod image;
pub(super) mod program;

pub(crate) use self::{
    image::{CompiledProgramRef, ProgramImageBytes, RoleDescriptorRef, RoleImageSlice},
    program::{DynamicResolverSite, EventSemanticKind},
};

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use self::image::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_RESOLVER_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    ProgramColumnRange,
};
