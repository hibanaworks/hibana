import Hibana.EndToEndRefinement
import Hibana.ProductionKernelArtifact
import Hibana.ProtocolCapability

namespace Hibana

/-- Production composition boundary. The compact runtime remains unchanged;
this host-only record connects exact descriptor/codecs, the complete static
deployment family, the finite prepare/commit relation, and the carrier profile.
Kani and Miri provide external evidence for the Rust owners named by
`kernelEvidence`; Lean records that evidence through the explicit
`OwnerVerified` premise rather than restating it as source verification. -/
structure AssumptionIndexedProductionRefinement
    (OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop)
    (profile : CarrierProfile)
    (Fairness : Prop)
    (primary : VerifiedProtocolMember)
    (family : List VerifiedProtocolMember)
    (deployment : DeploymentEvidence)
    (kernelArtifact : ProductionKernelArtifact)
    (kernel : PreparedKernelSemantics)
    (staticCertificate : StaticDeploymentCertificate) : Prop where
  primaryInFamily : primary ∈ family
  familyCapabilities : VerifiedProtocolFamilyCapabilityCoverage family
  staticDeployment : staticCertificate.Refines family
  codecRegistryExact : deployment.codecs = primary.codecs
  kernelEvidence : CrossToolProductionRefinement OwnerVerified kernelArtifact kernel
    primary.session primary.roleCount primary.choreo
  endToEnd : AssumptionIndexedEndToEndRefinement profile Fairness
    primary.certificate primary.session primary.roleCount primary.choreo
    deployment kernel

/-- General cross-tool composition theorem for an accepted production artifact
under explicit kernel-owner and carrier premises. The checked artifact rejects
omitted role families and byte-different descriptors; Rust miscompilation is
excluded only when the external refinement premises are actually established. -/
theorem assumption_indexed_static_cross_tool_production_refinement
    {OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop}
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {kernel : PreparedKernelSemantics}
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
    (kernelRefinement : PreparedKernelRefinement kernel primary.session
      primary.roleCount primary.choreo)
    (ownerEvidence : ProductionOwnerEvidence OwnerVerified kernelArtifact kernel)
    (carrierGuarantees :
      profile.Holds deployment.carrier deployment.abstraction Fairness) :
    AssumptionIndexedProductionRefinement OwnerVerified profile Fairness primary family
      deployment kernelArtifact kernel staticCertificate := by
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
      accepted_production_kernel_artifact_combines_with_external_kernel_refinement
        kernelAccepted kernelRefinement ownerEvidence
    endToEnd :=
      assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement
        primary.accepted imagesAccepted codecCoverage
        kernelRefinement
        carrierGuarantees
  }

theorem production_refinement_static_certificate_covers_primary
    {OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop}
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {kernel : PreparedKernelSemantics}
    {staticCertificate : StaticDeploymentCertificate}
    (verified : AssumptionIndexedProductionRefinement OwnerVerified profile Fairness primary
      family deployment kernelArtifact kernel staticCertificate) :
    Exists fun entry =>
      entry ∈ staticCertificate.entries /\
      RoleImagesExact primary.certificate entry.roleImages :=
  static_deployment_certificate_covers_family_member
    verified.staticDeployment verified.primaryInFamily

theorem production_refinement_prepared_commit_is_exact
    {OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop}
    {profile : CarrierProfile}
    {Fairness : Prop}
    {primary : VerifiedProtocolMember}
    {family : List VerifiedProtocolMember}
    {deployment : DeploymentEvidence}
    {kernelArtifact : ProductionKernelArtifact}
    {kernel : PreparedKernelSemantics}
    {staticCertificate : StaticDeploymentCertificate}
    (verified : AssumptionIndexedProductionRefinement OwnerVerified profile Fairness primary
      family deployment kernelArtifact kernel staticCertificate)
    {state : kernel.State}
    {input : kernel.Input}
    {prepared : kernel.Prepared}
    (preparedExact : kernel.prepare state input = some prepared) :
    let transition : RuntimeTransitionCertificate := {
      before := kernel.abstractState state
      effect := kernel.effect prepared
      after := kernel.abstractState
        (kernel.commit state prepared)
    }
    transition.Refines primary.session primary.roleCount primary.choreo :=
  end_to_end_prepared_commit_refines_exact_lean_effect
    verified.endToEnd preparedExact

end Hibana
