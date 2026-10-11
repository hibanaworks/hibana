import Hibana.DescriptorImage

/- Durable regression proofs against the actual production reference phase.
   Baseline colors and scope IDs are frozen before the final greedy pass.
   Runtime coverage still requires Covers / SameClassUnique; this file proves
   allocation properties and finite source correspondence, not that premise. -/
namespace Hibana.WireFrameRefinement
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000



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

theorem chosen_color_separates_every_conflicting_prior
    (success : firstAvailableFrameLabel (rollFrameUsed atom owner assigned) = some color)
    (member : prior ∈ assigned) (conflict : rollFrameConflict atom owner prior = true) :
    color ≠ prior.color := by
  intro same
  have used : prior.color ∈ rollFrameUsed atom owner assigned := by
    exact List.mem_map.mpr ⟨prior, List.mem_filter.mpr ⟨member, conflict⟩, rfl⟩
  exact (chosen_color_bound_and_unused success).2 (same ▸ used)

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

theorem narrower_roll_owner_wins
    (roll : isRollFrameEnter marker = true) (start : marker.offset ≤ event)
    (finish : event < stop) (narrower : stop - marker.offset < span) :
    selectRollFrameOwner event stop marker (some (span, owner)) =
      some (stop - marker.offset, marker.scope % 8192 + 1) := by
  simp [selectRollFrameOwner, roll, start, finish, narrower]

theorem coextensive_roll_owner_uses_later_source_ordinal
    (roll : isRollFrameEnter marker = true) (start : marker.offset ≤ event)
    (finish : event < stop) (later : owner < marker.scope % 8192 + 1) :
    selectRollFrameOwner event stop marker (some (stop - marker.offset, owner)) =
      some (stop - marker.offset, marker.scope % 8192 + 1) := by
  unfold selectRollFrameOwner
  apply Eq.trans (if_pos (Bool.and_eq_true_iff.mpr
    ⟨Bool.and_eq_true_iff.mpr ⟨roll, decide_eq_true_eq.mpr start⟩,
      decide_eq_true_eq.mpr finish⟩))
  exact if_pos (Bool.or_eq_true_iff.mpr (Or.inr
    (Bool.and_eq_true_iff.mpr ⟨beq_iff_eq.mpr rfl, decide_eq_true_eq.mpr later⟩)))

theorem different_domain_does_not_block
    (different : prior.original.sender ≠ atom.sender ∨
      prior.original.receiver ≠ atom.receiver ∨ prior.original.lane ≠ atom.lane) :
    rollFrameConflict atom owner prior = false := by
  rcases different with hs | hr | hl <;> simp [rollFrameConflict, *]

theorem refinement_preserves_all_noncolor_fields
    (observe : ProgramAtomBody → α)
    (erase : ∀ atom color, observe { atom with frameLabel := color } = observe atom)
    (markers : List DecodedScopeMarker) (atomCount index : Nat)
    (assigned : List RollFrameAssignment) (atoms : List ProgramAtomBody) :
    (separateRollFrameAtomsFrom markers atomCount index assigned atoms).map observe =
      atoms.map observe := by
  induction atoms generalizing index assigned with
  | nil => rfl
  | cons atom rest ih =>
      simp only [separateRollFrameAtomsFrom, List.map_cons, erase]
      rw [ih]

theorem refinement_preserves_event_inventory
    (markers : List DecodedScopeMarker) (atomCount index : Nat)
    (assigned : List RollFrameAssignment) (atoms : List ProgramAtomBody) :
    (separateRollFrameAtomsFrom markers atomCount index assigned atoms).length = atoms.length := by
  induction atoms generalizing index assigned with
  | nil => rfl
  | cons atom rest ih => simp [separateRollFrameAtomsFrom, ih]

theorem no_roll_is_identity
    (absent : markers.any isRollFrameEnter = false) :
    separateRollFrameAtoms markers atoms = atoms := by
  simp [separateRollFrameAtoms, absent]

theorem self_send_keeps_original_label
    (same : atom.sender = atom.receiver) :
    (separateRollFrameAtomsFrom markers count index assigned (atom :: rest)).head?.map
        ProgramAtomBody.frameLabel = some atom.frameLabel := by
  change some (if atom.sender == atom.receiver then atom.frameLabel else _) = some atom.frameLabel
  rw [beq_iff_eq.mpr same]
  rfl

theorem exhaustion_retains_invalid_nonempty_row
    (remote : atom.sender ≠ atom.receiver)
    (full : firstAvailableFrameLabel
      (rollFrameUsed atom (rollFrameOwner markers count index) assigned) = none) :
    (separateRollFrameAtomsFrom markers count index assigned (atom :: rest)).head?.map
        ProgramAtomBody.frameLabel = some 256 := by
  simp [separateRollFrameAtomsFrom, remote, full]

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
  (canonicalWireAtoms choreo).map ProgramAtomBody.frameLabel

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
    rollFrameOwner [⟨0, 8194, 0⟩, ⟨0, 8193, 0⟩, ⟨1, 8193, 2⟩, ⟨1, 8194, 2⟩] 1 0 = 3 := by decide
theorem narrowest_owner_overrides_ordinal :
    rollFrameOwner [⟨0, 8194, 0⟩, ⟨1, 8193, 0⟩, ⟨2, 8193, 2⟩, ⟨3, 8194, 2⟩] 3 1 = 2 := by decide
theorem all_byte_colors_are_available : firstAvailableFrameLabel (List.range 255) = some 255 := by decide
theorem all_byte_colors_exhausted : firstAvailableFrameLabel (List.range 256) = none := by decide

def partitionFixture : List ProgramAtomBody := [
  ⟨0, 1, 0, 0, 0, 0, 0⟩, ⟨0, 1, 0, 0, 0, 0, 1⟩,
  ⟨2, 1, 0, 0, 0, 0, 0⟩, ⟨0, 2, 0, 0, 0, 0, 0⟩,
  ⟨0, 1, 0, 0, 0, 1, 0⟩, ⟨0, 0, 0, 0, 0, 0, 7⟩]
theorem exact_domain_partition_and_self_send :
    (separateRollFrameAtoms [⟨0, 8192, 0⟩, ⟨6, 8192, 2⟩] partitionFixture).map
        ProgramAtomBody.frameLabel = [0, 1, 0, 0, 0, 7] := by decide

#print axioms chosen_color_bound_and_unused
#print axioms color_exhaustion_is_exact
#print axioms chosen_color_separates_every_conflicting_prior
#print axioms original_inequality_is_conflict
#print axioms distinct_owner_is_conflict
#print axioms same_class_does_not_block
#print axioms narrower_roll_owner_wins
#print axioms coextensive_roll_owner_uses_later_source_ordinal
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
