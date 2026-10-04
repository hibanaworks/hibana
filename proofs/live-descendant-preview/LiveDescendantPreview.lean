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

#print axioms completed_uses_fresh_poll
#print axioms completed_without_poll_waits
#print axioms live_without_poll_preserves_selection
#print axioms live_conflict_rejects
#print axioms live_agreement_preserves_selection
#print axioms chosen_has_live_or_poll_authority
#print axioms old_preview_can_choose_completed_wrong_arm
#print axioms fresh_preview_corrects_completed_wrong_arm

end LiveDescendantPreview
