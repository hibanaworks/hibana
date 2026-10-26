use core::marker::PhantomData;

use super::{ProgramProjection, ProgramShape};
use crate::global::compiled::images::COMPACT_DESCRIPTOR_BYTE_CAPACITY;
use crate::global::role_program::ROLE_IMAGE_STORAGE_BYTE_CAPACITY;

#[cfg(kani)]
mod kani;
mod source_order;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

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
        crate::global::compiled::images::ProgramImageBytes::<N>::from_source_if_fits(
            &ProgramProjection::<Steps, CAPACITY>::SOURCE,
            ProgramProjection::<Steps, CAPACITY>::PROGRAM_COLUMNS,
        );
}

impl<Steps, const CAPACITY: usize> ProgramProjection<Steps, CAPACITY>
where
    Steps: ProgramShape,
{
    const SCOPE_FACTS: &'static crate::global::role_program::ScopeFacts =
        &crate::global::role_program::ScopeFacts::new(Self::SOURCE_EFF_LIST);

    const PROGRAM_PLAN: crate::global::compiled::images::ProgramImagePlan =
        crate::global::compiled::images::ProgramImagePlan::from_source(&Self::SOURCE);
    const PROGRAM_COLUMNS: crate::global::compiled::images::ProgramImageColumns =
        Self::PROGRAM_PLAN.columns();
    const PROGRAM_BLOB_LEN: usize = Self::PROGRAM_PLAN.blob_len();
    const fn program_ref<const N: usize>() -> crate::global::compiled::images::CompiledProgramRef {
        let bytes = match &ProgramProjectionBlob::<Steps, CAPACITY, N>::BYTES {
            Some(bytes) => bytes,
            None => panic!("program bucket selection"),
        };
        let program = bytes.compiled_ref(&Self::IMAGE, Self::PROGRAM_COLUMNS);
        if !program.covers_source_counts(
            Steps::EVENT_COUNT,
            Steps::SCOPE_MARKER_COUNT,
            Steps::RESOLVER_MARKER_COUNT,
        ) {
            crate::invariant();
        }
        program
    }

    const PROGRAM_REF: crate::global::compiled::images::CompiledProgramRef = {
        // Build immutable facts before semantic validation so that ownership-
        // aware checks can consume actor projections. Only the public witness
        // boundary below certifies execution; a byte image is not that witness.
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
    const ACTOR: u8 =
        match ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF.initial_actor_for(ROLE) {
            Some(actor) => actor,
            None => ROLE,
        };
    // Keeping the guard inside the constant prevents ordinary large programs
    // from eagerly constructing an unused actor map during const evaluation.
    const ACTOR_ROWS: Option<crate::global::role_program::ActorLocalRows<CAPACITY>> = {
        if Steps::SOURCE_NODE.has_delegation() {
            Some(crate::global::role_program::ActorLocalRows::new(
                &ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF,
                Self::ACTOR,
            ))
        } else {
            None
        }
    };
    const ROWS: crate::global::role_program::LocalRows<'static, CAPACITY> = match &Self::ACTOR_ROWS
    {
        Some(rows) => crate::global::role_program::LocalRows::Actor(rows),
        None => crate::global::role_program::LocalRows::Role(Self::ACTOR),
    };
    const COUNTS: crate::global::compiled::lowering::RoleCompiledCounts =
        ProgramProjection::<Steps, CAPACITY>::IMAGE.role_lowering_counts(
            ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
            ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
            Self::ROWS,
        );
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);
    const PLAN: crate::global::role_program::RoleImagePlan =
        crate::global::role_program::RoleImagePlan::from_program(
            (
                ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
                ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
            ),
            Self::FACTS,
            Self::ROWS,
        );
    const BLOB_LEN: usize = Self::PLAN.blob_len();

    #[cfg(hibana_profile)]
    fn profile_emit_bytes<const N: usize>() -> bool {
        let build = core::hint::black_box(&Self::PLAN).build_if_fits::<N, CAPACITY>(
            (
                core::hint::black_box(ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST),
                core::hint::black_box(ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS),
            ),
            core::hint::black_box(Self::FACTS),
            core::hint::black_box(Self::ROWS),
        );
        let fits = build.is_some();
        core::hint::black_box(build);
        fits
    }

    const fn image_ref<const N: usize>() -> crate::global::role_program::RoleImageRef {
        let build = match &RoleProjectionBlob::<ROLE, Steps, CAPACITY, N>::BUILD {
            Some(build) => build,
            None => panic!("role bucket selection"),
        };
        build.image_ref(
            &ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF,
            Self::ACTOR,
            Self::FACTS,
        )
    }

    const BUILT_IMAGE_REF: crate::global::role_program::RoleImageRef = if Self::BLOB_LEN <= 32 {
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
    } else if Self::BLOB_LEN <= ROLE_IMAGE_STORAGE_BYTE_CAPACITY {
        Self::image_ref::<{ ROLE_IMAGE_STORAGE_BYTE_CAPACITY }>()
    } else {
        panic!("role image exceeds compact offset domain")
    };

    const SOURCE_ORDER_VALID: bool = source_order::check::<Steps, CAPACITY>(&Self::BUILT_IMAGE_REF);

    const IMAGE_REF: crate::global::role_program::RoleImageRef = {
        if !Self::SOURCE_ORDER_VALID {
            panic!("actor projection does not preserve source operation dependencies");
        }
        Self::BUILT_IMAGE_REF
    };
}

impl<const ROLE: u8, Steps, const CAPACITY: usize, const N: usize>
    RoleProjectionBlob<ROLE, Steps, CAPACITY, N>
where
    Steps: ProgramShape,
{
    const BUILD: Option<crate::global::role_program::RoleImageBuild<N>> =
        RoleProjection::<ROLE, Steps, CAPACITY>::PLAN.build_if_fits(
            (
                ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
                ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
            ),
            RoleProjection::<ROLE, Steps, CAPACITY>::FACTS,
            RoleProjection::<ROLE, Steps, CAPACITY>::ROWS,
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
    let () = ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    &RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF
}

#[cfg(hibana_profile)]
pub(super) fn profile_local_pass<const ROLE: u8, Steps, const CAPACITY: usize>(
    pass: super::profiling::LocalPass,
) -> bool
where
    Steps: ProgramShape,
{
    let image = core::hint::black_box(role_projection_image_for::<ROLE, Steps, CAPACITY>());
    match pass {
        super::profiling::LocalPass::OperationOrder => {
            source_order::check::<Steps, CAPACITY>(image)
        }
        super::profiling::LocalPass::RolePlan => {
            let plan = crate::global::role_program::RoleImagePlan::from_program(
                (
                    core::hint::black_box(ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST),
                    core::hint::black_box(ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS),
                ),
                core::hint::black_box(RoleProjection::<ROLE, Steps, CAPACITY>::FACTS),
                core::hint::black_box(RoleProjection::<ROLE, Steps, CAPACITY>::ROWS),
            );
            plan.blob_len() == RoleProjection::<ROLE, Steps, CAPACITY>::BLOB_LEN
        }
        super::profiling::LocalPass::ResidentValidation => {
            image.profile_resident_validation();
            true
        }
        super::profiling::LocalPass::ImageEmission => {
            let emit: fn() -> bool = const {
                match RoleProjection::<ROLE, Steps, CAPACITY>::BLOB_LEN {
                    0..=32 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<32>,
                    33..=64 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<64>,
                    65..=96 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<96>,
                    97..=128 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<128>,
                    129..=192 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<192>,
                    193..=256 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<256>,
                    257..=384 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<384>,
                    385..=512 => RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<512>,
                    513..=1024 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<1024>
                    }
                    1025..=2048 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<2048>
                    }
                    2049..=4096 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<4096>
                    }
                    4097..=8192 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<8192>
                    }
                    8193..=16384 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<16384>
                    }
                    16385..=32768 => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<32768>
                    }
                    32769..=COMPACT_DESCRIPTOR_BYTE_CAPACITY => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<
                            { COMPACT_DESCRIPTOR_BYTE_CAPACITY },
                        >
                    }
                    65536..=ROLE_IMAGE_STORAGE_BYTE_CAPACITY => {
                        RoleProjection::<ROLE, Steps, CAPACITY>::profile_emit_bytes::<
                            { ROLE_IMAGE_STORAGE_BYTE_CAPACITY },
                        >
                    }
                    _ => panic!("role image exceeds compact offset domain"),
                }
            };
            emit()
        }
    }
}
