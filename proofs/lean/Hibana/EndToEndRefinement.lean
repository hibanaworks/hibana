import Hibana.Deployment
import Hibana.ElasticErasure
import Hibana.RustKernelRefinement

namespace Hibana

/-- The complete proof/deployment artifact. Every field is host-only evidence;
all profiles execute the same compact endpoint runtime. In particular, this
record adds no endpoint continuation type, iteration counter, carrier flag, or
wire header field. -/
structure AssumptionIndexedEndToEndRefinement
    (profile : CarrierProfile)
    (Fairness : Prop)
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo)
    (deployment : DeploymentEvidence)
    (kernel : PreparedKernelSemantics) : Prop where
  protocolExecutionGuarantees :
    ProtocolExecutionGuarantees certificate session roleCount choreo
  translationValidated :
    certificate.AllRolesRefine session roleCount choreo
  deploymentAgreement :
    RoleImagesExact certificate deployment.roleImages
  canonicalCodecCoverage :
    VerifiedCodecCoverage choreo deployment.codecs
  rustKernelRefinement :
    RustKernelRefinement kernel session roleCount choreo
  carrierGuarantees :
    profile.Holds deployment.carrier deployment.abstraction Fairness
  elasticTraceRefinement : ElasticErasureRefinement

/-- Main composition theorem. Descriptor translation is independently checked
from exact bytes; Rust/Kani/Miri refinement and deployment/carrier assumptions
remain explicit premises. Therefore the theorem neither hides the cross-tool
TCB nor grants stronger delivery claims to a weaker profile. -/
theorem assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (protocolAccepted : certificate.check session roleCount choreo = true)
    (imagesAccepted :
      checkRoleImages certificate deployment.roleImages = true)
    (codecCoverage : VerifiedCodecCoverage choreo deployment.codecs)
    (kernelRefinement : RustKernelRefinement kernel session roleCount choreo)
    (carrierGuarantees :
      profile.Holds deployment.carrier deployment.abstraction Fairness) :
    AssumptionIndexedEndToEndRefinement profile Fairness certificate
      session roleCount choreo deployment kernel := by
  exact {
    protocolExecutionGuarantees :=
      verified_protocol_establishes_execution_guarantees
        protocolAccepted
    translationValidated :=
      verified_protocol_certificate_establishes_all_role_refinement
        protocolAccepted
    deploymentAgreement := role_images_checker_sound imagesAccepted
    canonicalCodecCoverage := codecCoverage
    rustKernelRefinement := kernelRefinement
    carrierGuarantees := carrierGuarantees
    elasticTraceRefinement := elastic_erasure_refinement_holds
  }

theorem end_to_end_prepared_commit_refines_exact_lean_effect
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement profile Fairness certificate
      session roleCount choreo deployment kernel)
    {state : kernel.State}
    {input : kernel.Input}
    {prepared : kernel.Prepared}
    (preparedExact : kernel.prepare state input = some prepared) :
    let transition : RuntimeTransitionCertificate := {
      before := kernel.abstractState state
      effect := kernel.effect prepared
      after := kernel.abstractState (kernel.commit state prepared)
    }
    transition.Refines session roleCount choreo :=
  prepared_kernel_commit_refines_exact_lean_effect
    verified.rustKernelRefinement preparedExact

theorem end_to_end_closing_profile_has_affine_carrier_refinement
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement .closing Fairness certificate
      session roleCount choreo deployment kernel) :
    AffineCarrierRefinement deployment.carrier deployment.abstraction :=
  closing_profile_establishes_affine_carrier_refinement
    verified.carrierGuarantees

theorem end_to_end_fair_profile_exposes_fairness_premise
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement .fair Fairness certificate
      session roleCount choreo deployment kernel) :
    Fairness :=
  fair_profile_exposes_fairness_premise verified.carrierGuarantees

/-- Instantiating the fair profile with the model's actual run predicate makes
the liveness consequence available at the end-to-end boundary. A deployment
must still establish this premise for its executor and carrier. -/
theorem end_to_end_fair_run_schedules_recurrently_enabled_operation
    {initial : GlobalConfig}
    {run : GlobalRun initial}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement .fair
      (GlobalFairnessAssumptions run) certificate session roleCount choreo
      deployment kernel)
    {operation : GlobalOperation}
    (recurrentlyEnabled :
      forall time,
        Exists fun later =>
          Exists fun next =>
            time <= later /\
              (run.state later).step? operation = some next) :
    forall time,
      Exists fun later =>
        time <= later /\ run.operation later = some operation :=
  fairness_schedules_recurrently_enabled_operation
    (end_to_end_fair_profile_exposes_fairness_premise verified)
    recurrentlyEnabled

/-- A successful production descriptor send extends the proof history while
its runtime-visible trace contains only the static global event identity. The
elastic iteration ordinal is absent from the conclusion's erased trace. -/
theorem end_to_end_descriptor_send_erases_elastic_iteration
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement profile Fairness certificate
      session roleCount choreo deployment kernel)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (descriptorAt : certificate.descriptors[role]? = some descriptor)
    {cursor nextCursor : CommitState}
    {event : GlobalEvent}
    {history : ElasticAdmissionHistory}
    {globalId peer label schema : Nat}
    (eventAt : descriptor.choreo.globalEvents[globalId]? = some event)
    (projected : event.action? descriptor.image.role =
      some (.send peer label schema))
    (channelMatches : history.MatchesEventChannel event)
    (historyWellFormed : history.WellFormed)
    (stepped : descriptorCursorStep
      descriptor.image
      (projectGraph descriptor.image.role descriptor.choreo)
      cursor
      (descriptor.choreo.localEventId descriptor.image.role globalId) =
        .accepted nextCursor) :
    descriptor.image.eventAction?
        (descriptor.choreo.localEventId descriptor.image.role globalId) =
      some (.send peer label schema) /\
    (history.appendOccurrence globalId).WellFormed /\
    (history.appendOccurrence globalId).erasedTrace =
      history.erasedTrace ++ [globalId] := by
  have descriptorAccepted :=
    (verified.translationValidated.2.2 role descriptor descriptorAt).1
  have exactSend := accepted_descriptor_pipelined_send_extends_elastic_history
    descriptorAccepted eventAt projected channelMatches historyWellFormed stepped
  exact ⟨exactSend.1, exactSend.2.1,
    verified.elasticTraceRefinement.admissionAppend history globalId⟩

/-- The complete production carrier label trace is the descriptor-label image
of the erased Lean occurrence trace. Iteration ordinals are absent on both
sides, while exact labels and affine delivery remain related. -/
theorem end_to_end_transport_trace_is_exact_elastic_erasure
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    {kernel : PreparedKernelSemantics}
    (verified : AssumptionIndexedEndToEndRefinement profile Fairness certificate
      session roleCount choreo deployment kernel)
    {frameLabelOf : Nat -> Fin 256}
    {history : ElasticAdmissionHistory}
    {transport : TransportState}
    (erases : history.ErasesToTransport frameLabelOf transport) :
    history.erasedTrace.map frameLabelOf =
      transport.frames.map TransportFrame.frameLabel :=
  verified.elasticTraceRefinement.transportTraceExact erases

end Hibana
