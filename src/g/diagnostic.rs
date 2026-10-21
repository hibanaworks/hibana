//! Read-only source diagnostics. These values are not projection evidence.
use super::{ProgramProjection, ProgramShape, ProgramSourceError};
use crate::global::program::Projectable;

/// The obligation that prevented projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionProblem {
    /// Ownership analysis exceeded the compact source domain.
    OwnershipCapacity,
    /// A self-role message carries a payload other than unit.
    NonUnitSelfMessage,
    /// A transferred Endpoint is also its own sender or receiver.
    EndpointIsItsOwnCarrier,
    /// The same Endpoint would require different initial holders.
    ConflictingInitialOwners,
    /// No unique initial holder can be derived for an Endpoint.
    UnrootedInitialOwner,
    /// One parallel fragment transfers an Endpoint used by another.
    ParallelOwnershipConflict,
    /// Repetition does not restore its incoming Endpoint ownership.
    RollChangesOwner,
    /// Both arms must have the same first-visible controller.
    RouteControllerMismatch,
    /// A shared receive lane changes sender without a causal handoff.
    ReceiveLaneCausality,
    /// Parallel endpoint selectors overlap.
    ParallelSelector,
    /// Rolled reentry selectors overlap.
    ReentrySelector,
    /// A reentry sender cannot learn that the entry resource has left.
    ReentryResourceKnowledge,
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
    pub(crate) const fn from_ownership_error(error: super::source::ownership::Error) -> Self {
        use super::source::ownership::ErrorKind;
        Self {
            problem: match error.kind {
                ErrorKind::Capacity => ProjectionProblem::OwnershipCapacity,
                ErrorKind::NonUnitSelfMessage => ProjectionProblem::NonUnitSelfMessage,
                ErrorKind::EndpointIsItsOwnCarrier => ProjectionProblem::EndpointIsItsOwnCarrier,
                ErrorKind::ConflictingInitialOwners => ProjectionProblem::ConflictingInitialOwners,
                ErrorKind::UnrootedInitialOwner => ProjectionProblem::UnrootedInitialOwner,
                ErrorKind::ParallelOwnershipConflict => {
                    ProjectionProblem::ParallelOwnershipConflict
                }
                ErrorKind::RollChangesOwner => ProjectionProblem::RollChangesOwner,
            },
            role: match error.endpoint {
                Some(endpoint) => Some(endpoint.role),
                None => None,
            },
            scope: None,
            controller: None,
            arms: None,
            first: None,
            second: None,
        }
    }
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
            ProgramSourceError::ReentryResourceKnowledge => {
                ProjectionProblem::ReentryResourceKnowledge
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
            ProjectionProblem::OwnershipCapacity => {
                "reduce the source to fit the compact descriptor domain"
            }
            ProjectionProblem::NonUnitSelfMessage => "use unit for a self-role action",
            ProjectionProblem::EndpointIsItsOwnCarrier => {
                "transfer the Endpoint through distinct control roles"
            }
            ProjectionProblem::ConflictingInitialOwners
            | ProjectionProblem::UnrootedInitialOwner => {
                "give each transferred Endpoint one initial holder"
            }
            ProjectionProblem::ParallelOwnershipConflict => {
                "keep the transfer and dependent resource operations in sequence"
            }
            ProjectionProblem::RollChangesOwner => {
                "return each transferred Endpoint to its incoming holder inside the roll"
            }
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
            ProjectionProblem::ReentryResourceKnowledge => {
                "let the resource's entry owner authorize each visit inside the roll"
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
/// source-domain invariants can still panic during construction or lowering.
/// Endpoint ownership errors are returned as diagnostic values. The function
/// uses projection's static obligations, with bounded, allocation-free data.
pub fn diagnose<P: Projectable>(program: &P) -> Option<ProjectionDiagnostic> {
    let _ = program;
    const {
        let rows = P::Steps::SOURCE_ROW_COUNT;
        if rows <= 8 {
            diagnostic_for::<P::Steps, 8>()
        } else if rows <= 32 {
            diagnostic_for::<P::Steps, 32>()
        } else if rows <= 128 {
            diagnostic_for::<P::Steps, 128>()
        } else if rows <= 512 {
            diagnostic_for::<P::Steps, 512>()
        } else if rows <= 2048 {
            diagnostic_for::<P::Steps, 2048>()
        } else if rows <= 8192 {
            diagnostic_for::<P::Steps, 8192>()
        } else if rows <= 32768 {
            diagnostic_for::<P::Steps, 32768>()
        } else if rows <= 65535 {
            diagnostic_for::<P::Steps, 65535>()
        } else {
            panic!("choreography source exceeds compact descriptor domain")
        }
    }
}

const fn diagnostic_for<Steps: ProgramShape, const CAPACITY: usize>() -> Option<ProjectionDiagnostic>
{
    ProgramProjection::<Steps, CAPACITY>::DIAGNOSTIC
}
