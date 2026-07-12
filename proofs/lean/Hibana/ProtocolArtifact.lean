import Hibana.DescriptorRefinement
import Hibana.GlobalCoherence

namespace Hibana

def checkDescriptorCertificates
    (choreo : Choreo) : Nat -> List ExactDescriptorCertificate -> Bool
  | _, [] => true
  | role, certificate :: rest =>
      certificate.check &&
      decide (certificate.choreo = choreo) &&
      decide (certificate.image.role = role) &&
      checkDescriptorCertificates choreo (role + 1) rest

private theorem descriptor_certificates_sound_at
    {choreo : Choreo}
    {start : Nat}
    {certificates : List ExactDescriptorCertificate}
    (accepted : checkDescriptorCertificates choreo start certificates = true) :
    ∀ (offset : Nat) (certificate : ExactDescriptorCertificate),
      certificates[offset]? = some certificate ->
      certificate.check = true /\
      certificate.Refines /\
      certificate.choreo = choreo /\
      certificate.image.role = start + offset := by
  induction certificates generalizing start with
  | nil =>
      intro offset certificate present
      simp at present
  | cons head tail inductionHypothesis =>
      simp [checkDescriptorCertificates] at accepted
      intro offset certificate present
      cases offset with
      | zero =>
          have exactCertificate : head = certificate := Option.some.inj (by
            simpa using present)
          subst certificate
          exact ⟨accepted.1.1.1,
            exact_descriptor_certificate_sound accepted.1.1.1,
            accepted.1.1.2, by simpa using accepted.1.2⟩
      | succ offset =>
          have tailPresent : tail[offset]? = some certificate := by
            simpa using present
          have verified := inductionHypothesis accepted.2
            offset certificate tailPresent
          refine ⟨verified.1, verified.2.1, verified.2.2.1, ?_⟩
          simpa [Nat.add_assoc, Nat.add_comm 1 offset] using verified.2.2.2

/-- One proof-producing host artifact binds all role descriptor bytes to the
same choreography and to its all-role projectability closure. -/
structure VerifiedProtocolCertificate where
  projectability : ProjectabilityCertificate
  descriptors : List ExactDescriptorCertificate

def VerifiedProtocolCertificate.check
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  certificate.projectability.check session roleCount choreo &&
  decide (certificate.descriptors.length = roleCount) &&
  checkDescriptorCertificates choreo 0 certificate.descriptors

def VerifiedProtocolCertificate.AllRolesRefine
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  certificate.projectability.Projectable session roleCount choreo /\
  certificate.descriptors.length = roleCount /\
  ∀ (role : Nat) (descriptor : ExactDescriptorCertificate),
    certificate.descriptors[role]? = some descriptor ->
    descriptor.check = true /\ descriptor.Refines /\
      descriptor.choreo = choreo /\ descriptor.image.role = role

theorem verified_protocol_certificate_establishes_all_role_refinement
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.AllRolesRefine session roleCount choreo := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨projectability_certificate_sound accepted.1.1,
    of_decide_eq_true accepted.1.2, ?_⟩
  intro role descriptor present
  simpa using descriptor_certificates_sound_at
    accepted.2 role descriptor present

theorem verified_protocol_descriptor_actions_match_projection
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    descriptor.image.decodeActions? =
      some ((projectGraph role choreo).events.map Event.action) := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  have descriptorVerified := verified.2.2 role descriptor present
  have actions := accepted_descriptor_actions_match_projection
    (certificate := descriptor)
    descriptorVerified.1
  simpa [descriptorVerified.2.2.1, descriptorVerified.2.2.2] using actions

theorem verified_protocol_has_injective_inbound_evidence
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    choreo.InboundOccurrenceIdentity /\
      descriptor.image.decodeEventFrameLabels? =
        some (choreo.canonicalRoleFrameLabels role) := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  have descriptorVerified := verified.2.2 role descriptor present
  have staticIdentity := verified.1.2.2.2.inboundOccurrenceIdentity
  have roleImage := descriptorVerified.2.1.2.1.2.2.2.1.2.1
  simpa [descriptorVerified.2.2.1, descriptorVerified.2.2.2] using
    And.intro staticIdentity roleImage

/-- Every raw header admitted under a verified protocol artifact denotes one
global occurrence. The theorem composes production descriptor bytes, static
evidence injectivity, and transport admission without adding wire fields. -/
theorem verified_protocol_transport_admission_is_unique
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {frame : TransportFrame}
    {leftId rightId : Nat} {leftEvent rightEvent : GlobalEvent}
    (left : TransportAdmission choreo frame leftId leftEvent)
    (right : TransportAdmission choreo frame rightId rightEvent) :
    leftId = rightId /\ leftEvent = rightEvent := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  exact transport_admission_is_unique
    verified.1.2.2.2.inboundOccurrenceIdentity left right

end Hibana
