namespace LiveDescendantPreview

inductive Preview where
  | waiting
  | chosen (arm : Bool)
  | rejected
  deriving DecidableEq

-- A completed selection belongs to history. A live conflicting selection
-- remains a contract violation; it is never silently replaced by a poll.
def preview : Option Bool → Bool → Option Bool → Preview
  | some selected, false, some ready =>
      if selected = ready then .chosen selected else .rejected
  | some selected, false, none => .chosen selected
  | _, _, some ready => .chosen ready
  | _, _, none => .waiting

def oldPreview : Option Bool → Option Bool → Preview
  | some selected, _ => .chosen selected
  | none, some ready => .chosen ready
  | none, none => .waiting

theorem completed_uses_fresh_poll (selected : Option Bool) (ready : Bool) :
    preview selected true (some ready) = .chosen ready := by
  cases selected <;> rfl

theorem completed_without_poll_waits (selected : Option Bool) :
    preview selected true none = .waiting := by
  cases selected <;> rfl

theorem live_without_poll_preserves_selection (selected : Bool) :
    preview (some selected) false none = .chosen selected := rfl

theorem live_conflict_rejects (selected ready : Bool) (different : selected ≠ ready) :
    preview (some selected) false (some ready) = .rejected := by
  simp [preview, different]

theorem live_agreement_preserves_selection (selected : Bool) :
    preview (some selected) false (some selected) = .chosen selected := by
  simp [preview]

theorem chosen_has_live_or_poll_authority
    (selected : Option Bool) (completed : Bool) (ready : Option Bool) (arm : Bool)
    (chosen : preview selected completed ready = .chosen arm) :
    (selected = some arm ∧ completed = false) ∨ ready = some arm := by
  cases selected with
  | none => cases completed <;> cases ready <;> simp_all [preview]
  | some prior =>
      cases prior <;> cases completed <;> cases ready with
      | none => cases arm <;> simp_all [preview]
      | some fresh => cases fresh <;> cases arm <;> simp_all [preview]

theorem old_preview_can_choose_completed_wrong_arm :
    oldPreview (some false) (some true) = .chosen false := rfl

theorem fresh_preview_corrects_completed_wrong_arm :
    preview (some false) true (some true) = .chosen true := rfl

-- Candidate lookup uses the proposed row's arm. It may forget a completed
-- rolled selection, but cannot choose a branch or alter its committed history.
def candidateSelection (selected : Option Bool) (candidate reentrant completed : Bool) :
    Option Bool :=
  match selected with
  | some prior => if prior != candidate && reentrant && completed then none else some prior
  | none => none

def candidateVisible (selected : Option Bool) (candidate reentrant completed : Bool) : Bool :=
  match candidateSelection selected candidate reentrant completed with
  | none => true
  | some prior => prior == candidate

theorem unfinished_candidate_keeps_selection (selected : Option Bool) (candidate reentrant : Bool) :
    candidateSelection selected candidate reentrant false = selected := by
  cases selected <;> simp [candidateSelection]

theorem ordinary_candidate_keeps_selection (selected : Option Bool) (candidate completed : Bool) :
    candidateSelection selected candidate false completed = selected := by
  cases selected <;> simp [candidateSelection]

theorem completed_roll_alternative_remains_visible (prior candidate : Bool) :
    candidateVisible (some prior) candidate true true = true := by
  cases prior <;> cases candidate <;> rfl

theorem conflicting_live_candidate_stays_excluded (prior candidate reentrant : Bool)
    (different : prior ≠ candidate) :
    candidateVisible (some prior) candidate reentrant false = false := by
  cases prior <;> cases candidate <;> cases reentrant <;> simp_all [candidateVisible, candidateSelection]

theorem candidate_visibility_is_exact
    (selected : Option Bool) (candidate reentrant completed : Bool) :
    candidateVisible selected candidate reentrant completed = true ↔
      selected = none ∨ selected = some candidate ∨ (reentrant = true ∧ completed = true) := by
  cases selected with
  | none => simp [candidateVisible, candidateSelection]
  | some prior =>
    cases prior <;> cases candidate <;> cases reentrant <;> cases completed <;>
      simp [candidateVisible, candidateSelection]

theorem stale_history_can_hide_a_completed_roll_alternative :
    (some false : Option Bool) ≠ some true ∧
      candidateVisible (some false) true true true = true := by decide

#print axioms completed_uses_fresh_poll
#print axioms completed_without_poll_waits
#print axioms live_without_poll_preserves_selection
#print axioms live_conflict_rejects
#print axioms live_agreement_preserves_selection
#print axioms chosen_has_live_or_poll_authority
#print axioms old_preview_can_choose_completed_wrong_arm
#print axioms fresh_preview_corrects_completed_wrong_arm
#print axioms unfinished_candidate_keeps_selection
#print axioms ordinary_candidate_keeps_selection
#print axioms completed_roll_alternative_remains_visible
#print axioms conflicting_live_candidate_stays_excluded
#print axioms candidate_visibility_is_exact
#print axioms stale_history_can_hide_a_completed_roll_alternative

end LiveDescendantPreview
