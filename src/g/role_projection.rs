use core::marker::PhantomData;

use super::{ProgramProjection, ProgramShape};
use crate::global::compiled::images::COMPACT_DESCRIPTOR_BYTE_CAPACITY;

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
    const SCOPE_FACTS: crate::global::role_program::ScopeFacts =
        crate::global::role_program::ScopeFacts::new(Self::SOURCE_EFF_LIST);

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
            &ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
            Self::ROWS,
        );
    const FACTS: crate::global::role_program::RuntimeRoleFacts =
        crate::global::role_program::RuntimeRoleFacts::from_counts(Self::COUNTS);
    const PLAN: crate::global::role_program::RoleImagePlan =
        crate::global::role_program::RoleImagePlan::from_program(
            (
                ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
                &ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
            ),
            Self::FACTS,
            Self::ROWS,
        );
    const BLOB_LEN: usize = Self::PLAN.blob_len();

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
            (
                ProgramProjection::<Steps, CAPACITY>::SOURCE_EFF_LIST,
                &ProgramProjection::<Steps, CAPACITY>::SCOPE_FACTS,
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
    // Owned operations and shared actor progress execute in private tests.
    // Public issuance still requires the full acceptance/continuation proof and
    // initial binding boundary. Static checks alone do not qualify execution.
    if Steps::SOURCE_NODE.has_delegation() {
        panic!("owned endpoint projection is not yet qualified");
    }
    let () = ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    &RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF
}
