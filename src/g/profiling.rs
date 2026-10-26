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
    /// Number of messages in the global protocol.
    pub events: usize,
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
}

pub(crate) struct Counters {
    pub full_inspections: usize,
    pub resumed_inspections: usize,
    pub alignment_attempts: usize,
    pub recolorings: usize,
}

impl Counters {
    pub(crate) const fn new() -> Self {
        Self {
            full_inspections: 0,
            resumed_inspections: 0,
            alignment_attempts: 0,
            recolorings: 0,
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
        if rows <= 8 {
            super::source::profile::<P::Steps, 8>
        } else if rows <= 32 {
            super::source::profile::<P::Steps, 32>
        } else if rows <= 128 {
            super::source::profile::<P::Steps, 128>
        } else if rows <= 512 {
            super::source::profile::<P::Steps, 512>
        } else if rows <= 2048 {
            super::source::profile::<P::Steps, 2048>
        } else if rows <= 8192 {
            super::source::profile::<P::Steps, 8192>
        } else if rows <= 32768 {
            super::source::profile::<P::Steps, 32768>
        } else if rows <= 65535 {
            super::source::profile::<P::Steps, 65535>
        } else {
            panic!("choreography source exceeds compact descriptor domain")
        }
    };
    lower()
}

/// Rerun source-order validation for one projected participant.
///
/// This runs the same validator as projection, natively over the compiled image.
/// Source emission, image construction and the other projection checks remain
/// compile-time work. Time the call externally and sample it in a separate run.
pub fn operation_order<const ROLE: u8>(program: &impl Projectable) -> bool {
    operation_order_for::<ROLE, _>(program)
}

fn operation_order_for<const ROLE: u8, P: Projectable>(_program: &P) -> bool {
    let validate: fn() -> bool = const {
        let rows = P::Steps::SOURCE_ROW_COUNT;
        if rows <= 8 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 8>
        } else if rows <= 32 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 32>
        } else if rows <= 128 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 128>
        } else if rows <= 512 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 512>
        } else if rows <= 2048 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 2048>
        } else if rows <= 8192 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 8192>
        } else if rows <= 32768 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 32768>
        } else if rows <= 65535 {
            super::role_projection::profile_operation_order::<ROLE, P::Steps, 65535>
        } else {
            panic!("choreography source exceeds compact descriptor domain")
        }
    };
    validate()
}

#[cfg(test)]
mod tests {
    use super::{operation_order, source};
    use crate::{Endpoint, g};

    #[test]
    fn operation_order_uses_the_projected_actor_for_delegated_roles() {
        type Loan = g::Msg<1, Endpoint<'static, 2>>;
        let global = g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(g::send::<2, 3, g::Msg<2, ()>>(), g::send::<1, 0, Loan>()),
        );
        assert!(operation_order::<0>(&global));
        assert!(operation_order::<1>(&global));
        assert!(operation_order::<2>(&global));
        assert!(operation_order::<3>(&global));
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
    fn ordinary_recoloring_counts_full_inspections() {
        let global = g::seq(
            g::send::<0, 2, g::Msg<1, ()>>(),
            g::send::<1, 2, g::Msg<2, ()>>(),
        );
        let result = source(&global);
        assert!(result.receive_causal);
        assert_eq!((result.events, result.lanes), (2, 2));
        assert_eq!(result.full_inspections, 2);
        assert_eq!(result.resumed_inspections, 0);
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
