pub(super) mod image;
pub(super) mod program;

pub(crate) use self::{
    image::{CompiledProgramRef, ProgramImageBytes, RoleDescriptorRef, RoleImageSlice},
    program::{ControlSemanticKind, DynamicPolicySite},
};

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use self::image::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_CONTROL_DESC_STRIDE, PROGRAM_IMAGE_POLICY_STRIDE,
    PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE, ProgramColumnRange,
};
