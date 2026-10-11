import Hibana.DescriptorImage

/- Durable regression proofs against the actual production reference phase.
   Baseline colors and scope IDs are frozen before the final greedy pass.
   Runtime coverage still requires Covers / SameClassUnique; this file proves
   allocation properties and finite source correspondence, not that premise. -/
namespace Hibana.WireFrameRefinement
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000




/-- Proof-only predicates naming the actual inline prior-row test. They do not
    allocate, and actual_head_uses_this_predicate below is definitional equality. -/
def rollFrameConflict (atom : ProgramAtomBody) (owner : Nat)
    (prior : ElasticFrameAssignment) : Bool :=
  prior.original.sender == atom.sender &&
    prior.original.receiver == atom.receiver && prior.original.lane == atom.lane &&
    (prior.original.frameLabel != atom.frameLabel || prior.owner != owner)

def rollFrameUsed (atom : ProgramAtomBody) (owner : Nat)
    (assigned : List ElasticFrameAssignment) : List Nat :=
  assigned.filterMap fun entry =>
    if rollFrameConflict atom owner entry then some entry.color else none

def isRollFrameEnter (marker : DecodedScopeMarker) : Bool :=
  marker.scope / 8192 == 1 && marker.tag % 4 == 0

theorem actual_head_uses_this_predicate
    (markers : List DecodedScopeMarker) (assigned : List ElasticFrameAssignment)
    (index : Nat) (atom : ProgramAtomBody) (rest : List ProgramAtomBody) :
    (separateElasticFrameDomainsFrom markers assigned index (atom :: rest)).head?.map
      ProgramAtomBody.frameLabel =
      some (if atom.sender == atom.receiver then atom.frameLabel else
        firstElasticFrameColor (rollFrameUsed atom (elasticFrameOwner markers index) assigned)) := rfl

theorem actual_palette_matches_optional_search (used : List Nat) :
    firstElasticFrameColor used =
      match firstAvailableFrameLabel used with | some color => color | none => 256 := rfl

/-- A named proof view of the external function's inline fold step. It is not
    an allocator; the next theorem binds it definitionally to the actual fold. -/
def ownerTransition (markers : List DecodedScopeMarker) (index : Nat)
    (best : Option (Nat × Nat)) (marker : DecodedScopeMarker) : Option (Nat × Nat) :=
  if marker.scope / 8192 == 1 && marker.tag % 4 == 0 && marker.offset ≤ index then
    match markers.find? (fun closing =>
        closing.scope == marker.scope && closing.tag % 4 == 2) with
    | none => best
    | some closing =>
        if index < closing.offset then
          let span := closing.offset - marker.offset
          let owner := marker.scope % 8192 + 1
          match best with
          | none => some (span, owner)
          | some (oldSpan, oldOwner) =>
              if span < oldSpan || (span == oldSpan && oldOwner < owner) then
                some (span, owner)
              else best
        else best
  else best

theorem actual_owner_is_this_fold (markers : List DecodedScopeMarker) (index : Nat) :
    elasticFrameOwner markers index =
      match markers.foldl (ownerTransition markers index) none with
      | none => 0
      | some (_, owner) => owner := rfl

theorem narrower_roll_owner_wins
    (roll : isRollFrameEnter marker = true) (start : marker.offset ≤ event)
    (found : markers.find? (fun closing =>
      closing.scope == marker.scope && closing.tag % 4 == 2) = some closing)
    (finish : event < closing.offset) (narrower : closing.offset - marker.offset < span) :
    ownerTransition markers event (some (span, owner)) marker =
      some (closing.offset - marker.offset, marker.scope % 8192 + 1) := by
  unfold ownerTransition
  unfold isRollFrameEnter at roll
  rw [if_pos (Bool.and_eq_true_iff.mpr ⟨roll, decide_eq_true_eq.mpr start⟩)]
  rw [found]
  dsimp only
  rw [if_pos finish]
  exact if_pos (Bool.or_eq_true_iff.mpr (Or.inl (decide_eq_true_eq.mpr narrower)))

theorem coextensive_roll_owner_uses_later_source_ordinal
    (roll : isRollFrameEnter marker = true) (start : marker.offset ≤ event)
    (found : markers.find? (fun closing =>
      closing.scope == marker.scope && closing.tag % 4 == 2) = some closing)
    (finish : event < closing.offset) (later : owner < marker.scope % 8192 + 1) :
    ownerTransition markers event (some (closing.offset - marker.offset, owner)) marker =
      some (closing.offset - marker.offset, marker.scope % 8192 + 1) := by
  unfold ownerTransition
  unfold isRollFrameEnter at roll
  rw [if_pos (Bool.and_eq_true_iff.mpr ⟨roll, decide_eq_true_eq.mpr start⟩)]
  rw [found]
  dsimp only
  rw [if_pos finish]
  exact if_pos (Bool.or_eq_true_iff.mpr (Or.inr
    (Bool.and_eq_true_iff.mpr ⟨beq_iff_eq.mpr rfl, decide_eq_true_eq.mpr later⟩)))

theorem canonical_source_applies_exactly_one_refinement (choreo : Choreo) :
    (canonicalProgramSource choreo).atoms =
      separateElasticFrameDomains (canonicalControlSource choreo).markers
        (choreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody) := rfl

theorem chosen_color_bound_and_unused
    (success : firstAvailableFrameLabel used = some color) :
    color < 256 ∧ color ∉ used := by
  have chosen := List.find?_some success
  exact ⟨List.mem_range.mp (List.mem_of_find?_eq_some success), by
    simpa using chosen⟩

theorem color_exhaustion_is_exact :
    firstAvailableFrameLabel used = none ↔
      ∀ color, color < 256 → color ∈ used := by
  simp [firstAvailableFrameLabel, List.find?_eq_none]

theorem actual_color_is_bounded_or_invalid (used : List Nat) :
    firstElasticFrameColor used < 256 ∨ firstElasticFrameColor used = 256 := by
  rw [actual_palette_matches_optional_search]
  cases chosen : firstAvailableFrameLabel used with
  | none => exact Or.inr rfl
  | some color => exact Or.inl (chosen_color_bound_and_unused chosen).1

theorem actual_color_exhaustion_is_exact (used : List Nat) :
    firstElasticFrameColor used = 256 ↔ ∀ color, color < 256 → color ∈ used := by
  constructor
  · intro exhausted
    cases chosen : firstAvailableFrameLabel used with
    | none => exact color_exhaustion_is_exact.mp chosen
    | some color =>
        have bounded := (chosen_color_bound_and_unused chosen).1
        rw [actual_palette_matches_optional_search, chosen] at exhausted
        simp only at exhausted
        omega
  · intro full
    rw [actual_palette_matches_optional_search, color_exhaustion_is_exact.mpr full]

theorem chosen_color_separates_every_conflicting_prior
    (success : firstAvailableFrameLabel (rollFrameUsed atom owner assigned) = some color)
    (member : prior ∈ assigned) (conflict : rollFrameConflict atom owner prior = true) :
    color ≠ prior.color := by
  intro same
  have used : prior.color ∈ rollFrameUsed atom owner assigned := by
    exact List.mem_filterMap.mpr ⟨prior, member, by simp [conflict]⟩
  exact (chosen_color_bound_and_unused success).2 (same ▸ used)

theorem actual_head_separates_conflicting_prior
    (remote : atom.sender ≠ atom.receiver)
    (success : firstAvailableFrameLabel
      (rollFrameUsed atom (elasticFrameOwner markers index) assigned) = some color)
    (member : prior ∈ assigned)
    (conflict : rollFrameConflict atom (elasticFrameOwner markers index) prior = true) :
    (separateElasticFrameDomainsFrom markers assigned index (atom :: rest)).head?.map
      ProgramAtomBody.frameLabel = some color ∧ color ≠ prior.color := by
  constructor
  · rw [actual_head_uses_this_predicate]
    have different : ¬ (atom.sender == atom.receiver) = true :=
      fun same => remote (beq_iff_eq.mp same)
    rw [if_neg different, actual_palette_matches_optional_search, success]
  · exact chosen_color_separates_every_conflicting_prior success member conflict

theorem original_inequality_is_conflict
    (sender : prior.original.sender = atom.sender)
    (receiver : prior.original.receiver = atom.receiver)
    (lane : prior.original.lane = atom.lane)
    (different : prior.original.frameLabel ≠ atom.frameLabel) :
    rollFrameConflict atom owner prior = true := by
  exact Bool.and_eq_true_iff.mpr ⟨Bool.and_eq_true_iff.mpr
    ⟨Bool.and_eq_true_iff.mpr ⟨beq_iff_eq.mpr sender, beq_iff_eq.mpr receiver⟩,
      beq_iff_eq.mpr lane⟩, Bool.or_eq_true_iff.mpr (Or.inl (bne_iff_ne.mpr different))⟩

theorem distinct_owner_is_conflict
    (sender : prior.original.sender = atom.sender)
    (receiver : prior.original.receiver = atom.receiver)
    (lane : prior.original.lane = atom.lane) (different : prior.owner ≠ owner) :
    rollFrameConflict atom owner prior = true := by
  exact Bool.and_eq_true_iff.mpr ⟨Bool.and_eq_true_iff.mpr
    ⟨Bool.and_eq_true_iff.mpr ⟨beq_iff_eq.mpr sender, beq_iff_eq.mpr receiver⟩,
      beq_iff_eq.mpr lane⟩, Bool.or_eq_true_iff.mpr (Or.inr (bne_iff_ne.mpr different))⟩

theorem same_class_does_not_block
    (sameColor : prior.original.frameLabel = atom.frameLabel)
    (sameOwner : prior.owner = owner) :
    rollFrameConflict atom owner prior = false := by
  simp [rollFrameConflict, sameColor, sameOwner]

theorem different_domain_does_not_block
    (different : prior.original.sender ≠ atom.sender ∨
      prior.original.receiver ≠ atom.receiver ∨ prior.original.lane ≠ atom.lane) :
    rollFrameConflict atom owner prior = false := by
  rcases different with hs | hr | hl <;> simp [rollFrameConflict, *]

theorem refinement_preserves_all_noncolor_fields
    (observe : ProgramAtomBody → α)
    (erase : ∀ atom color, observe { atom with frameLabel := color } = observe atom)
    (markers : List DecodedScopeMarker) (index : Nat)
    (assigned : List ElasticFrameAssignment) (atoms : List ProgramAtomBody) :
    (separateElasticFrameDomainsFrom markers assigned index atoms).map observe =
      atoms.map observe := by
  induction atoms generalizing index assigned with
  | nil => rfl
  | cons atom rest ih =>
      simp only [separateElasticFrameDomainsFrom, List.map_cons, erase]
      rw [ih]

theorem refinement_preserves_event_inventory
    (markers : List DecodedScopeMarker) (index : Nat)
    (assigned : List ElasticFrameAssignment) (atoms : List ProgramAtomBody) :
    (separateElasticFrameDomainsFrom markers assigned index atoms).length = atoms.length := by
  induction atoms generalizing index assigned with
  | nil => rfl
  | cons atom rest ih => simp [separateElasticFrameDomainsFrom, ih]

theorem no_roll_is_identity
    (absent : markers.any isRollFrameEnter = false) :
    separateElasticFrameDomains markers atoms = atoms := by
  change (if markers.any isRollFrameEnter then _ else atoms) = atoms
  rw [absent]
  rfl

theorem self_send_keeps_original_label
    (same : atom.sender = atom.receiver) :
    (separateElasticFrameDomainsFrom markers assigned index (atom :: rest)).head?.map
        ProgramAtomBody.frameLabel = some atom.frameLabel := by
  change some (if atom.sender == atom.receiver then atom.frameLabel else _) = some atom.frameLabel
  rw [beq_iff_eq.mpr same]
  rfl

theorem exhaustion_retains_invalid_nonempty_row
    (remote : atom.sender ≠ atom.receiver)
    (full : firstAvailableFrameLabel
      (rollFrameUsed atom (elasticFrameOwner markers index) assigned) = none) :
    (separateElasticFrameDomainsFrom markers assigned index (atom :: rest)).head?.map
        ProgramAtomBody.frameLabel = some 256 := by
  rw [actual_head_uses_this_predicate]
  have different : ¬ (atom.sender == atom.receiver) = true :=
    fun same => remote (beq_iff_eq.mp same)
  rw [if_neg different, actual_palette_matches_optional_search, full]

theorem invalid_color_cannot_equal_decoded_byte (byte : Nat) (valid : byte < 256) :
    byte ≠ (none : Option Nat).getD 256 := by
  simp only [Option.getD_none]
  omega

theorem read_byte_result_is_bounded
    (decoded : readByte? bytes offset = some color) : color < 256 := by
  cases atByte : bytes[offset]? with
  | none => simp [readByte?, atByte] at decoded
  | some byte =>
      by_cases bounded : byte < 256
      · simp [readByte?, atByte, bounded] at decoded
        exact decoded ▸ bounded
      · simp [readByte?, atByte, bounded] at decoded

theorem decoded_frame_label_is_bounded
    {image : RustDescriptorImage} {event color : Nat}
    (decoded : image.eventFrameLabel? event = some color) : color < 256 := by
  by_cases inRange : event < image.eventCount
  · simp [RustDescriptorImage.eventFrameLabel?, inRange] at decoded
    exact read_byte_result_is_bounded decoded
  · simp [RustDescriptorImage.eventFrameLabel?, inRange] at decoded

theorem successful_map_has_only_bounded_results
    (f : Nat → Option Nat) (bound : ∀ index color, f index = some color → color < 256)
    (indices colors : List Nat) (success : indices.mapM f = some colors) :
    ∀ color ∈ colors, color < 256 := by
  induction indices generalizing colors with
  | nil =>
      simp only [List.mapM_nil, Option.pure_def, Option.some.injEq] at success
      subst colors
      simp
  | cons index rest ih =>
      cases head : f index with
      | none => simp [List.mapM_cons, head] at success
      | some color =>
          cases tail : rest.mapM f with
          | none => simp [List.mapM_cons, head, tail] at success
          | some others =>
              simp [List.mapM_cons, head, tail] at success
              subst colors
              intro candidate member
              rcases List.mem_cons.mp member with rfl | member
              · exact bound index candidate head
              · exact ih others tail candidate member

/-- This is the unchanged exact-label admission conjunct: a retained 256 row
    cannot compare equal to any successfully decoded descriptor label list.
    The explicit member premise rules out an empty-list/vacuous rejection. -/
theorem exhausted_row_rejects_exact_decoded_labels
    (image : RustDescriptorImage) (labels : List Nat) (invalid : 256 ∈ labels) :
    image.decodeEventFrameLabels? ≠ some labels := by
  intro decoded
  have bounded := successful_map_has_only_bounded_results image.eventFrameLabel?
    (fun _ _ accepted => decoded_frame_label_is_bounded accepted)
    (List.range image.eventCount) labels decoded 256 invalid
  omega

def send (label : Nat) : Choreo := .send 0 1 label 0
def labels (choreo : Choreo) : List Nat :=
  (canonicalProgramSource choreo).atoms.map ProgramAtomBody.frameLabel

theorem ordinary_ordered_reuse : labels (.seq (send 71) (send 72)) = [0, 0] := by decide
theorem one_roll_ordered_reuse : labels (.roll (.seq (send 71) (send 72))) = [0, 0] := by decide
theorem nested_roll_source_correspondence :
    labels (.roll (.seq (.roll (.seq (send 71) (send 72))) (send 73))) = [0, 0, 1] := by decide
theorem reverse_nested_roll_source_correspondence :
    labels (.roll (.seq (send 71) (.roll (send 72)))) = [0, 1] := by decide
theorem coextensive_roll_source_correspondence :
    labels (.roll (.seq (send 71) (.seq (.roll (.roll (send 72))) (send 73)))) = [0, 1, 0] := by decide
theorem sibling_roll_source_correspondence :
    labels (.seq (.roll (send 71)) (.roll (send 72))) = [0, 1] := by decide
theorem existing_route_inequality_is_preserved :
    labels (.roll (.route .intrinsic (send 71) (send 72))) = [0, 1] := by decide
theorem coextensive_owner_uses_later_ordinal :
    elasticFrameOwner [⟨0, 8194, 0⟩, ⟨0, 8193, 0⟩, ⟨1, 8193, 2⟩, ⟨1, 8194, 2⟩] 0 = 3 := by decide
theorem narrowest_owner_overrides_ordinal :
    elasticFrameOwner [⟨0, 8194, 0⟩, ⟨1, 8193, 0⟩, ⟨2, 8193, 2⟩, ⟨3, 8194, 2⟩] 1 = 2 := by decide
theorem all_byte_colors_are_available : firstAvailableFrameLabel (List.range 255) = some 255 := by decide
theorem all_byte_colors_exhausted : firstAvailableFrameLabel (List.range 256) = none := by decide

def partitionFixture : List ProgramAtomBody := [
  ⟨0, 1, 0, 0, 0, 0, 0⟩, ⟨0, 1, 0, 0, 0, 0, 1⟩,
  ⟨2, 1, 0, 0, 0, 0, 0⟩, ⟨0, 2, 0, 0, 0, 0, 0⟩,
  ⟨0, 1, 0, 0, 0, 1, 0⟩, ⟨0, 0, 0, 0, 0, 0, 7⟩]
theorem exact_domain_partition_and_self_send :
    (separateElasticFrameDomains [⟨0, 8192, 0⟩, ⟨6, 8192, 2⟩] partitionFixture).map
        ProgramAtomBody.frameLabel = [0, 1, 0, 0, 0, 7] := by decide

#print axioms chosen_color_bound_and_unused
#print axioms color_exhaustion_is_exact
#print axioms chosen_color_separates_every_conflicting_prior
#print axioms original_inequality_is_conflict
#print axioms distinct_owner_is_conflict
#print axioms same_class_does_not_block
#print axioms different_domain_does_not_block
#print axioms refinement_preserves_all_noncolor_fields
#print axioms refinement_preserves_event_inventory
#print axioms no_roll_is_identity
#print axioms self_send_keeps_original_label
#print axioms exhaustion_retains_invalid_nonempty_row
#print axioms invalid_color_cannot_equal_decoded_byte
#print axioms read_byte_result_is_bounded
#print axioms decoded_frame_label_is_bounded
#print axioms successful_map_has_only_bounded_results
#print axioms exhausted_row_rejects_exact_decoded_labels
#print axioms nested_roll_source_correspondence
#print axioms coextensive_roll_source_correspondence
#print axioms all_byte_colors_exhausted

/-- An unrelated role can omit an invalid global row. Therefore per-role exact
    rejection requires the explicit retained-row premise below; it does not
    prove global Rust capacity rejection for arbitrary synthetic certificates. -/
theorem unrelated_role_can_omit_invalid_row :
    ([({ sender := 0, receiver := 1, label := 0, schema := 0,
          origin := 0, lane := 0, frameLabel := 256 } : ProgramAtomBody)].filterMap
      fun atom => if (Choreo.localAction? 2 atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) = [] := by decide

theorem participating_role_retains_invalid_row :
    ([({ sender := 0, receiver := 1, label := 0, schema := 0,
          origin := 0, lane := 0, frameLabel := 256 } : ProgramAtomBody)].filterMap
      fun atom => if (Choreo.localAction? 1 atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) = [256] := by decide

theorem retained_overflow_rejects_role_admission
    (image : RustDescriptorImage) (atoms : List ProgramAtomBody) (role : Nat)
    (retained : 256 ∈ atoms.filterMap fun atom =>
      if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) :
    image.decodeEventFrameLabels? ≠ some (atoms.filterMap fun atom =>
      if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) :=
  exhausted_row_rejects_exact_decoded_labels image _ retained

#print axioms unrelated_role_can_omit_invalid_row
#print axioms participating_role_retains_invalid_row
#print axioms retained_overflow_rejects_role_admission
end Hibana.WireFrameRefinement
