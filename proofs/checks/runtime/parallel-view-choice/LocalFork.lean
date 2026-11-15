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

/-- Rows use the compiler's immutable (logical view, source event, lane) facts.
Removing a source suffix with no row in this view keeps every local operation,
its order, its multiplicity and hence the lane set used by a parallel borrow. -/
theorem foreign_suffix_preserves_local_rows
    (events : List (Nat × Nat × Nat)) (view first cut limit : Nat)
    (bounded : cut ≤ limit)
    (foreign : ∀ row ∈ events, row.1 = view → cut ≤ row.2.1 → row.2.1 < limit → False) :
    events.filter (fun row => decide (row.1 = view ∧ first ≤ row.2.1 ∧ row.2.1 < limit)) =
    events.filter (fun row => decide (row.1 = view ∧ first ≤ row.2.1 ∧ row.2.1 < cut)) := by
  apply List.filter_congr
  intro row member
  apply decide_eq_decide.mpr
  constructor
  · rintro ⟨same, afterFirst, beforeLimit⟩
    refine ⟨same, afterFirst, ?_⟩
    by_cases beforeCut : row.2.1 < cut
    · exact beforeCut
    · exact False.elim (foreign row member same (by omega) beforeLimit)
  · rintro ⟨same, afterFirst, beforeCut⟩
    exact ⟨same, afterFirst, Nat.lt_of_lt_of_le beforeCut bounded⟩

/-- The empty-range fast path returns the same empty local projection without
searching any row. This law holds for arbitrary views and row populations. -/
theorem an_empty_source_range_has_no_local_rows
    (events : List (Nat × Nat × Nat)) (view boundary : Nat) :
    events.filter (fun row => decide (row.1 = view ∧ boundary ≤ row.2.1 ∧ row.2.1 < boundary)) = [] := by
  apply List.filter_eq_nil_iff.mpr
  intro row _
  simp only [Bool.not_eq_true, decide_eq_false_iff_not]
  omega

#print axioms foreign_suffix_preserves_local_rows
#print axioms an_empty_source_range_has_no_local_rows
end ParallelLocalFork
