namespace NestedResolverReentry

-- Facts come from the projected route rows and current committed visit.
-- Completion alone does not establish that a candidate enters that visit.
def fresh (selected : Option Bool) (completed ownEntry inherited : Bool) : Bool :=
  inherited || match selected with
    | none => true
    | some _ => completed && ownEntry

def decide (selected isFresh callback : Bool) : Bool :=
  if isFresh then callback else selected

def oldFresh : Option Bool → Bool → Bool
  | none, _ => true
  | some _, completed => completed

theorem child_entry_retains_parent (selected completed : Bool) :
    fresh (some selected) completed false false = false := by
  cases completed <;> rfl

theorem no_selection_is_fresh (completed ownEntry : Bool) :
    fresh none completed ownEntry false = true := rfl

theorem completed_own_entry_is_fresh (selected : Bool) :
    fresh (some selected) true true false = true := rfl

theorem containing_new_visit_owns_descendant (selected : Option Bool)
    (completed ownEntry : Bool) : fresh selected completed ownEntry true = true := by
  cases selected <;> rfl

theorem retained_parent_ignores_changed_callback (selected completed callback : Bool) :
    decide selected (fresh (some selected) completed false false) callback = selected := by
  cases completed <;> rfl

theorem completion_alone_misclassifies_child_entry :
    oldFresh (some false) true = true ∧ fresh (some false) true false false = false := by
  exact ⟨rfl, rfl⟩

#print axioms child_entry_retains_parent
#print axioms no_selection_is_fresh
#print axioms completed_own_entry_is_fresh
#print axioms containing_new_visit_owns_descendant
#print axioms retained_parent_ignores_changed_callback
#print axioms completion_alone_misclassifies_child_entry
end NestedResolverReentry
