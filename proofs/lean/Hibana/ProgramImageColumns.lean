import Hibana.DescriptorBytes

namespace Hibana

def productionProgramAtomStride : Nat := 9

def productionProgramRouteResolverStride : Nat := 8

def productionProgramRouteParticipantStride : Nat := 1

def productionProgramScopeMarkerStride : Nat := 5

def productionDescriptorByteCapacity : Nat := 65535

def productionAtomOnlyEventCapacity : Nat :=
  productionDescriptorByteCapacity / productionProgramAtomStride

/-- An immutable conditional actor reference. References below 256 are physical
actors; larger references name strictly earlier rows in this list. The scope
is the original global route identity, not a newly supplied decision. -/
structure DecodedActorDecision where
  scope : Nat
  left : Nat
  right : Nat
  deriving Repr, DecidableEq

/-- Decoded columns of the eight-byte production program header. Empty actor
columns are the ordinary grammar. Owned images contain one binding per atom,
followed by the decision DAG and a nonempty initial-issuer list. -/
structure DecodedProgramColumns where
  atomCount : Nat
  routeResolverCount : Nat
  routeParticipantCount : Nat
  scopeMarkerCount : Nat
  scopeMarkerOffset : Nat
  actorBindings : List (Nat × Nat)
  actorDecisions : List DecodedActorDecision
  initialActors : List (Nat × Nat)
  deriving Repr, DecidableEq

private def decodeActorBindings?
    (bytes : List Nat) (offset count : Nat) : Option (List (Nat × Nat)) :=
  (List.range count).mapM fun row => do
    let sender ← readU16LE? bytes (offset + row * 4)
    let receiver ← readU16LE? bytes (offset + row * 4 + 2)
    pure (sender, receiver)

private def decodeActorDecisions?
    (bytes : List Nat) (offset count : Nat) : Option (List DecodedActorDecision) :=
  (List.range count).mapM fun row => do
    let scope ← readU16LE? bytes (offset + row * 6)
    let left ← readU16LE? bytes (offset + row * 6 + 2)
    let right ← readU16LE? bytes (offset + row * 6 + 4)
    pure { scope, left, right }

private def decodeInitialActors?
    (bytes : List Nat) (offset count : Nat) : Option (List (Nat × Nat)) :=
  (List.range count).mapM fun row => do
    let role ← readByte? bytes (offset + row * 2)
    let actor ← readByte? bytes (offset + row * 2 + 1)
    pure (role, actor)

/-- Decode the actual tagged header and its exact byte tail. Counts and offsets
are derived here; the caller cannot supply a different interpretation of the
same owned image. Semantic agreement with the choreography is checked after
this structural decoding. -/
def decodeProgramColumns?
    (header bytes : List Nat) : Option DecodedProgramColumns := do
  if header.length = 8 && bytes.length ≤ productionDescriptorByteCapacity then
    pure ()
  else none
  let taggedAtoms ← readU16LE? header 0
  let routeResolverCount ← readU16LE? header 2
  let routeParticipantCount ← readU16LE? header 4
  let tail ← readU16LE? header 6
  let atomCount := taggedAtoms % 32768
  if atomCount ≤ productionAtomOnlyEventCapacity then pure () else none
  let prefixLength := atomCount * 9 + routeResolverCount * 8 + routeParticipantCount
  if taggedAtoms < 32768 then
    if bytes.length = prefixLength + tail * 5 then
      pure {
        atomCount, routeResolverCount, routeParticipantCount
        scopeMarkerCount := tail
        scopeMarkerOffset := prefixLength
        actorBindings := [], actorDecisions := [], initialActors := []
      }
    else none
  else
    if 0 < atomCount && tail = bytes.length then pure () else none
    let scopeMarkerCount ← readU16LE? bytes prefixLength
    let decisionCount ← readU16LE? bytes (prefixLength + 2)
    if decisionCount ≤ 65535 / 6 then pure () else none
    let scopeMarkerOffset := prefixLength + 4
    let bindingOffset := scopeMarkerOffset + scopeMarkerCount * 5
    let decisionOffset := bindingOffset + atomCount * 4
    let initialOffset := decisionOffset + decisionCount * 6
    if initialOffset < bytes.length then pure () else none
    let remaining := bytes.length - initialOffset
    if remaining % 2 = 0 && remaining / 2 ≤ 256 then pure () else none
    let actorBindings ← decodeActorBindings? bytes bindingOffset atomCount
    let actorDecisions ← decodeActorDecisions? bytes decisionOffset decisionCount
    let initialActors ← decodeInitialActors? bytes initialOffset (remaining / 2)
    pure {
      atomCount, routeResolverCount, routeParticipantCount
      scopeMarkerCount, scopeMarkerOffset
      actorBindings, actorDecisions, initialActors
    }

/-- This is the production DAG reference domain. A decision row may only refer
to a direct actor or an earlier decision; the reserved u16 sentinel is never
an actor reference. -/
def validActorReference (roleCount earlierRows reference : Nat) : Bool :=
  if reference < 256 then reference < roleCount
  else reference < 65535 && reference - 256 < earlierRows

def DecodedActorDecision.check
    (row : DecodedActorDecision) (roleCount earlierRows : Nat)
    (routeScopes : List Nat) : Bool :=
  validActorReference roleCount earlierRows row.left &&
    validActorReference roleCount earlierRows row.right &&
    row.left != row.right && row.scope / 8192 = 0 &&
    routeScopes.contains row.scope

theorem accepted_actor_decision_references_decrease
    {row : DecodedActorDecision} {roleCount index : Nat} {routeScopes : List Nat}
    (accepted : row.check roleCount index routeScopes = true) :
    row.left < 256 + index ∧ row.right < 256 + index := by
  simp only [DecodedActorDecision.check, Bool.and_eq_true] at accepted
  have left := accepted.1.1.1.1
  have right := accepted.1.1.1.2
  constructor
  · unfold validActorReference at left
    split at left <;> simp_all <;> omega
  · unfold validActorReference at right
    split at right <;> simp_all <;> omega

end Hibana
