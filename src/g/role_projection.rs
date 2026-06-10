use core::marker::PhantomData;

use super::{ProgramProjection, ProgramTerm};

struct RoleProjection<const ROLE: u8, Steps>(PhantomData<Steps>);
struct ProgramProjectionBlob<Steps, const N: usize>(PhantomData<Steps>);
struct RoleProjectionBlob<const ROLE: u8, Steps, const N: usize>(PhantomData<Steps>);

impl<Steps, const N: usize> ProgramProjectionBlob<Steps, N>
where
    Steps: ProgramTerm,
{
    const BYTES: crate::global::compiled::images::ProgramImageBytes<N> =
        crate::global::compiled::images::ProgramImageBytes::<N>::from_unselected_bucket_or_empty(
            &ProgramProjection::<Steps>::IMAGE,
        );
}

impl<Steps> ProgramProjection<Steps>
where
    Steps: ProgramTerm,
{
    const PROGRAM_BLOB_LEN: usize =
        crate::global::compiled::images::ProgramImageBytes::<0>::projected_len(
            &ProgramProjection::<Steps>::IMAGE,
        );

    const PROGRAM_REF: crate::global::compiled::images::CompiledProgramRef =
        if Self::PROGRAM_BLOB_LEN <= 32 {
            let bytes = &ProgramProjectionBlob::<Steps, 32>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 64 {
            let bytes = &ProgramProjectionBlob::<Steps, 64>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 96 {
            let bytes = &ProgramProjectionBlob::<Steps, 96>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 128 {
            let bytes = &ProgramProjectionBlob::<Steps, 128>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 192 {
            let bytes = &ProgramProjectionBlob::<Steps, 192>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 256 {
            let bytes = &ProgramProjectionBlob::<Steps, 256>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 384 {
            let bytes = &ProgramProjectionBlob::<Steps, 384>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 512 {
            let bytes = &ProgramProjectionBlob::<Steps, 512>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 1024 {
            let bytes = &ProgramProjectionBlob::<Steps, 1024>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 2048 {
            let bytes = &ProgramProjectionBlob::<Steps, 2048>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 4096 {
            let bytes = &ProgramProjectionBlob::<Steps, 4096>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else if Self::PROGRAM_BLOB_LEN <= 8192 {
            let bytes = &ProgramProjectionBlob::<Steps, 8192>::BYTES;
            bytes.compiled_ref(&ProgramProjection::<Steps>::IMAGE)
        } else {
            panic!("program bucket")
        };
}

impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>
where
    Steps: ProgramTerm,
{
    const COUNTS: crate::global::compiled::lowering::RoleCompiledCounts =
        ProgramProjection::<Steps>::IMAGE.role_lowering_counts::<ROLE>();
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);
    const SCRATCH: crate::global::role_program::RoleLaneScratch =
        crate::global::role_program::RoleLaneScratch::from_program::<ROLE>(
            &ProgramProjection::<Steps>::IMAGE,
            Self::FACTS.footprint().logical_lane_count,
        );
    const BLOB_LEN: usize =
        crate::global::role_program::RoleImageBytes::<0>::projected_len(Self::SCRATCH, Self::FACTS);
    const IMAGE_REF: crate::global::role_program::RoleImageRef = if Self::BLOB_LEN <= 32 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 32>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 64 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 64>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 96 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 96>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 128 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 128>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 192 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 192>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 256 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 256>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 384 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 384>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 512 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 512>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 1024 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 1024>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 2048 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 2048>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 4096 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 4096>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else if Self::BLOB_LEN <= 8192 {
        let bytes = &RoleProjectionBlob::<ROLE, Steps, 8192>::BYTES;
        bytes.image_ref(
            &ProgramProjection::<Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
        )
    } else {
        panic!("role bucket")
    };
}

impl<const ROLE: u8, Steps, const N: usize> RoleProjectionBlob<ROLE, Steps, N>
where
    Steps: ProgramTerm,
{
    const BYTES: crate::global::role_program::RoleImageBytes<N> =
        crate::global::role_program::RoleImageBytes::<N>::from_unselected_bucket_or_empty(
            RoleProjection::<ROLE, Steps>::SCRATCH,
            RoleProjection::<ROLE, Steps>::FACTS,
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
