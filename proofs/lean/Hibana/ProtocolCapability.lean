import Hibana.CodecEvidence
import Hibana.ProtocolArtifact

namespace Hibana

/-- Protocol-neutral capabilities of the normalized choreography language.
These names describe Hibana's own finite core; no algorithm or carrier-specific
vocabulary enters the proof or runtime surface. -/
inductive ProtocolCapability where
  | communication
  | sequencing
  | parallelComposition
  | intrinsicChoice
  | resolvedChoice
  | recursion
  deriving Repr, DecidableEq, BEq

def requiredProtocolCapabilities : List ProtocolCapability :=
  [.communication, .sequencing, .parallelComposition, .intrinsicChoice,
    .resolvedChoice, .recursion]

def Choreo.hasCapability (capability : ProtocolCapability) : Choreo -> Bool
  | .send _ _ _ _ => capability == .communication
  | .seq left right =>
      capability == .sequencing ||
        left.hasCapability capability || right.hasCapability capability
  | .par left right =>
      capability == .parallelComposition ||
        left.hasCapability capability || right.hasCapability capability
  | .route authority left right =>
      (match authority with
        | .intrinsic => capability == .intrinsicChoice
        | .dynamic _ => capability == .resolvedChoice) ||
        left.hasCapability capability || right.hasCapability capability
  | .roll body =>
      capability == .recursion || body.hasCapability capability

/-- One already accepted production protocol artifact. Proof metadata is
host-only and does not generate a continuation type or resident capability
table. -/
structure VerifiedProtocolMember where
  session : Nat
  roleCount : Nat
  choreo : Choreo
  certificate : VerifiedProtocolCertificate
  accepted : certificate.check session roleCount choreo = true
  codecs : List VerifiedCodec
  codecCoverage : VerifiedCodecCoverage choreo codecs

def VerifiedProtocolFamilyCapabilityCoverage
    (family : List VerifiedProtocolMember) : Prop :=
  forall capability,
    capability ∈ requiredProtocolCapabilities ->
      Exists fun member =>
        member ∈ family /\ member.choreo.hasCapability capability = true

def checkVerifiedProtocolFamilyCapabilities
    (family : List VerifiedProtocolMember) : Bool :=
  requiredProtocolCapabilities.all fun capability =>
    family.any fun member => member.choreo.hasCapability capability

theorem verified_protocol_family_capability_checker_sound
    {family : List VerifiedProtocolMember}
    (accepted : checkVerifiedProtocolFamilyCapabilities family = true) :
    VerifiedProtocolFamilyCapabilityCoverage family := by
  unfold checkVerifiedProtocolFamilyCapabilities at accepted
  intro capability required
  have covered := List.all_eq_true.mp accepted capability required
  obtain ⟨member, present, exact⟩ := List.any_eq_true.mp covered
  exact ⟨member, present, exact⟩

theorem verified_protocol_family_member_has_execution_guarantees
    (member : VerifiedProtocolMember) :
    ProtocolExecutionGuarantees member.certificate member.session
      member.roleCount member.choreo :=
  verified_protocol_establishes_execution_guarantees
    member.accepted

theorem verified_protocol_family_member_has_canonical_codec_coverage
    (member : VerifiedProtocolMember) :
    VerifiedCodecCoverage member.choreo member.codecs :=
  member.codecCoverage

end Hibana
