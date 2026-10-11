pub(super) mod image;
pub(super) mod program;

pub(crate) use self::{
    image::{
        COMPACT_DESCRIPTOR_BYTE_CAPACITY, CompiledProgramRef, ProgramImageBytes,
        ProgramImageColumns, ProgramImagePlan, RoleDescriptorRef, RoleImageSlice,
    },
    program::EventSemanticKind,
};

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use self::image::{
    PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY, PROGRAM_IMAGE_ATOM_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
};
