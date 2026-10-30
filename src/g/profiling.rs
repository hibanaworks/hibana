//! Native measurements of source lowering, enabled with `--cfg hibana_profile`.
//! The source and actor image use the same constants as `project`. Lane
//! assignment runs natively so host profilers can inspect its call stacks.
//! A normal build still checks projection through const evaluation.

use super::ProgramShape;
use crate::runtime::program::Projectable;

/// Work performed while assigning receive lanes to one source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceProfile {
    /// Number of source events and structural markers.
    pub source_rows: usize,
    /// Bytes of compiler workspace used by the local dependency scan.
    pub dependency_workspace_bytes: usize,
    /// Number of messages in the global protocol.
    pub events: usize,
    /// Number of conditional owner expressions in the compiled protocol.
    pub actor_decisions: usize,
    /// Number of lanes used by the assigned source.
    pub lanes: usize,
    /// Whether the assigned lanes preserve receive order.
    pub receive_causal: bool,
    /// Inspections that start at the beginning of the source.
    pub full_inspections: usize,
    /// Inspections that resume at a previously rejected receive.
    pub resumed_inspections: usize,
    /// Attempts to assign new lanes to an arm or individual event.
    pub alignment_attempts: usize,
    /// Successful changes to the lane assignment.
    pub recolorings: usize,
    /// Pointwise causal queries while checking proposed lane assignments.
    pub alignment_pair_queries: usize,
    /// Receive candidates inspected while refining proposed lane assignments.
    pub alignment_candidate_inspections: usize,
}

pub(crate) struct Counters {
    pub full_inspections: usize,
    pub resumed_inspections: usize,
    pub alignment_attempts: usize,
    pub recolorings: usize,
    pub alignment_pair_queries: usize,
    pub alignment_candidate_inspections: usize,
}

impl Counters {
    pub(crate) const fn new() -> Self {
        Self {
            full_inspections: 0,
            resumed_inspections: 0,
            alignment_attempts: 0,
            recolorings: 0,
            alignment_pair_queries: 0,
            alignment_candidate_inspections: 0,
        }
    }
}

/// Measure lane-assignment work for the same source used by `project`.
///
/// Time this call externally with `std::time::Instant` and pass the input and
/// output through `std::hint::black_box`. Cached source emission and actor-image
/// construction happen at compile time and are outside this native measurement.
pub fn source<P: Projectable>(program: &P) -> SourceProfile {
    let _ = program;
    let lower: fn() -> SourceProfile = const {
        let rows = P::Steps::SOURCE_ROW_COUNT;
        with_source_capacity!(
            rows,
            |CAPACITY| super::source::profile::<P::Steps, CAPACITY>
        )
    };
    lower()
}

/// Rerun source emission with the compiled actor bindings.
pub fn source_emission(program: &impl Projectable) -> bool {
    construction_pass(program, SourcePass::Emission)
}

/// Rebuild the source's actor bindings from the typed protocol.
pub fn actor_image(program: &impl Projectable) -> bool {
    construction_pass(program, SourcePass::Actors)
}

/// Check resource-dependent repetition using the retained ownership facts.
/// Source ownership compilation remains outside this native measurement.
pub fn reentry_knowledge(program: &impl Projectable) -> bool {
    construction_pass(program, SourcePass::Reentry)
}

/// Rebuild the shared scope information used by local projection.
pub fn scope_facts(program: &impl Projectable) -> bool {
    construction_pass(program, SourcePass::Scopes)
}

pub(super) enum SourcePass {
    Emission,
    Actors,
    Reentry,
    Scopes,
}

fn construction_pass<P: Projectable>(_program: &P, pass: SourcePass) -> bool {
    let run: fn(SourcePass) -> bool = const {
        let rows = P::Steps::SOURCE_ROW_COUNT;
        with_source_capacity!(rows, |CAPACITY| super::source::profile_construction::<
            P::Steps,
            CAPACITY,
        >)
    };
    run(pass)
}

/// Rerun source-order validation for one projected participant.
///
/// This runs the same validator as projection, natively over the compiled image.
/// Source emission, image construction and the other projection checks remain
/// compile-time work. Time the call externally and sample it in a separate run.
pub fn operation_order<const ROLE: u8>(program: &impl Projectable) -> bool {
    local_pass::<ROLE, _, 0>(program)
}

/// Rerun local image planning and compare its size with the compiled image.
///
/// Source emission, actor rows and role counts are cached at compile time.
pub fn role_plan<const ROLE: u8>(program: &impl Projectable) -> bool {
    local_pass::<ROLE, _, 1>(program)
}

/// Rewrite the local image using the same buffer size as normal projection.
///
/// Planning, source emission and actor rows are cached at compile time.
pub fn image_emission<const ROLE: u8>(program: &impl Projectable) -> bool {
    local_pass::<ROLE, _, 3>(program)
}

/// Rerun the compiled image's event and ownership validation.
///
/// This uses the same immutable image accepted by normal projection.
pub fn resident_validation<const ROLE: u8>(program: &impl Projectable) -> bool {
    local_pass::<ROLE, _, 2>(program)
}

fn local_pass<const ROLE: u8, P: Projectable, const PASS: u8>(_program: &P) -> bool {
    let validate: fn() -> bool = const {
        let rows = P::Steps::SOURCE_ROW_COUNT;
        with_source_capacity!(
            rows,
            |CAPACITY| super::role_projection::profile_local_pass::<ROLE, P::Steps, CAPACITY, PASS>
        )
    };
    validate()
}

#[cfg(test)]
mod tests {
    use super::{
        actor_image, image_emission, operation_order, resident_validation, role_plan, scope_facts,
        source, source_emission,
    };
    use crate::{Endpoint, g};

    #[test]
    fn operation_order_uses_the_projected_actor_for_delegated_roles() {
        type Loan = g::Msg<1, Endpoint<'static, 2>>;
        let global = g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(g::send::<2, 3, g::Msg<2, ()>>(), g::send::<1, 0, Loan>()),
        );
        assert!(source_emission(&global));
        assert!(actor_image(&global));
        assert!(scope_facts(&global));
        assert!(operation_order::<0>(&global));
        assert!(operation_order::<1>(&global));
        assert!(operation_order::<2>(&global));
        assert!(operation_order::<3>(&global));
        assert!(role_plan::<0>(&global));
        assert!(role_plan::<1>(&global));
        assert!(role_plan::<2>(&global));
        assert!(role_plan::<3>(&global));
        assert!(image_emission::<0>(&global));
        assert!(image_emission::<1>(&global));
        assert!(image_emission::<2>(&global));
        assert!(image_emission::<3>(&global));
        assert!(resident_validation::<0>(&global));
        assert!(resident_validation::<1>(&global));
        assert!(resident_validation::<2>(&global));
        assert!(resident_validation::<3>(&global));
    }

    #[test]
    fn ordinary_source_needs_one_inspection() {
        let global = g::seq(
            g::send::<0, 1, g::Msg<1, u32>>(),
            g::send::<1, 0, g::Msg<2, u32>>(),
        );
        let result = source(&global);
        assert!(result.receive_causal);
        assert_eq!((result.source_rows, result.events, result.lanes), (2, 2, 1));
        assert_eq!(result.full_inspections, 1);
        assert_eq!(result.resumed_inspections, 0);
        assert_eq!(result.alignment_attempts, 0);
        assert_eq!(result.recolorings, 0);
    }

    #[test]
    fn ordinary_recoloring_counts_partitioned_inspections() {
        let global = g::seq(
            g::send::<0, 2, g::Msg<1, ()>>(),
            g::send::<1, 2, g::Msg<2, ()>>(),
        );
        let result = source(&global);
        assert!(result.receive_causal);
        assert_eq!((result.events, result.lanes), (2, 2));
        assert_eq!(result.full_inspections, 2);
        assert_eq!(result.resumed_inspections, 1);
        assert_eq!(result.recolorings, 1);
    }

    #[test]
    fn delegated_source_reports_repairs_and_final_validation() {
        type Loan = g::Msg<240, Endpoint<'static, 36>>;
        let global = g::seq(
            g::send::<3, 2, g::Msg<160, ()>>(),
            g::seq(
                g::send::<1, 2, Loan>(),
                g::seq(
                    g::send::<36, 1, g::Msg<241, ()>>(),
                    g::send::<1, 36, g::Msg<249, ()>>(),
                ),
            ),
        );
        let result = source(&global);
        assert!(result.receive_causal);
        assert_eq!((result.source_rows, result.events), (4, 4));
        assert_eq!(result.full_inspections, 2);
        assert_ne!(result.recolorings, 0);
        assert_eq!(result.resumed_inspections, result.recolorings);
        assert!(result.alignment_attempts >= result.recolorings);
    }
}
