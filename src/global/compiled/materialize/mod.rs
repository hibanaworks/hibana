mod lease;

pub(crate) use self::lease::{
    LoweringLeaseMode, RoleLoweringScratch, init_compiled_program_image_from_summary,
    init_compiled_role_image_from_prevalidated_summary, try_init_compiled_role_image_from_summary,
    validate_compiled_role_image_init_from_summary, with_lowering_lease,
};

#[cfg(test)]
pub(crate) use self::lease::{
    role_lowering_scratch_storage_bytes, with_compiled_program, with_compiled_role_image,
    with_role_lowering_scratch, with_role_lowering_scratch_storage,
};
