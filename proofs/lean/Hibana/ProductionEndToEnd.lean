import Hibana.EndToEndRefinement
import Hibana.ProductionKernelArtifact
import Hibana.ProtocolCapability

namespace Hibana

/-- Production composition boundary. The compact runtime remains unchanged;
this host-only record connects exact descriptor/codecs, the complete static
deployment family, the finite prepare/commit relation, and the carrier profile.
Kani and Miri discharge the Rust owners named by `kernelEvidence`, so their
results remain an explicit cross-tool TCB rather than being restated as Lean
source verification. -/
structure AssumptionIndexedProductionRefinement
    (profile : CarrierProfile)
    (Fairness : Prop)
    (primary : VerifiedProtocolMember)
    (family : List VerifiedProtocolMember)
    (deployment : DeploymentEvidence)
    (kernelArtifact : ProductionKernelArtifact)
    (staticCertificate : StaticDeploymentCertificate) : Prop where
  primaryInFamily : primary ∈ family
  familyCapabilities : VerifiedProtocolFamilyCapabilityCoverage family
  staticDeployment : staticCertificate.Refines family
  codecRegistryExact : deployment.codecs = primary.codecs
  kernelEvidence : CrossToolProductionRefinement kernelArtifact
    primary.session primary.roleCount primary.choreo
  endToEnd : AssumptionIndexedEndToEndRefinement profile Fairness
    primary.certificate primary.session primary.roleCount primary.choreo
    deployment kernelArtifact.kernel

/-- General cross-tool composition theorem for any accepted production
artifact. Miscompilation, omitted role families, byte-different descriptors,
or weaker-than-claimed carrier profiles cannot construct this record. -/
theorem assumption_indexed_static_cross_tool_production_refinement
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {staticCertificate : StaticDeploymentCertificate}
    (primaryInFamily : primary ∈ family)
    (familyCapabilitiesAccepted :
      checkVerifiedProtocolFamilyCapabilities family = true)
    (staticDeploymentAccepted : staticCertificate.check family = true)
    (imagesAccepted :
      checkRoleImages primary.certificate deployment.roleImages = true)
    (codecRegistryExact : deployment.codecs = primary.codecs)
    (kernelAccepted : kernelArtifact.check primary.session primary.roleCount
      primary.choreo = true)
    (carrierGuarantees :
      profile.Holds deployment.carrier deployment.abstraction Fairness) :
    AssumptionIndexedProductionRefinement profile Fairness primary family
      deployment kernelArtifact staticCertificate := by
  have codecCoverage : VerifiedCodecCoverage primary.choreo deployment.codecs := by
    rw [codecRegistryExact]
    exact primary.codecCoverage
  exact {
    primaryInFamily
    familyCapabilities :=
      verified_protocol_family_capability_checker_sound
        familyCapabilitiesAccepted
    staticDeployment :=
      static_deployment_certificate_sound staticDeploymentAccepted
    codecRegistryExact
    kernelEvidence :=
      accepted_production_kernel_artifact_establishes_cross_tool_refinement
        kernelAccepted
    endToEnd :=
      assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement
        primary.accepted imagesAccepted codecCoverage
        (accepted_production_kernel_artifact_refines_rust_kernel kernelAccepted)
        carrierGuarantees
  }

theorem production_refinement_static_certificate_covers_primary
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {staticCertificate : StaticDeploymentCertificate}
    (verified : AssumptionIndexedProductionRefinement profile Fairness primary
      family deployment kernelArtifact staticCertificate) :
    Exists fun entry =>
      entry ∈ staticCertificate.entries /\
      RoleImagesExact primary.certificate entry.roleImages :=
  static_deployment_certificate_covers_family_member
    verified.staticDeployment verified.primaryInFamily

theorem production_refinement_prepared_commit_is_exact
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {staticCertificate : StaticDeploymentCertificate}
    (verified : AssumptionIndexedProductionRefinement profile Fairness primary
      family deployment kernelArtifact staticCertificate)
    {state : kernelArtifact.kernel.State}
    {input : kernelArtifact.kernel.Input}
    {prepared : kernelArtifact.kernel.Prepared}
    (preparedExact : kernelArtifact.kernel.prepare state input = some prepared) :
    let transition : RuntimeTransitionCertificate := {
      before := kernelArtifact.kernel.abstractState state
      effect := kernelArtifact.kernel.effect prepared
      after := kernelArtifact.kernel.abstractState
        (kernelArtifact.kernel.commit state prepared)
    }
    transition.Refines primary.session primary.roleCount primary.choreo :=
  end_to_end_prepared_commit_refines_exact_lean_effect
    verified.endToEnd preparedExact

end Hibana
