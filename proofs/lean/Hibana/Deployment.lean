import Hibana.CarrierProfile
import Hibana.CodecEvidence
import Hibana.ProtocolArtifact
import Hibana.ProtocolCapability

namespace Hibana

/-- Exact role images named by a finite deployment artifact. Equality is
byte-for-byte because `RustDescriptorImage` contains resident metadata and both
packed blobs. The artifact may be supplied statically, by an authenticated
manifest, or by a separately verified bootstrap session; core transport
headers remain unchanged. -/
def RoleImagesExact
    (certificate : VerifiedProtocolCertificate)
    (roleImages : List RustDescriptorImage) : Prop :=
  roleImages = certificate.descriptors.map ExactDescriptorCertificate.image

instance (certificate : VerifiedProtocolCertificate)
    (roleImages : List RustDescriptorImage) :
    Decidable (RoleImagesExact certificate roleImages) := by
  unfold RoleImagesExact
  infer_instance

def checkRoleImages
    (certificate : VerifiedProtocolCertificate)
    (roleImages : List RustDescriptorImage) : Bool :=
  decide (RoleImagesExact certificate roleImages)

theorem role_images_checker_sound
    {certificate : VerifiedProtocolCertificate}
    {roleImages : List RustDescriptorImage}
    (accepted : checkRoleImages certificate roleImages = true) :
    RoleImagesExact certificate roleImages :=
  of_decide_eq_true accepted

theorem exact_role_images_have_role_count
    {certificate : VerifiedProtocolCertificate}
    {roleImages : List RustDescriptorImage}
    {roleCount : Nat}
    (descriptorCount : certificate.descriptors.length = roleCount)
    (exact : RoleImagesExact certificate roleImages) :
    roleImages.length = roleCount := by
  rw [exact, List.length_map, descriptorCount]

theorem exact_role_image
    {certificate : VerifiedProtocolCertificate}
    {roleImages : List RustDescriptorImage}
    (exact : RoleImagesExact certificate roleImages)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    roleImages[role]? = some descriptor.image := by
  rw [exact]
  simpa using congrArg (Option.map ExactDescriptorCertificate.image) present

/-- One exact role-image family emitted by the host deployment generator. The
corresponding protocol and codec evidence remains owned by the separately
checked `VerifiedProtocolMember` family. -/
structure StaticDeploymentEntry where
  roleImages : List RustDescriptorImage

def checkStaticDeploymentEntries :
    List VerifiedProtocolMember -> List StaticDeploymentEntry -> Bool
  | [], [] => true
  | member :: family, entry :: entries =>
      checkRoleImages member.certificate entry.roleImages &&
        checkStaticDeploymentEntries family entries
  | _, _ => false

/-- Complete static release artifact for a finite protocol family. The checker
rejects omission, reordering, extra protocols, or any byte-different role image.
The artifact is generated and checked on the host and is erased from runtime. -/
structure StaticDeploymentCertificate where
  entries : List StaticDeploymentEntry

def StaticDeploymentCertificate.check
    (certificate : StaticDeploymentCertificate)
    (family : List VerifiedProtocolMember) : Bool :=
  checkStaticDeploymentEntries family certificate.entries

/-- Exact positional relation between the sole protocol-family authority and
the role-image families selected for deployment. -/
inductive StaticDeploymentEntriesExact :
    List VerifiedProtocolMember -> List StaticDeploymentEntry -> Prop where
  | nil : StaticDeploymentEntriesExact [] []
  | cons :
      RoleImagesExact member.certificate entry.roleImages ->
      StaticDeploymentEntriesExact family entries ->
      StaticDeploymentEntriesExact (member :: family) (entry :: entries)

def StaticDeploymentCertificate.Refines
    (certificate : StaticDeploymentCertificate)
    (family : List VerifiedProtocolMember) : Prop :=
  StaticDeploymentEntriesExact family certificate.entries

theorem static_deployment_certificate_sound
    {certificate : StaticDeploymentCertificate}
    {family : List VerifiedProtocolMember}
    (accepted : certificate.check family = true) :
    certificate.Refines family := by
  rcases certificate with ⟨entries⟩
  change checkStaticDeploymentEntries family entries = true at accepted
  change StaticDeploymentEntriesExact family entries
  induction family generalizing entries with
  | nil =>
      cases entries with
      | nil => exact .nil
      | cons _ _ => simp [checkStaticDeploymentEntries] at accepted
  | cons member family inductionHypothesis =>
      cases entries with
      | nil => simp [checkStaticDeploymentEntries] at accepted
      | cons entry entries =>
          simp only [checkStaticDeploymentEntries, Bool.and_eq_true] at accepted
          exact .cons (role_images_checker_sound accepted.1)
            (inductionHypothesis entries accepted.2)

theorem static_deployment_certificate_covers_family_member
    {certificate : StaticDeploymentCertificate}
    {family : List VerifiedProtocolMember}
    (refines : certificate.Refines family)
    {member : VerifiedProtocolMember}
    (present : member ∈ family) :
    Exists fun entry =>
      entry ∈ certificate.entries /\
      RoleImagesExact member.certificate entry.roleImages := by
  rcases certificate with ⟨entries⟩
  change StaticDeploymentEntriesExact family entries at refines
  change Exists fun entry =>
    entry ∈ entries /\ RoleImagesExact member.certificate entry.roleImages
  induction refines with
  | nil => simp at present
  | cons imageExact tailRefines inductionHypothesis =>
      simp only [List.mem_cons] at present
      rcases present with identity | tailPresent
      · subst member
        exact ⟨_, List.mem_cons_self, imageExact⟩
      · obtain ⟨entry, entryPresent, exact⟩ :=
          inductionHypothesis tailPresent
        exact ⟨entry, List.mem_cons_of_mem _ entryPresent, exact⟩

/-- Host/proof-only deployment artifact. `codecs` may be empty when only schema
identity is checked; an end-to-end canonical-payload claim separately requires
`VerifiedCodecCoverage`. None of these fields is resident in an endpoint. -/
structure DeploymentEvidence where
  roleImages : List RustDescriptorImage
  codecs : List VerifiedCodec
  carrier : CarrierSemantics
  abstraction : carrier.State -> TransportBoundaryState

/-- One deployment contract indexed by the exact carrier assumptions it may
use. This replaces parallel weak/strong runtime types: every profile executes
the same message-erased endpoint kernel. -/
structure AssumptionIndexedDeploymentContract
    (profile : CarrierProfile)
    (Fairness : Prop)
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo)
    (deployment : DeploymentEvidence) : Prop where
  executionGuarantees :
    ProtocolExecutionGuarantees certificate session roleCount choreo
  roleImagesExact :
    RoleImagesExact certificate deployment.roleImages
  carrierProfile :
    profile.Holds deployment.carrier deployment.abstraction Fairness

theorem verified_protocol_and_deployment_evidence_establish_profile
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (protocolAccepted : certificate.check session roleCount choreo = true)
    (imagesAccepted :
      checkRoleImages certificate deployment.roleImages = true)
    (carrierProfile :
      profile.Holds deployment.carrier deployment.abstraction Fairness) :
    AssumptionIndexedDeploymentContract profile Fairness
      certificate session roleCount choreo deployment := by
  exact {
    executionGuarantees :=
      verified_protocol_establishes_execution_guarantees
        protocolAccepted
    roleImagesExact := role_images_checker_sound imagesAccepted
    carrierProfile
  }

theorem assumption_indexed_deployment_has_mediated_carrier
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract profile Fairness
      certificate session roleCount choreo deployment) :
    MediatedCarrierContract deployment.carrier := by
  have weaker : CarrierProfile.mediated.atMost profile := by
    cases profile <;>
      simp [CarrierProfile.atMost, CarrierProfile.rank]
  exact carrier_profile_supports_every_weaker_profile
    weaker runtime.carrierProfile

theorem closing_deployment_establishes_affine_carrier_refinement
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract .closing Fairness
      certificate session roleCount choreo deployment) :
    AffineCarrierRefinement deployment.carrier deployment.abstraction :=
  closing_profile_establishes_affine_carrier_refinement runtime.carrierProfile

theorem fair_deployment_exposes_fairness_premise
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract .fair Fairness
      certificate session roleCount choreo deployment) :
    Fairness :=
  fair_profile_exposes_fairness_premise runtime.carrierProfile

theorem assumption_indexed_deployment_live_fault_snapshot_retires
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract profile Fairness
      certificate session roleCount choreo deployment)
    {protocol : GlobalConfig}
    {reporter : Nat}
    {cause : SessionFault}
    {transports : List TransportState}
    (live : protocol.status = .live)
    (channelsUnique : (transports.map TransportState.channel).Nodup)
    (wellFormed : forall state, state ∈ transports -> state.WellFormed)
    (covers : forall role, role < protocol.roleCount -> role ≠ reporter ->
      role ∈ transports.map fun state => state.channel.receiver) :
    Exists fun retired =>
      AsyncCancellationReachable
        (AsyncCancellationConfig.fromTransportSnapshot
          (protocol.beginCancellation reporter cause) transports) retired /\
      retired.fullyRetired :=
  runtime.executionGuarantees.abstractRetirementAfterLiveFault
    live channelsUnique wellFormed covers

/-- Carrier rollback and protocol rollback are one atomic semantic stutter.
Ordering is the weakest profile that supplies exact poll/resolve commutation. -/
theorem ordered_deployment_requeue_is_exact_stutter
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract .ordered Fairness
      certificate session roleCount choreo deployment)
    {current offered restored : deployment.carrier.State}
    {frame : TransportFrame}
    (polled : deployment.carrier.pollReceive current = .delivered frame offered)
    (requeued : deployment.carrier.resolve offered .requeue = some restored)
    {transition : RuntimeTransitionCertificate}
    {config : GlobalConfig}
    (transitionAccepted : transition.check session roleCount choreo = true)
    (decoded : transition.before.decode? session roleCount choreo = some config)
    (effectExact : transition.effect = .requeue) :
    deployment.abstraction restored = deployment.abstraction current /\
      transition.after = transition.before := by
  exact ⟨
    ordered_profile_requeue_restores_abstract_state
      runtime.carrierProfile polled requeued,
    runtime_stutter_effect_is_zero_transition transitionAccepted decoded
      (Or.inr (Or.inr effectExact))
  ⟩

theorem assumption_indexed_deployment_duplicate_receipt_resolution_rejected
    {profile : CarrierProfile}
    {Fairness : Prop}
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {deployment : DeploymentEvidence}
    (runtime : AssumptionIndexedDeploymentContract profile Fairness
      certificate session roleCount choreo deployment)
    {current next : deployment.carrier.State}
    {resolution : ReceiptResolution}
    (resolved : deployment.carrier.resolve current resolution = some next) :
    forall later, deployment.carrier.resolve next later = none :=
  (assumption_indexed_deployment_has_mediated_carrier runtime).resolutionIsAffine
    resolved

end Hibana
