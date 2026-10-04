//! Read-only source diagnostics. These values are not projection evidence.
use super::{Program, ProgramProjection, ProgramShape, ProgramSourceError};

/// The obligation that prevented projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionProblem {
    /// Both arms must have the same first-visible controller.
    RouteControllerMismatch,
    /// A shared receive lane changes sender without a causal handoff.
    ReceiveLaneCausality,
    /// Parallel endpoint selectors overlap.
    ParallelSelector,
    /// Rolled reentry selectors overlap.
    ReentrySelector,
    /// A route is ambiguous or cannot be projected.
    UnprojectableRoute,
    /// A non-controller cannot distinguish or merge the route arms.
    MissingBranchKnowledge,
}
/// A half-open range in the global source's preorder send sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticRange {
    /// Inclusive source event ordinal.
    pub start: usize,
    /// Exclusive source event ordinal.
    pub end: usize,
}
/// A source send, preserving the user's logical label (not a wire frame label).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticEvent {
    /// Zero-based source send ordinal.
    pub index: usize,
    /// Sending role.
    pub from: u8,
    /// Receiving role.
    pub to: u8,
    /// User-declared message label.
    pub label: u8,
    /// Lowered transport lane.
    pub lane: u8,
}
impl DiagnosticEvent {
    pub(crate) const fn from_atom(index: usize, atom: crate::eff::EffAtom) -> Self {
        Self {
            index,
            from: atom.from,
            to: atom.to,
            label: atom.label,
            lane: atom.lane,
        }
    }
}
/// Structured explanation of a rejection. Missing witnesses stay `None`.
///
/// For a lane conflict, `first` and `second` are the conflicting receives;
/// `scope` is present when the second receive occurs on rolled reentry.
/// For missing branch knowledge, `arms` gives both source ranges and each event
/// is that role's first source send/receive in the corresponding arm, if any.
/// Without an affected role, route events are the first source events of each arm.
/// These representative route events need not themselves be the ambiguous pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionDiagnostic {
    /// Rejected obligation.
    pub problem: ProjectionProblem,
    /// Role whose receive lane or route knowledge is insufficient, if located.
    pub role: Option<u8>,
    /// Preorder structured scope ordinal, if located.
    pub scope: Option<u16>,
    /// Route controller, when unique.
    pub controller: Option<u8>,
    /// Source event ranges for the left and right route arms.
    pub arms: Option<[DiagnosticRange; 2]>,
    /// Earlier receive, or representative local event from the left route arm.
    pub first: Option<DiagnosticEvent>,
    /// Later receive, or representative local event from the right route arm.
    pub second: Option<DiagnosticEvent>,
}
impl ProjectionDiagnostic {
    pub(crate) const fn from_error(error: ProgramSourceError) -> Self {
        let problem = match error {
            ProgramSourceError::RouteControllerMismatch => {
                ProjectionProblem::RouteControllerMismatch
            }
            ProgramSourceError::ReceiveLaneCausalityConflict => {
                ProjectionProblem::ReceiveLaneCausality
            }
            ProgramSourceError::ParallelAmbiguousEndpointSelector => {
                ProjectionProblem::ParallelSelector
            }
            ProgramSourceError::ReentryAmbiguousEndpointSelector => {
                ProjectionProblem::ReentrySelector
            }
            ProgramSourceError::ProjectionRouteUnprojectable => {
                ProjectionProblem::UnprojectableRoute
            }
        };
        Self {
            problem,
            role: None,
            scope: None,
            controller: None,
            arms: None,
            first: None,
            second: None,
        }
    }
    /// A repair direction, not permission to bypass the rejected obligation.
    pub const fn help(&self) -> &'static str {
        match self.problem {
            ProjectionProblem::RouteControllerMismatch => {
                "give both route arms the same first-visible controller"
            }
            ProjectionProblem::ReceiveLaneCausality => {
                "add a causal handoff from the earlier receiver to the later sender; on reentry, close that handoff inside the roll"
            }
            ProjectionProblem::ParallelSelector => "make parallel endpoint selectors distinct",
            ProjectionProblem::ReentrySelector => {
                "make rolled continuation and reentry endpoint selectors distinct"
            }
            ProjectionProblem::MissingBranchKnowledge => {
                "notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"
            }
            ProjectionProblem::UnprojectableRoute => {
                "inspect route selectors and passive-child scope structure"
            }
        }
    }
}
impl core::fmt::Display for ProjectionDiagnostic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.problem)?;
        if let Some(role) = self.role {
            write!(f, " role={role}")?;
        }
        if let Some(scope) = self.scope {
            write!(f, " scope={scope}")?;
        }
        if let Some(controller) = self.controller {
            write!(f, " controller={controller}")?;
        }
        if let Some(arms) = self.arms {
            write!(
                f,
                " arms={}..{}|{}..{}",
                arms[0].start, arms[0].end, arms[1].start, arms[1].end
            )?;
        }
        for event in [self.first, self.second].into_iter().flatten() {
            write!(
                f,
                " event#{}({}->{} label={} lane={})",
                event.index, event.from, event.to, event.label, event.lane
            )?;
        }
        write!(f, "; {}", self.help())
    }
}

/// Inspect projection obligations without creating a role program or endpoint.
///
/// `None` means the projection acceptance checks found no error. It does not
/// validate payload algorithms, physical I/O, or application progress. Malformed
/// source-domain invariants can still panic during lowering. The function uses
/// the same acceptance gate as projection, with bounded, allocation-free data.
#[allow(private_bounds)]
pub fn diagnose<Steps: ProgramShape>(program: &Program<Steps>) -> Option<ProjectionDiagnostic> {
    let _ = program;
    const {
        let rows = Steps::SOURCE_ROW_COUNT;
        if rows <= 8 {
            diagnostic_for::<Steps, 8>()
        } else if rows <= 32 {
            diagnostic_for::<Steps, 32>()
        } else if rows <= 128 {
            diagnostic_for::<Steps, 128>()
        } else if rows <= 512 {
            diagnostic_for::<Steps, 512>()
        } else if rows <= 2048 {
            diagnostic_for::<Steps, 2048>()
        } else if rows <= 8192 {
            diagnostic_for::<Steps, 8192>()
        } else if rows <= 32768 {
            diagnostic_for::<Steps, 32768>()
        } else if rows <= 65535 {
            diagnostic_for::<Steps, 65535>()
        } else {
            panic!("choreography source exceeds compact descriptor domain")
        }
    }
}

const fn diagnostic_for<Steps: ProgramShape, const CAPACITY: usize>() -> Option<ProjectionDiagnostic>
{
    ProgramProjection::<Steps, CAPACITY>::DIAGNOSTIC
}
