use core::marker::PhantomData;

use super::{ProgramProjection, ProgramShape};
use crate::global::compiled::images::COMPACT_DESCRIPTOR_BYTE_CAPACITY;

struct RoleProjection<const ROLE: u8, Steps, const CAPACITY: usize>(PhantomData<Steps>);
struct ProgramProjectionBlob<Steps, const CAPACITY: usize, const N: usize>(PhantomData<Steps>);
struct RoleProjectionBlob<const ROLE: u8, Steps, const CAPACITY: usize, const N: usize>(
    PhantomData<Steps>,
);

impl<Steps, const CAPACITY: usize, const N: usize> ProgramProjectionBlob<Steps, CAPACITY, N>
where
    Steps: ProgramShape,
{
    const BYTES: Option<crate::global::compiled::images::ProgramImageBytes<N>> =
        crate::global::compiled::images::ProgramImageBytes::<N>::from_image_if_fits(
            ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
            ProgramProjection::<Steps, CAPACITY>::PROGRAM_COLUMNS,
        );
}

impl<Steps, const CAPACITY: usize> ProgramProjection<Steps, CAPACITY>
where
    Steps: ProgramShape,
{
    const PROGRAM_PLAN: crate::global::compiled::images::ProgramImagePlan =
        crate::global::compiled::images::ProgramImagePlan::from_program(Self::SOURCE_EFF_LIST);
    const PROGRAM_COLUMNS: crate::global::compiled::images::ProgramImageColumns =
        Self::PROGRAM_PLAN.columns();
    const PROGRAM_BLOB_LEN: usize = Self::PROGRAM_PLAN.blob_len();
    const SOURCE_COUNTS_COVERED: () = {
        if !Self::PROGRAM_COLUMNS.covers_source_counts(
            Steps::EVENT_COUNT,
            Steps::SCOPE_MARKER_COUNT,
            Steps::RESOLVER_MARKER_COUNT,
        ) {
            crate::invariant();
        }
    };

    const fn program_ref<const N: usize>() -> crate::global::compiled::images::CompiledProgramRef {
        let bytes = match &ProgramProjectionBlob::<Steps, CAPACITY, N>::BYTES {
            Some(bytes) => bytes,
            None => panic!("program bucket selection"),
        };
        bytes.compiled_ref(&Self::IMAGE, Self::PROGRAM_COLUMNS)
    }

    const PROGRAM_REF: crate::global::compiled::images::CompiledProgramRef = {
        let () = Self::SOURCE_COUNTS_COVERED;
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
        } else if Self::PROGRAM_BLOB_LEN <= 16384 {
            Self::program_ref::<16384>()
        } else if Self::PROGRAM_BLOB_LEN <= 32768 {
            Self::program_ref::<32768>()
        } else if Self::PROGRAM_BLOB_LEN <= COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            Self::program_ref::<{ COMPACT_DESCRIPTOR_BYTE_CAPACITY }>()
        } else {
            panic!("program image exceeds compact offset domain")
        }
    };
}

impl<const ROLE: u8, Steps, const CAPACITY: usize> RoleProjection<ROLE, Steps, CAPACITY>
where
    Steps: ProgramShape,
{
    const COUNTS: crate::global::compiled::lowering::RoleCompiledCounts =
        ProgramProjection::<Steps, CAPACITY>::IMAGE
            .role_lowering_counts(ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST, ROLE);
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);
    const PLAN: crate::global::role_program::RoleImagePlan =
        crate::global::role_program::RoleImagePlan::from_program(
            ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
            Self::FACTS,
            ROLE,
        );
    const BLOB_LEN: usize = Self::PLAN.blob_len();

    const fn image_ref<const N: usize>() -> crate::global::role_program::RoleImageRef {
        let build = match &RoleProjectionBlob::<ROLE, Steps, CAPACITY, N>::BUILD {
            Some(build) => build,
            None => panic!("role bucket selection"),
        };
        build.image_ref(
            &ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF,
            ROLE,
            Self::FACTS,
        )
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
    } else if Self::BLOB_LEN <= 16384 {
        Self::image_ref::<16384>()
    } else if Self::BLOB_LEN <= 32768 {
        Self::image_ref::<32768>()
    } else if Self::BLOB_LEN <= COMPACT_DESCRIPTOR_BYTE_CAPACITY {
        Self::image_ref::<{ COMPACT_DESCRIPTOR_BYTE_CAPACITY }>()
    } else {
        panic!("role image exceeds compact offset domain")
    };
}

impl<const ROLE: u8, Steps, const CAPACITY: usize, const N: usize>
    RoleProjectionBlob<ROLE, Steps, CAPACITY, N>
where
    Steps: ProgramShape,
{
    const BUILD: Option<crate::global::role_program::RoleImageBuild<N>> =
        RoleProjection::<ROLE, Steps, CAPACITY>::PLAN.build_if_fits(
            ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
            RoleProjection::<ROLE, Steps, CAPACITY>::FACTS,
            ROLE,
        );
}

#[inline(always)]
pub(super) const fn role_projection_image_for<const ROLE: u8, Steps, const CAPACITY: usize>()
-> &'static crate::global::role_program::RoleImageRef
where
    Steps: ProgramShape,
{
    if !ProgramProjection::<Steps, CAPACITY>::IMAGE.contains_role(ROLE) {
        panic!("projected role is outside the choreography role range");
    }
    &RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF
}
