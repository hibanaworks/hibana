use std::string::String;

pub(super) fn source() -> String {
    String::from(
        r#"def generatedProductionSession : Nat := 0

def generatedProductionInitial : Hibana.CompactGlobalState :=
  Hibana.CompactGlobalState.initial generatedProductionSession 4 generatedChoreo

def generatedProductionAfterProtocol : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 4 generatedChoreo
    generatedProductionInitial (.protocol (.send 0))).get (by native_decide)

def generatedProductionAfterReceive : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 4 generatedChoreo
    generatedProductionAfterProtocol (.protocol (.recv 0))).get (by native_decide)

def generatedProductionLocalChoreo : Hibana.Choreo :=
  .send 0 0 201 0

def generatedProductionLocalInitial : Hibana.CompactGlobalState :=
  Hibana.CompactGlobalState.initial generatedProductionSession 1
    generatedProductionLocalChoreo

def generatedProductionAfterLocal : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 1
    generatedProductionLocalChoreo generatedProductionLocalInitial
    (.protocol (.localAction 0))).get (by native_decide)

def generatedProductionResolvedInitial : Hibana.CompactGlobalState :=
  Hibana.CompactGlobalState.initial generatedProductionSession 2
    generatedResolvedChoreo

def generatedProductionAfterResolve : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedResolvedChoreo
    generatedProductionResolvedInitial
    (.protocol (.resolve 0 0 901 .left))).get (by native_decide)

def generatedProductionAfterResolverReject : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedResolvedChoreo
    generatedProductionResolvedInitial
    (.protocol (.rejectResolver 0 0 901))).get (by native_decide)

def generatedProductionRolledInitial : Hibana.CompactGlobalState :=
  Hibana.CompactGlobalState.initial generatedProductionSession 2
    generatedRolledChoreo

def generatedProductionRolledAfterSend : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedRolledChoreo
    generatedProductionRolledInitial (.protocol (.send 0))).get (by native_decide)

def generatedProductionRolledAfterReceive : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedRolledChoreo
    generatedProductionRolledAfterSend (.protocol (.recv 0))).get (by native_decide)

def generatedProductionRolledAfterTailSend : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedRolledChoreo
    generatedProductionRolledAfterReceive (.protocol (.send 2))).get (by native_decide)

def generatedProductionRolledBeforeRoll : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedRolledChoreo
    generatedProductionRolledAfterTailSend (.protocol (.recv 2))).get (by native_decide)

def generatedProductionRolledAfterRoll : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 2 generatedRolledChoreo
    generatedProductionRolledBeforeRoll (.protocol (.roll 0))).get (by native_decide)

def generatedProductionAfterFault : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 4 generatedChoreo
    generatedProductionInitial (.fault 0 .transportOffline)).get (by native_decide)

def generatedProductionAfterAmbiguousReceive : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 4 generatedChoreo
    generatedProductionInitial (.ambiguousReceive 0)).get (by native_decide)

def generatedProductionAfterCancellationObservation : Hibana.CompactGlobalState :=
  (Hibana.applyRuntimeEffect? generatedProductionSession 4 generatedChoreo
    generatedProductionAfterFault (.observeCancellation 1)).get (by native_decide)

def generatedProductionKernelArtifact : Hibana.ProductionKernelArtifact := {
  transitions := [
    { before := generatedProductionInitial
      effect := .protocol (.send 0)
      after := generatedProductionAfterProtocol },
    { before := generatedProductionInitial
      effect := .preview
      after := generatedProductionInitial },
    { before := generatedProductionInitial
      effect := .dropPending
      after := generatedProductionInitial },
    { before := generatedProductionInitial
      effect := .requeue
      after := generatedProductionInitial },
    { before := generatedProductionInitial
      effect := .fault 0 .transportOffline
      after := generatedProductionAfterFault },
    { before := generatedProductionInitial
      effect := .ambiguousReceive 0
      after := generatedProductionAfterAmbiguousReceive },
    { before := generatedProductionAfterFault
      effect := .observeCancellation 1
      after := generatedProductionAfterCancellationObservation }
  ]
  protocolCases := [
    { session := generatedProductionSession
      roleCount := 4
      choreo := generatedChoreo
      transition := {
        before := generatedProductionInitial
        effect := .protocol (.send 0)
        after := generatedProductionAfterProtocol } },
    { session := generatedProductionSession
      roleCount := 4
      choreo := generatedChoreo
      transition := {
        before := generatedProductionAfterProtocol
        effect := .protocol (.recv 0)
        after := generatedProductionAfterReceive } },
    { session := generatedProductionSession
      roleCount := 1
      choreo := generatedProductionLocalChoreo
      transition := {
        before := generatedProductionLocalInitial
        effect := .protocol (.localAction 0)
        after := generatedProductionAfterLocal } },
    { session := generatedProductionSession
      roleCount := 2
      choreo := generatedResolvedChoreo
      transition := {
        before := generatedProductionResolvedInitial
        effect := .protocol (.resolve 0 0 901 .left)
        after := generatedProductionAfterResolve } },
    { session := generatedProductionSession
      roleCount := 2
      choreo := generatedResolvedChoreo
      transition := {
        before := generatedProductionResolvedInitial
        effect := .protocol (.rejectResolver 0 0 901)
        after := generatedProductionAfterResolverReject } },
    { session := generatedProductionSession
      roleCount := 2
      choreo := generatedRolledChoreo
      transition := {
        before := generatedProductionRolledBeforeRoll
        effect := .protocol (.roll 0)
        after := generatedProductionRolledAfterRoll } }
  ]
  owners := [.endpoint, .cursor, .slab, .waiter, .resolver,
    .receiveReceipt, .transport, .callbackReentry]
}

example : generatedProductionKernelArtifact.check
    generatedProductionSession 4 generatedChoreo = true := by
  native_decide

example : Hibana.PreparedKernelRefinement generatedProductionKernelArtifact.kernel
    generatedProductionSession 4 generatedChoreo :=
  Hibana.accepted_production_kernel_artifact_refines_artifact_kernel (by native_decide)

theorem generated_production_protocol_cases_refine_prepared_kernels :
    forall protocolCase,
      protocolCase ∈ generatedProductionKernelArtifact.protocolCases ->
        Hibana.PreparedKernelRefinement protocolCase.kernel protocolCase.session
          protocolCase.roleCount protocolCase.choreo :=
  Hibana.accepted_production_kernel_artifact_refines_protocol_case_kernels
    (artifact := generatedProductionKernelArtifact)
    (session := generatedProductionSession)
    (roleCount := 4)
    (choreo := generatedChoreo)
    (by native_decide)

def generatedProductionCodecEntries : List (Nat × Nat) :=
  [(0, 0), (6, 4), (7, 4)]

def generatedProductionCodecs : List Hibana.VerifiedCodec :=
  Hibana.fixedWidthCodecRegistry generatedProductionCodecEntries

theorem generated_production_codec_coverage_for
    (choreo : Hibana.Choreo)
    (supported :
      forall candidate, candidate ∈ choreo.globalEvents ->
        candidate.schema = 0 \/ candidate.schema = 6 \/ candidate.schema = 7) :
    Hibana.VerifiedCodecCoverage choreo generatedProductionCodecs := by
  constructor
  · exact Hibana.fixed_width_codec_registry_agrees
      (Hibana.fixed_width_schema_registry_checker_sound (by native_decide))
  · intro event member
    rcases supported event member with unitSchema | u32Schema | i32Schema
    · refine ⟨Hibana.fixedWidthVerifiedCodec 0 0, ?_, ?_⟩
      · simp [generatedProductionCodecs, generatedProductionCodecEntries,
          Hibana.fixedWidthCodecRegistry]
      · rw [unitSchema]
        rfl
    · refine ⟨Hibana.fixedWidthVerifiedCodec 6 4, ?_, ?_⟩
      · simp [generatedProductionCodecs, generatedProductionCodecEntries,
          Hibana.fixedWidthCodecRegistry]
      · rw [u32Schema]
        rfl
    · refine ⟨Hibana.fixedWidthVerifiedCodec 7 4, ?_, ?_⟩
      · simp [generatedProductionCodecs, generatedProductionCodecEntries,
          Hibana.fixedWidthCodecRegistry]
      · rw [i32Schema]
        rfl

theorem generated_production_codec_coverage :
    Hibana.VerifiedCodecCoverage generatedChoreo generatedProductionCodecs :=
  generated_production_codec_coverage_for generatedChoreo (by native_decide)

def generatedVerifiedProtocolMember
    (roleCount : Nat)
    (choreo : Hibana.Choreo)
    (certificate : Hibana.VerifiedProtocolCertificate)
    (accepted : certificate.check 0 roleCount choreo = true)
    (supported :
      forall candidate, candidate ∈ choreo.globalEvents ->
        candidate.schema = 0 \/ candidate.schema = 6 \/ candidate.schema = 7) :
    Hibana.VerifiedProtocolMember := {
  session := 0
  roleCount
  choreo
  certificate
  accepted
  codecs := generatedProductionCodecs
  codecCoverage := generated_production_codec_coverage_for choreo supported
}

def generatedVerifiedProtocolPrimary : Hibana.VerifiedProtocolMember :=
  generatedVerifiedProtocolMember 4 generatedChoreo generatedVerifiedProtocol
    (by native_decide) (by native_decide)

def generatedVerifiedProtocolFamily : List Hibana.VerifiedProtocolMember := [
  generatedVerifiedProtocolPrimary,
  generatedVerifiedProtocolMember 2 generatedRolledChoreo
    generatedRolledVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 2 generatedNestedRolledChoreo
    generatedNestedRolledVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 2 generatedResolvedChoreo
    generatedResolvedVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 2 generatedNestedResolvedChoreo
    generatedNestedResolvedVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 2 generatedRolledResolvedChoreo
    generatedRolledResolvedVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 2 generatedRejectingChoreo
    generatedRejectingVerifiedProtocol (by native_decide) (by native_decide),
  generatedVerifiedProtocolMember 3 generatedCyclicRollChoreo
    generatedCyclicRollVerifiedProtocol (by native_decide) (by native_decide)
]

theorem generated_verified_protocol_family_covers_core_capabilities :
    Hibana.VerifiedProtocolFamilyCapabilityCoverage
      generatedVerifiedProtocolFamily :=
  Hibana.verified_protocol_family_capability_checker_sound (by native_decide)

def generatedStaticDeploymentEntry
    (member : Hibana.VerifiedProtocolMember) : Hibana.StaticDeploymentEntry := {
  roleImages := member.certificate.descriptors.map
    Hibana.ExactDescriptorCertificate.image
}

def generatedPrimaryStaticDeploymentEntry : Hibana.StaticDeploymentEntry :=
  generatedStaticDeploymentEntry generatedVerifiedProtocolPrimary

def generatedStaticDeploymentCertificate : Hibana.StaticDeploymentCertificate := {
  entries := generatedVerifiedProtocolFamily.map generatedStaticDeploymentEntry
}

example : generatedStaticDeploymentCertificate.check
    generatedVerifiedProtocolFamily = true := by
  native_decide

theorem generated_static_deployment_certificate_refines_exact_family :
    generatedStaticDeploymentCertificate.Refines generatedVerifiedProtocolFamily :=
  Hibana.static_deployment_certificate_sound (by native_decide)

def generatedMissingStaticDeploymentCertificate :
    Hibana.StaticDeploymentCertificate := {
  entries := generatedStaticDeploymentCertificate.entries.drop 1
}

example : generatedMissingStaticDeploymentCertificate.check
    generatedVerifiedProtocolFamily = false := by
  native_decide

def generatedExtraStaticDeploymentCertificate :
    Hibana.StaticDeploymentCertificate := {
  entries := generatedStaticDeploymentCertificate.entries ++
    [generatedPrimaryStaticDeploymentEntry]
}

example : generatedExtraStaticDeploymentCertificate.check
    generatedVerifiedProtocolFamily = false := by
  native_decide

def generatedCorruptStaticDeploymentCertificate :
    Hibana.StaticDeploymentCertificate := {
  entries :=
    { roleImages := generatedPrimaryStaticDeploymentEntry.roleImages.drop 1 } ::
      generatedStaticDeploymentCertificate.entries.drop 1
}

example : generatedCorruptStaticDeploymentCertificate.check
    generatedVerifiedProtocolFamily = false := by
  native_decide

def generatedProductionDeployment : Hibana.DeploymentEvidence := {
  roleImages := generatedVerifiedProtocol.descriptors.map
    Hibana.ExactDescriptorCertificate.image
  codecs := generatedProductionCodecs
  carrier := Hibana.referenceCarrierSemantics
  abstraction := id
}

theorem generated_production_closing_carrier :
    Hibana.CarrierProfile.closing.Holds
      generatedProductionDeployment.carrier
      generatedProductionDeployment.abstraction True :=
  Hibana.affine_carrier_refinement_supports_closing_profile
    Hibana.reference_carrier_refines_affine_boundary

def generatedProductionRefinement
    (kernel : Hibana.PreparedKernelSemantics)
    (kernelRefinement : Hibana.PreparedKernelRefinement kernel
      generatedVerifiedProtocolPrimary.session
      generatedVerifiedProtocolPrimary.roleCount
      generatedVerifiedProtocolPrimary.choreo)
    (OwnerVerified : Hibana.ProductionKernelOwner ->
      Hibana.PreparedKernelSemantics -> Prop)
    (ownerEvidence : Hibana.ProductionOwnerEvidence OwnerVerified
      generatedProductionKernelArtifact kernel) :
    Hibana.AssumptionIndexedProductionRefinement OwnerVerified .closing True
      generatedVerifiedProtocolPrimary generatedVerifiedProtocolFamily
      generatedProductionDeployment generatedProductionKernelArtifact kernel
      generatedStaticDeploymentCertificate :=
  Hibana.assumption_indexed_static_cross_tool_production_refinement
    (by simp [generatedVerifiedProtocolFamily])
    (by native_decide)
    (by native_decide)
    (by native_decide)
    rfl
    (by native_decide)
    kernelRefinement
    ownerEvidence
    generated_production_closing_carrier

#eval IO.println "hibana Lean production artifact passed transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing"
"#,
    )
}
