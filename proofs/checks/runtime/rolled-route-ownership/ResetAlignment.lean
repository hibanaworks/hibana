import Std.Tactic

namespace ResetAlignment
/- Source-linked reset and no-ingress alignment obligations, not a refinement
   proof of arbitrary Rust. A prepared fresh-visit commit owns the reset scope.
   Clearing completion without rewinding that scope's lanes leaves old arm
   progress behind. resetHead describes lane heads within the first row after
   materialization; other lanes in that row retain their heads. Committed bits
   outside the reset scope remain unchanged even when the row cache changes. -/
def materializeResetRows (rows : List Nat) : Option Nat := rows.head?

theorem reset_materializes_the_first_row_once (first : Nat) (rest : List Nat) :
    materializeResetRows (first :: rest) = some first := by rfl

theorem later_lane_cannot_overwrite_the_fresh_prefix :
    materializeResetRows [0, 0, 1, 1] = some 0 := by decide

def resetHead (owned : Bool) (first old : Nat) : Nat :=
  if owned then min first old else old

def advance (head next : Nat) : Nat := max head next

theorem owned_head_rewinds (first old : Nat) (passed : first ≤ old) :
    resetHead true first old = first := by simp [resetHead, Nat.min_eq_left passed]

theorem unowned_head_unchanged (first old : Nat) :
    resetHead false first old = old := by simp [resetHead]

theorem clearing_only_completion_can_skip_a_new_arm : advance 2 1 = 2 := by decide
theorem fresh_prefix_starts_from_reset_head : advance (resetHead true 0 2) 1 = 1 := by decide

def realign (sameScope allowsCurrent : Bool) : Bool := !sameScope && !allowsCurrent

theorem materializable_descendant_is_preserved (sameScope : Bool) :
    realign sameScope true = false := by cases sameScope <;> decide

theorem unrelated_scope_still_realigns : realign false false = true := by decide

/- Finishing the current arm can position its next visit. Starting a later
   route does not authorize an arm remembered from the previous visit. -/
def relocate (atCurrentArmEnd : Bool) (currentArmStart : Nat) : Option Nat :=
  if atCurrentArmEnd then some currentArmStart else none

theorem fresh_prefix_cannot_reuse_next_route_arm (start : Nat) :
    relocate false start = none := by simp [relocate]

theorem current_arm_end_remains_relocatable (start : Nat) :
    relocate true start = some start := by simp [relocate]

def resetEvidence (owned : Bool) (old : Option Nat) : Option Nat :=
  if owned then none else old

theorem cleared_selection_cannot_retain_previous_evidence (old : Option Nat) :
    resetEvidence true old = none := by simp [resetEvidence]

theorem independent_evidence_is_preserved (old : Option Nat) :
    resetEvidence false old = old := by simp [resetEvidence]

inductive ArmView | committed | preview
def authority (history candidate : Option Nat) : ArmView → Option Nat
  | .committed => history
  | .preview => candidate

theorem preview_cannot_change_iteration_history
    (history candidate : Option Nat) :
    authority history candidate .committed = history := by rfl

theorem new_arm_can_differ_from_completed_arm :
    authority (some 1) (some 0) .preview = some 0 ∧
    authority (some 1) (some 0) .committed = some 1 := by decide

#print axioms owned_head_rewinds
#print axioms unowned_head_unchanged
#print axioms fresh_prefix_starts_from_reset_head
#print axioms materializable_descendant_is_preserved
#print axioms fresh_prefix_cannot_reuse_next_route_arm
#print axioms cleared_selection_cannot_retain_previous_evidence
#print axioms preview_cannot_change_iteration_history
#print axioms reset_materializes_the_first_row_once
end ResetAlignment

namespace ParallelVisitCompletion

/- A completion in a parallel child can pay for the containing visit while
   that child's next visit is pending. It cannot pay for its own new work. -/
def rowComplete (done credited independent : Bool) : Bool :=
  done || (credited && independent)

def completeAt (rows : List Nat) (done credited : Nat → Bool)
    (independent : Nat → Nat → Bool) (entry : Nat) : Bool :=
  rows.all fun row => rowComplete (done row) (credited row) (independent row entry)

theorem independent_completed_visits_satisfy_the_parent
    (rows : List Nat) (done credited : Nat → Bool)
    (independent : Nat → Nat → Bool) (entry : Nat)
    (covered : ∀ row ∈ rows, done row = true ∨
      (credited row = true ∧ independent row entry = true)) :
    completeAt rows done credited independent entry = true := by
  apply List.all_eq_true.mpr
  intro row member
  rcases covered row member with finished | ⟨retained, separate⟩
  · simp [rowComplete, finished]
  · simp [rowComplete, retained, separate]

theorem an_entering_child_requires_its_current_completion
    (rows : List Nat) (done credited : Nat → Bool)
    (independent : Nat → Nat → Bool) (entry : Nat)
    (member : entry ∈ rows) (sameChild : independent entry entry = false)
    (complete : completeAt rows done credited independent entry = true) :
    done entry = true := by
  have current := List.all_eq_true.mp complete entry member
  simpa [rowComplete, sameChild] using current

/-- A participating unfinished entry makes the search over all of its
enclosing repeated bodies empty, regardless of retained sibling visits. -/
theorem an_unfinished_entry_blocks_every_enclosing_roll
    (scopes : List Nat) (rows : Nat → List Nat) (done credited : Nat → Bool)
    (independent : Nat → Nat → Bool) (entry : Nat)
    (member : ∀ scope ∈ scopes, entry ∈ rows scope)
    (sameChild : independent entry entry = false) (unfinished : done entry = false) :
    scopes.find? (fun scope => completeAt (rows scope) done credited independent entry) = none := by
  apply List.find?_eq_none.mpr
  intro scope inside
  cases complete : completeAt (rows scope) done credited independent entry with
  | false => simp
  | true =>
      have finished := an_entering_child_requires_its_current_completion
        (rows scope) done credited independent entry (member scope inside) sameChild complete
      simp [unfinished] at finished

theorem an_unfinished_uncredited_sibling_blocks_parent_reentry
    (rows : List Nat) (done credited : Nat → Bool)
    (independent : Nat → Nat → Bool) (entry blocked : Nat)
    (member : blocked ∈ rows) (pending : done blocked = false)
    (uncredited : credited blocked = false) :
    completeAt rows done credited independent entry = false := by
  cases complete : completeAt rows done credited independent entry
  · rfl
  · have current := List.all_eq_true.mp complete blocked member
    simp [rowComplete, pending, uncredited] at current

theorem raw_completion_loses_a_valid_parallel_boundary :
    ([0, 1].all (fun row => row == 0)) = false ∧
    completeAt [0, 1] (fun row => row == 0) (fun row => row == 1)
      (fun row entry => row != entry) 0 = true := by decide

#print axioms independent_completed_visits_satisfy_the_parent
#print axioms an_entering_child_requires_its_current_completion
#print axioms an_unfinished_entry_blocks_every_enclosing_roll
#print axioms an_unfinished_uncredited_sibling_blocks_parent_reentry
#print axioms raw_completion_loses_a_valid_parallel_boundary
end ParallelVisitCompletion
