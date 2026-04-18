pub(super) mod driver;
pub(super) mod program_image_builder;
pub(super) mod program_lowering;
#[cfg(test)]
pub(super) mod program_owner;
pub(super) mod program_tail_storage;
pub(super) mod role_image_builder;
pub(super) mod role_image_lowering;
pub(super) mod role_scope_storage;
pub(super) mod seal;

pub(crate) use self::{
    driver::{LoweringSummary, LoweringView, ProgramStamp, RoleLoweringCounts},
    seal::validate_all_roles,
};

#[cfg(test)]
pub(crate) use self::program_owner::CompiledProgram;
