import Std

namespace ParallelLocalFork

structure Candidate where
  left : Bool
  right : Bool
  frontier : Bool
  deriving DecidableEq

def eligible (multiple : Bool) (candidate : Candidate) : Bool :=
  candidate.frontier && (candidate.left || candidate.right) &&
    (!multiple || (candidate.left && candidate.right))

def choose (multiple : Bool) (candidates : List Candidate) : Option Candidate :=
  candidates.find? (eligible multiple)

/-- Looking through unary source groups never removes the frontier check. -/
theorem selected_frontier_is_valid (multiple : Bool) (candidates : List Candidate)
    (candidate : Candidate) (selected : choose multiple candidates = some candidate) :
    candidate.frontier = true := by
  have h := List.find?_some selected
  simp only [eligible, Bool.and_eq_true] at h
  exact h.1.1

/-- Explicit one-fragment borrows retain the old source-group selection. -/
theorem explicit_single_fragment_is_unchanged (candidates : List Candidate) :
    choose false candidates =
      candidates.find? (fun candidate => candidate.frontier && (candidate.left || candidate.right)) := by
  have same : eligible false =
      (fun candidate => candidate.frontier && (candidate.left || candidate.right)) := by
    funext candidate
    simp [eligible]
  simp [choose, same]

/-- Every width above one uses the same candidate search. Width validation
therefore cannot search past a real fork to find a different matching width. -/
theorem multi_fragment_search_is_independent_of_exact_width (n m : Nat)
    (hn : 1 < n) (hm : 1 < m) (candidates : List Candidate) :
    choose (decide (1 < n)) candidates = choose (decide (1 < m)) candidates := by
  simp [hn, hm]

theorem unary_source_groups_are_transparent (left right : Bool)
    (single : left = false ∨ right = false) (tail : List Candidate) :
    choose true (⟨left, right, true⟩ :: tail) = choose true tail := by
  rcases single with h | h
  · simp [choose, eligible, h]
  · simp [choose, eligible, h]

theorem an_unfinished_prefix_cannot_be_skipped (left right : Bool) :
    eligible true ⟨left, right, false⟩ = false := rfl

#print axioms selected_frontier_is_valid
#print axioms explicit_single_fragment_is_unchanged
#print axioms multi_fragment_search_is_independent_of_exact_width
#print axioms unary_source_groups_are_transparent
#print axioms an_unfinished_prefix_cannot_be_skipped
end ParallelLocalFork
