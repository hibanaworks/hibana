import Hibana.GlobalSyntax

namespace Hibana

def fixedArraySchemaCapacity : Nat := 2 ^ 24

def fixedArraySchemaIdentity (width : Nat) : Nat :=
  fixedArraySchemaCapacity + width

/-- The built-in fixed-array schema namespace is injective. Production Rust
admits the exact 24-bit width subdomain; width remains proof metadata only and
does not enlarge the runtime header. -/
theorem fixed_array_schema_identity_injective
    {left right : Nat}
    (same : fixedArraySchemaIdentity left = fixedArraySchemaIdentity right) :
    left = right := by
  unfold fixedArraySchemaIdentity at same
  omega

theorem fixed_array_schema_identity_excludes_builtin_scalar_domain
    {width : Nat} :
    12 < fixedArraySchemaIdentity width := by
  unfold fixedArraySchemaIdentity fixedArraySchemaCapacity
  omega

/-- Every width admitted by production Rust remains inside the fixed-array
namespace and the `u32` schema domain. -/
theorem fixed_array_schema_identity_stays_in_reserved_namespace
    {width : Nat}
    (bound : width < fixedArraySchemaCapacity) :
    fixedArraySchemaCapacity <= fixedArraySchemaIdentity width /\
      fixedArraySchemaIdentity width < 2 * fixedArraySchemaCapacity /\
      fixedArraySchemaIdentity width < 2 ^ 32 := by
  have belowDouble :
      fixedArraySchemaIdentity width <
        fixedArraySchemaCapacity + fixedArraySchemaCapacity := by
    unfold fixedArraySchemaIdentity
    exact Nat.add_lt_add_left bound fixedArraySchemaCapacity
  constructor
  · simp [fixedArraySchemaIdentity]
  · constructor
    · simpa [Nat.two_mul] using belowDouble
    · exact Nat.lt_trans belowDouble (by decide)

/-- A byte codec identified by a canonical wire schema. `Payload` is local to
the evidence package: no theorem equates nominal Rust types across binaries. -/
structure CodecImplementation (Payload : Type) where
  schema : Nat
  encode : Payload -> List (Fin 256)
  decode : List (Fin 256) -> Option Payload

/-- Cross-binary schema meaning. It describes only the canonical byte language;
payload types and values remain local to each implementation. -/
structure CanonicalWireSchema where
  identity : Nat
  accepts : List (Fin 256) -> Bool

/-- Extensional equality between one implementation and an independently
specified canonical codec. This is the proof-producing-codec boundary; it is
not a runtime vtable or endpoint type parameter. -/
structure CodecRefinement
    {Payload : Type}
    (canonical implementation : CodecImplementation Payload) : Prop where
  schemaExact : implementation.schema = canonical.schema
  encodeExact : forall payload,
    implementation.encode payload = canonical.encode payload
  decodeExact : forall bytes,
    implementation.decode bytes = canonical.decode bytes

/-- Host-side certificate for one payload schema. Round-trip and canonical-byte
laws belong to the specification; refinement transfers them to the concrete
codec implementation. -/
structure VerifiedCodec where
  Payload : Type
  wireSchema : CanonicalWireSchema
  canonical : CodecImplementation Payload
  implementation : CodecImplementation Payload
  canonicalSchemaExact : canonical.schema = wireSchema.identity
  canonicalAcceptanceExact : forall bytes,
    wireSchema.accepts bytes = (canonical.decode bytes).isSome
  canonicalRoundTrip : forall payload,
    canonical.decode (canonical.encode payload) = some payload
  canonicalBytes : forall bytes payload,
    canonical.decode bytes = some payload -> canonical.encode payload = bytes
  refinement : CodecRefinement canonical implementation

/-- Canonical payload for a fixed-width byte language. This proof-only value is
the bytes plus the width invariant; it does not model or require one nominal
Rust payload type. -/
def FixedWidthPayload (width : Nat) :=
  { bytes : List (Fin 256) // bytes.length = width }

def fixedWidthCodecImplementation
    (schema width : Nat) : CodecImplementation (FixedWidthPayload width) := {
  schema
  encode := Subtype.val
  decode := fun bytes =>
    if exact : bytes.length = width then some ⟨bytes, exact⟩ else none
}

def fixedWidthWireSchema (schema width : Nat) : CanonicalWireSchema := {
  identity := schema
  accepts := fun bytes => decide (bytes.length = width)
}

/-- General host-side certificate for exact fixed-width codecs. Production Rust
built-ins discharge their concrete encode/decode equality with Kani; this Lean
object states the independent canonical byte law used by deployment artifacts. -/
def fixedWidthVerifiedCodec (schema width : Nat) : VerifiedCodec where
  Payload := FixedWidthPayload width
  wireSchema := fixedWidthWireSchema schema width
  canonical := fixedWidthCodecImplementation schema width
  implementation := fixedWidthCodecImplementation schema width
  canonicalSchemaExact := rfl
  canonicalAcceptanceExact := by
    intro bytes
    by_cases exact : bytes.length = width
    · simp [fixedWidthWireSchema, fixedWidthCodecImplementation, exact]
    · simp [fixedWidthWireSchema, fixedWidthCodecImplementation, exact]
  canonicalRoundTrip := by
    intro payload
    have same :
        (⟨payload.val, payload.property⟩ : FixedWidthPayload width) = payload :=
      Subtype.ext (by rfl)
    simpa [fixedWidthCodecImplementation, payload.property] using congrArg some same
  canonicalBytes := by
    intro bytes payload decoded
    simp only [fixedWidthCodecImplementation] at decoded ⊢
    split at decoded
    next exact =>
      have payloadExact :
          (⟨bytes, exact⟩ : FixedWidthPayload width) = payload :=
        Option.some.inj decoded
      exact (congrArg Subtype.val payloadExact).symm
    next => simp at decoded
  refinement := {
    schemaExact := rfl
    encodeExact := fun _ => rfl
    decodeExact := fun _ => rfl
  }

def VerifiedCodec.schema (codec : VerifiedCodec) : Nat :=
  codec.wireSchema.identity

@[simp]
theorem fixed_width_verified_codec_schema (schema width : Nat) :
    (fixedWidthVerifiedCodec schema width).schema = schema := rfl

theorem verified_codec_schema_is_exact
    (codec : VerifiedCodec) :
    codec.implementation.schema = codec.schema :=
  codec.refinement.schemaExact.trans codec.canonicalSchemaExact

theorem verified_codec_implementation_acceptance_is_exact
    (codec : VerifiedCodec)
    (bytes : List (Fin 256)) :
    codec.wireSchema.accepts bytes =
      (codec.implementation.decode bytes).isSome := by
  rw [codec.refinement.decodeExact]
  exact codec.canonicalAcceptanceExact bytes

theorem verified_codec_implementation_round_trip
    (codec : VerifiedCodec)
    (payload : codec.Payload) :
    codec.implementation.decode (codec.implementation.encode payload) =
      some payload := by
  rw [codec.refinement.encodeExact, codec.refinement.decodeExact]
  exact codec.canonicalRoundTrip payload

theorem verified_codec_implementation_accepts_only_canonical_bytes
    (codec : VerifiedCodec)
    (bytes : List (Fin 256))
    (payload : codec.Payload)
    (decoded : codec.implementation.decode bytes = some payload) :
    codec.implementation.encode payload = bytes := by
  rw [codec.refinement.decodeExact] at decoded
  rw [codec.refinement.encodeExact]
  exact codec.canonicalBytes bytes payload decoded

/-- A schema identity has one canonical byte language throughout an evidence
set, even when implementations use different local payload types. -/
def CanonicalWireSchemaAgreement
    (codecs : List VerifiedCodec) : Prop :=
  forall {left right : VerifiedCodec},
    left ∈ codecs ->
    right ∈ codecs ->
    left.schema = right.schema ->
      left.wireSchema = right.wireSchema

def fixedWidthCodecRegistry
    (entries : List (Nat × Nat)) : List VerifiedCodec :=
  entries.map fun entry => fixedWidthVerifiedCodec entry.1 entry.2

def FixedWidthSchemaRegistryWellFormed
    (entries : List (Nat × Nat)) : Prop :=
  forall left, left ∈ entries ->
    forall right, right ∈ entries ->
      left.1 = right.1 -> left = right

def checkFixedWidthSchemaRegistry (entries : List (Nat × Nat)) : Bool :=
  entries.all fun left =>
    entries.all fun right =>
      if left.1 = right.1 then decide (left = right) else true

theorem fixed_width_schema_registry_checker_sound
    {entries : List (Nat × Nat)}
    (accepted : checkFixedWidthSchemaRegistry entries = true) :
    FixedWidthSchemaRegistryWellFormed entries := by
  unfold checkFixedWidthSchemaRegistry at accepted
  intro left leftMember right rightMember sameIdentity
  have leftAccepted := List.all_eq_true.mp accepted left leftMember
  have pairAccepted := List.all_eq_true.mp leftAccepted right rightMember
  simp [sameIdentity] at pairAccepted
  exact pairAccepted

theorem fixed_width_codec_registry_agrees
    {entries : List (Nat × Nat)}
    (wellFormed : FixedWidthSchemaRegistryWellFormed entries) :
    CanonicalWireSchemaAgreement (fixedWidthCodecRegistry entries) := by
  intro left right leftMember rightMember sameSchema
  obtain ⟨leftEntry, leftEntryMember, rfl⟩ := List.mem_map.mp leftMember
  obtain ⟨rightEntry, rightEntryMember, rfl⟩ := List.mem_map.mp rightMember
  have identityExact : leftEntry.1 = rightEntry.1 := by
    simpa using sameSchema
  have entryExact := wellFormed leftEntry leftEntryMember
    rightEntry rightEntryMember identityExact
  subst rightEntry
  rfl

/-- Every schema named by the choreography has a generated or independently
verified codec, and equal schema identities denote one canonical byte language.
Coverage is stronger than schema-ID agreement and remains an optional
deployment proof obligation. -/
def VerifiedCodecCoverage
    (choreo : Choreo)
    (codecs : List VerifiedCodec) : Prop :=
  CanonicalWireSchemaAgreement codecs /\
    forall event,
      event ∈ choreo.globalEvents ->
        Exists fun codec => codec ∈ codecs /\ codec.schema = event.schema

theorem verified_codec_coverage_has_unique_wire_schema
    {choreo : Choreo}
    {codecs : List VerifiedCodec}
    (coverage : VerifiedCodecCoverage choreo codecs)
    {left right : VerifiedCodec}
    (leftMember : left ∈ codecs)
    (rightMember : right ∈ codecs)
    (sameSchema : left.schema = right.schema) :
    left.wireSchema = right.wireSchema :=
  coverage.1 leftMember rightMember sameSchema

theorem verified_codec_coverage_binds_event_schema
    {choreo : Choreo}
    {codecs : List VerifiedCodec}
    (coverage : VerifiedCodecCoverage choreo codecs)
    {globalId : Nat}
    {event : GlobalEvent}
    (eventAt : choreo.globalEvents[globalId]? = some event) :
    Exists fun codec => codec ∈ codecs /\
      codec.implementation.schema = event.schema := by
  obtain ⟨bound, exactAt⟩ := List.getElem?_eq_some_iff.mp eventAt
  have eventMember : choreo.globalEvents[globalId]'bound ∈ choreo.globalEvents :=
    List.getElem_mem bound
  rw [exactAt] at eventMember
  obtain ⟨codec, member, schemaExact⟩ := coverage.2 event eventMember
  exact ⟨codec, member,
    (verified_codec_schema_is_exact codec).trans schemaExact⟩

end Hibana
