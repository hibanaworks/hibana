pub(super) mod driver;
pub(super) mod seal;

pub(crate) use self::{
    driver::{CompiledProgramImage, RoleCompiledCounts},
    seal::{
        projection_diagnostic, validate_passive_child_projection_guarantees,
        validate_route_projection_guarantees,
    },
};
