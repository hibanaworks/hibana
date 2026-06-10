use core::marker::PhantomData;

use super::{ProgramProjection, ProgramTerm};

struct RoleProjection<const ROLE: u8, Steps>(PhantomData<Steps>);
struct ProgramProjectionBlob<Steps, const N: usize>(PhantomData<Steps>);
struct RoleProjectionBlob<const ROLE: u8, Steps, const N: usize>(PhantomData<Steps>);

impl<Steps, const N: usize> ProgramProjectionBlob<Steps, N>
where
    Steps: ProgramTerm,
{
    const BLOB: crate::global::compiled::images::ProgramImageBlobStorage<N> =
        crate::global::compiled::images::ProgramImageBlobStorage::<N>::from_unselected_bucket_or_empty(
            &ProgramProjection::<Steps>::IMAGE,
        );
}

impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>
where
    Steps: ProgramTerm,
{
    const COUNTS: crate::global::compiled::lowering::RoleCompiledCounts =
        ProgramProjection::<Steps>::IMAGE.role_lowering_counts::<ROLE>();
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);

    const PROGRAM_BLOB_LEN: usize =
        crate::global::compiled::images::ProgramImageBlobStorage::<0>::projected_len(
            &ProgramProjection::<Steps>::IMAGE,
        );
    const PROGRAM_REF: crate::global::compiled::images::CompiledProgramRef =
        if Self::PROGRAM_BLOB_LEN <= 32 {
            let blob = &ProgramProjectionBlob::<Steps, 32>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 64 {
            let blob = &ProgramProjectionBlob::<Steps, 64>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 96 {
            let blob = &ProgramProjectionBlob::<Steps, 96>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 128 {
            let blob = &ProgramProjectionBlob::<Steps, 128>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 192 {
            let blob = &ProgramProjectionBlob::<Steps, 192>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 256 {
            let blob = &ProgramProjectionBlob::<Steps, 256>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 384 {
            let blob = &ProgramProjectionBlob::<Steps, 384>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 512 {
            let blob = &ProgramProjectionBlob::<Steps, 512>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 1024 {
            let blob = &ProgramProjectionBlob::<Steps, 1024>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 2048 {
            let blob = &ProgramProjectionBlob::<Steps, 2048>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 4096 {
            let blob = &ProgramProjectionBlob::<Steps, 4096>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else if Self::PROGRAM_BLOB_LEN <= 8192 {
            let blob = &ProgramProjectionBlob::<Steps, 8192>::BLOB;
            crate::global::compiled::images::CompiledProgramRef::compact(
                blob.facts,
                blob.columns,
                blob.blob(),
            )
        } else {
            panic!("program bucket")
        };
    const SCRATCH: crate::global::role_program::RoleLaneScratch =
        crate::global::role_program::RoleLaneScratch::from_program::<ROLE>(
            &ProgramProjection::<Steps>::IMAGE,
            Self::FACTS.footprint().logical_lane_count,
        );
    const BLOB_LEN: usize = crate::global::role_program::RoleImageBlobStorage::<0>::projected_len(
        Self::SCRATCH,
        Self::FACTS,
    );
    const IMAGE_REF: crate::global::role_program::RoleImageRef = if Self::BLOB_LEN <= 32 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 32>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 64 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 64>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 96 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 96>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 128 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 128>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 192 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 192>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 256 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 256>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 384 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 384>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 512 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 512>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 1024 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 1024>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 2048 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 2048>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 4096 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 4096>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else if Self::BLOB_LEN <= 8192 {
        let blob = &RoleProjectionBlob::<ROLE, Steps, 8192>::BLOB;
        crate::global::role_program::RoleImageRef::new(
            RoleProjection::<ROLE, Steps>::PROGRAM_REF,
            ROLE,
            RoleProjection::<ROLE, Steps>::FACTS,
            blob.columns,
            blob.blob(),
            blob.active_lane_row,
            blob.first_active_lane,
        )
    } else {
        panic!("role bucket")
    };
}

impl<const ROLE: u8, Steps, const N: usize> RoleProjectionBlob<ROLE, Steps, N>
where
    Steps: ProgramTerm,
{
    const BLOB: crate::global::role_program::RoleImageBlobStorage<N> =
        crate::global::role_program::RoleImageBlobStorage::<N>::from_unselected_bucket_or_empty(
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
