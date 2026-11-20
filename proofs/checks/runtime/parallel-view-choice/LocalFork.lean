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

/-- Ignoring completed rows of an earlier rolled prefix during fork selection
preserves every unfinished row. This is a read-only frontier law; the actual
operation still needs its predecessor admission, tested at the Endpoint cut. -/
theorem ignoring_completed_prefix_rows_preserves_unfinished_rows
    (rows : List (Nat × Bool)) (first stop forward : Nat) :
    ((rows.filter fun row =>
        !(decide (first ≤ row.1 ∧ row.1 < stop ∧ stop ≤ forward) && row.2)).filter
      fun row => !row.2) = rows.filter (fun row => !row.2) := by
  simp only [List.filter_filter]
  apply List.filter_congr
  intro row _
  cases row.2 <;> simp

#print axioms ignoring_completed_prefix_rows_preserves_unfinished_rows

/-- Source ranges, rather than the previous visit's selected arm, determine
which arm contains a proposed parallel group. -/
def armAt (first split finish event : Nat) : Option Bool :=
  if first ≤ event ∧ event < split then some false
  else if split ≤ event ∧ event < finish then some true
  else none

theorem equal_endpoint_arms_cover_every_group_event
    (first split finish start stop event : Nat) (arm : Bool)
    (inside : start ≤ event ∧ event < stop)
    (head : armAt first split finish start = some arm)
    (last : armAt first split finish (stop - 1) = some arm) :
    armAt first split finish event = some arm := by
  cases arm with
  | false =>
    have lower : first ≤ start ∧ start < split := by
      by_cases left : first ≤ start ∧ start < split
      · exact left
      · by_cases right : split ≤ start ∧ start < finish <;>
          simp [armAt, left, right] at head
    have upper : first ≤ stop - 1 ∧ stop - 1 < split := by
      by_cases left : first ≤ stop - 1 ∧ stop - 1 < split
      · exact left
      · by_cases right : split ≤ stop - 1 ∧ stop - 1 < finish <;>
          simp [armAt, left, right] at last
    have bounds : first ≤ event ∧ event < split := by omega
    simp [armAt, bounds]
  | true =>
    have lower : split ≤ start ∧ start < finish := by
      by_cases left : first ≤ start ∧ start < split
      · simp [armAt, left] at head
      · by_cases right : split ≤ start ∧ start < finish
        · exact right
        · simp [armAt, left, right] at head
    have upper : split ≤ stop - 1 ∧ stop - 1 < finish := by
      by_cases left : first ≤ stop - 1 ∧ stop - 1 < split
      · simp [armAt, left] at last
      · by_cases right : split ≤ stop - 1 ∧ stop - 1 < finish
        · exact right
        · simp [armAt, left, right] at last
    have notLeft : ¬ (first ≤ event ∧ event < split) := by omega
    have bounds : split ≤ event ∧ event < finish := by omega
    simp [armAt, notLeft, bounds]

/-- Filtering by an already materialized input must precede choosing a group.
The chosen group still satisfies its ordinary frontier admission. -/
theorem a_selected_group_retains_its_materialized_input
    (multiple : Bool) (candidates : List Candidate) (keepsInput : Candidate → Bool)
    (candidate : Candidate)
    (selected : choose multiple (candidates.filter keepsInput) = some candidate) :
    keepsInput candidate = true ∧ candidate.frontier = true := by
  have member : candidate ∈ candidates.filter keepsInput :=
    List.mem_of_find?_eq_some selected
  exact ⟨(List.mem_filter.mp member).2,
    selected_frontier_is_valid multiple (candidates.filter keepsInput) candidate selected⟩

#print axioms equal_endpoint_arms_cover_every_group_event
#print axioms a_selected_group_retains_its_materialized_input
end ParallelLocalFork
