import Hibana.Allocation
import Hibana.Authority
import Hibana.Commit
import Hibana.Generation
import Hibana.GlobalSemantics
import Hibana.GlobalMessageTransitions
import Hibana.GlobalFidelity
import Hibana.DescriptorRefinement
import Hibana.Cancellation
import Hibana.TransportContract
import Hibana.AsyncCancellationTermination
import Hibana.GlobalProgress
import Hibana.GlobalCoherence
import Hibana.DistributedSemantics
import Hibana.RollFreshness
import Hibana.SessionComposition
import Hibana.SessionLifecycle
import Hibana.StaticProjectability
import Hibana.IterationErasure
import Hibana.AffineRoutePublication
import Hibana.RuntimeRefinement
import Hibana.ProtocolArtifact
import Hibana.StaticProjectabilityExamples
import Hibana.Layout
import Hibana.Progress
import Hibana.Refinement
import Hibana.RuntimeMonitor

namespace Hibana

theorem initial_state_has_no_selected_arm (conflict : Nat) :
    CommitState.initial.selected conflict = none := rfl

theorem initial_state_has_no_completed_event (event : Nat) :
    CommitState.initial.done event = false := rfl

private def resolverProofChoreo : Choreo :=
  .route (.dynamic 7) (.send 0 1 1 6) (.send 0 1 2 7)

private def resolverProofFrontier : List MessageKey :=
  [{ label := 1, schema := 6 }, { label := 2, schema := 7 }]

theorem unresolved_dynamic_commit_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .commit { label := 1, schema := 6 } }
    ] = false := by
  decide

theorem wrong_resolver_id_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .resolve 0 8 .left }
    ] = false := by
  decide

theorem resolver_reuse_without_reset_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .resolve 0 7 .left },
      { enabled := [{ label := 1, schema := 6 }], action := .resolve 0 7 .right }
    ] = false := by
  decide

theorem resolver_reject_is_terminal :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .reject 0 7 },
      { enabled := resolverProofFrontier, action := .stop }
    ] = false := by
  decide

end Hibana
