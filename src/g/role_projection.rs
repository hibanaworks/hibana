use core::marker::PhantomData;

use super::{ProgramProjection, ProgramTerm};

struct RoleProjection<const ROLE: u8, Steps>(PhantomData<Steps>);
struct ProgramProjectionBlob<Steps, const N: usize>(PhantomData<Steps>);
struct RoleProjectionBlob<const ROLE: u8, Steps, const N: usize>(PhantomData<Steps>);

impl<Steps, const N: usize> ProgramProjectionBlob<Steps, N>
where
    Steps: ProgramTerm,
{
    const BYTES: Option<crate::global::compiled::images::ProgramImageBytes<N>> =
        crate::global::compiled::images::ProgramImageBytes::<N>::from_image_if_fits(
            ProgramProjection::<Steps>::SOURCE_EFF_LIST,
            ProgramProjection::<Steps>::PROGRAM_COLUMNS,
        );
}

impl<Steps> ProgramProjection<Steps>
where
    Steps: ProgramTerm,
{
    const PROGRAM_PLAN: crate::global::compiled::images::ProgramImagePlan =
        crate::global::compiled::images::ProgramImagePlan::from_program(Self::SOURCE_EFF_LIST);
    const PROGRAM_COLUMNS: crate::global::compiled::images::ProgramImageColumns =
        Self::PROGRAM_PLAN.columns();
    const PROGRAM_BLOB_LEN: usize = Self::PROGRAM_PLAN.blob_len();

    const fn program_ref<const N: usize>() -> crate::global::compiled::images::CompiledProgramRef {
        let bytes = match &ProgramProjectionBlob::<Steps, N>::BYTES {
            Some(bytes) => bytes,
            None => panic!("program bucket selection"),
        };
        bytes.compiled_ref(&Self::IMAGE, Self::PROGRAM_COLUMNS)
    }

    const PROGRAM_REF: crate::global::compiled::images::CompiledProgramRef = {
        let () = Self::VALIDATION;
        if Self::PROGRAM_BLOB_LEN <= 32 {
            Self::program_ref::<32>()
        } else if Self::PROGRAM_BLOB_LEN <= 64 {
            Self::program_ref::<64>()
        } else if Self::PROGRAM_BLOB_LEN <= 96 {
            Self::program_ref::<96>()
        } else if Self::PROGRAM_BLOB_LEN <= 128 {
            Self::program_ref::<128>()
        } else if Self::PROGRAM_BLOB_LEN <= 192 {
            Self::program_ref::<192>()
        } else if Self::PROGRAM_BLOB_LEN <= 256 {
            Self::program_ref::<256>()
        } else if Self::PROGRAM_BLOB_LEN <= 384 {
            Self::program_ref::<384>()
        } else if Self::PROGRAM_BLOB_LEN <= 512 {
            Self::program_ref::<512>()
        } else if Self::PROGRAM_BLOB_LEN <= 1024 {
            Self::program_ref::<1024>()
        } else if Self::PROGRAM_BLOB_LEN <= 2048 {
            Self::program_ref::<2048>()
        } else if Self::PROGRAM_BLOB_LEN <= 4096 {
            Self::program_ref::<4096>()
        } else if Self::PROGRAM_BLOB_LEN <= 8192 {
            Self::program_ref::<8192>()
        } else {
            panic!("program bucket")
        }
    };
}

impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>
where
    Steps: ProgramTerm,
{
    const COUNTS: crate::global::compiled::lowering::RoleCompiledCounts =
        ProgramProjection::<Steps>::IMAGE.role_lowering_counts(ROLE);
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);
    const PLAN: crate::global::role_program::RoleImagePlan =
        crate::global::role_program::RoleImagePlan::from_program(
            ProgramProjection::<Steps>::SOURCE_EFF_LIST,
            Self::FACTS,
            ROLE,
        );
    const BLOB_LEN: usize = Self::PLAN.blob_len();

    const fn image_ref<const N: usize>() -> crate::global::role_program::RoleImageRef {
        let build = match &RoleProjectionBlob::<ROLE, Steps, N>::BUILD {
            Some(build) => build,
            None => panic!("role bucket selection"),
        };
        build.image_ref(&ProgramProjection::<Steps>::PROGRAM_REF, ROLE, Self::FACTS)
    }

    const IMAGE_REF: crate::global::role_program::RoleImageRef = if Self::BLOB_LEN <= 32 {
        Self::image_ref::<32>()
    } else if Self::BLOB_LEN <= 64 {
        Self::image_ref::<64>()
    } else if Self::BLOB_LEN <= 96 {
        Self::image_ref::<96>()
    } else if Self::BLOB_LEN <= 128 {
        Self::image_ref::<128>()
    } else if Self::BLOB_LEN <= 192 {
        Self::image_ref::<192>()
    } else if Self::BLOB_LEN <= 256 {
        Self::image_ref::<256>()
    } else if Self::BLOB_LEN <= 384 {
        Self::image_ref::<384>()
    } else if Self::BLOB_LEN <= 512 {
        Self::image_ref::<512>()
    } else if Self::BLOB_LEN <= 1024 {
        Self::image_ref::<1024>()
    } else if Self::BLOB_LEN <= 2048 {
        Self::image_ref::<2048>()
    } else if Self::BLOB_LEN <= 4096 {
        Self::image_ref::<4096>()
    } else if Self::BLOB_LEN <= 8192 {
        Self::image_ref::<8192>()
    } else {
        panic!("role bucket")
    };
}

impl<const ROLE: u8, Steps, const N: usize> RoleProjectionBlob<ROLE, Steps, N>
where
    Steps: ProgramTerm,
{
    const BUILD: Option<crate::global::role_program::RoleImageBuild<N>> =
        RoleProjection::<ROLE, Steps>::PLAN.build_if_fits::<N>(
            ProgramProjection::<Steps>::SOURCE_EFF_LIST,
            RoleProjection::<ROLE, Steps>::FACTS,
            ROLE,
        );
}

#[inline(always)]
pub(super) const fn role_projection_image_for<const ROLE: u8, Steps>()
-> &'static crate::global::role_program::RoleImageRef
where
    Steps: ProgramTerm,
{
    &RoleProjection::<ROLE, Steps>::IMAGE_REF
}
