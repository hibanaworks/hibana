import Hibana.Allocation
import Hibana.Authority
import Hibana.Commit
import Hibana.Generation
import Hibana.GlobalSemantics
import Hibana.GlobalAllocationCorrectness
import Hibana.GlobalMessageTransitions
import Hibana.GlobalFidelity
import Hibana.DescriptorRefinement
import Hibana.Cancellation
import Hibana.TransportContract
import Hibana.AsyncCancellationTermination
import Hibana.GlobalProgress
import Hibana.GlobalCoherence
import Hibana.DistributedSemantics
import Hibana.DistributedProgress
import Hibana.RollFreshness
import Hibana.DistributedRollRefinement
import Hibana.ElasticIterationQueue
import Hibana.ElasticRouteHistory
import Hibana.ElasticAdmissionHistory
import Hibana.ElasticErasure
import Hibana.SessionComposition
import Hibana.SessionLifecycle
import Hibana.StaticProjectability
import Hibana.IterationErasure
import Hibana.InBandChoiceKnowledge
import Hibana.RuntimeRefinement
import Hibana.PublicOperationKernel
import Hibana.PreparedKernelRefinement
import Hibana.ProductionKernelArtifact
import Hibana.ProtocolArtifact
import Hibana.ProtocolCapability
import Hibana.CarrierProfile
import Hibana.CodecEvidence
import Hibana.Deployment
import Hibana.EndToEndRefinement
import Hibana.ProductionEndToEnd
import Hibana.Layout
import Hibana.Progress
import Hibana.Refinement
import Hibana.OperationAdmission

namespace Hibana

theorem initial_state_has_no_selected_arm (conflict : Nat) :
    CommitState.initial.selected conflict = none := rfl

theorem initial_state_has_no_completed_event (event : Nat) :
    CommitState.initial.done event = false := rfl

private def resolverProofChoreo : Choreo :=
  .route (.dynamic 7) (.send 0 1 1 6) (.send 0 1 2 7)

private def resolverProofLeft : OperationRequest :=
  { eventId := 0, action := .send 1 1 6 }

private def resolverProofRight : OperationRequest :=
  { eventId := 1, action := .send 1 2 7 }

private def resolverProofFrontier : List OperationRequest :=
  [resolverProofLeft, resolverProofRight]

theorem unresolved_dynamic_commit_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .commit resolverProofLeft }
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
      { enabled := [resolverProofLeft], action := .resolve 0 7 .right }
    ] = false := by
  decide

theorem resolver_reject_is_terminal :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := resolverProofFrontier, action := .reject 0 7 },
      { enabled := resolverProofFrontier, action := .stop }
    ] = false := by
  decide

end Hibana
