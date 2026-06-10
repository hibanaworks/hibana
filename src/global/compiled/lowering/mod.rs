pub(super) mod driver;
pub(super) mod seal;

pub(crate) use self::{
    driver::{CompiledProgramImage, CompiledProgramView, RoleCompiledCounts},
    seal::projection_error_all_roles,
};
