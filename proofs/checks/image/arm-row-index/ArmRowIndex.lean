import Std

/-!
Route-arm lane spans store cumulative ends. The preceding end is the start of
the next span. Validation checks monotonicity, the 256-lane bound and the exact
terminal bound before the image is published. This model compares direct
lookup with a traversal that accumulates each span's decoded length.
-/
namespace Hibana.ArmRowIndex

inductive Ordered : Nat → List Nat → Prop where
  | nil (start) : Ordered start []
  | cons {start stop tail} : start ≤ stop → Ordered stop tail → Ordered start (stop :: tail)

def accumulatedStart (start : Nat) : List Nat → Nat → Option Nat
  | _, 0 => some start
  | [], _ + 1 => none
  | stop :: tail, n + 1 =>
      if start ≤ stop then accumulatedStart (start + (stop - start)) tail n else none

def indexedStart (start : Nat) (ends : List Nat) : Nat → Option Nat
  | 0 => some start
  | n + 1 => ends[n]?

theorem direct_start_equals_accumulated_prefix {start ends}
    (h : Ordered start ends) (n : Nat) :
    indexedStart start ends n = accumulatedStart start ends n := by
  induction h generalizing n with
  | nil start => cases n <;> simp [indexedStart, accumulatedStart]
  | @cons start stop tail ordered rest ih =>
      have step : start + (stop - start) = stop := by omega
      cases n with
      | zero => rfl
      | succ n =>
          simp only [accumulatedStart, ordered, ↓reduceIte, step]
          rw [← ih]
          cases n <;> simp [indexedStart]

def checkedSpan (start stop total : Nat) : Option (Nat × Nat) :=
  if start ≤ stop ∧ stop ≤ total ∧ stop - start ≤ 256 then
    some (start, stop - start)
  else none

theorem accepted_span_has_exact_bounds {start stop total range}
    (h : checkedSpan start stop total = some range) :
    range.1 = start ∧ range.1 + range.2 = stop ∧ range.2 ≤ 256 ∧ stop ≤ total := by
  simp only [checkedSpan] at h
  split at h
  · rename_i valid
    simp only [Option.some.injEq] at h
    subst range
    exact ⟨rfl, by omega, valid.2.2, valid.2.1⟩
  · cases h

theorem decreasing_end_is_rejected {start stop total} (h : stop < start) :
    checkedSpan start stop total = none := by
  simp [checkedSpan, show ¬ start ≤ stop by omega]

theorem outside_storage_is_rejected {start stop total} (h : total < stop) :
    checkedSpan start stop total = none := by
  simp [checkedSpan, show ¬ stop ≤ total by omega]

theorem too_many_lanes_is_rejected {start stop total} (h : 256 < stop - start) :
    checkedSpan start stop total = none := by
  simp [checkedSpan, show ¬ stop - start ≤ 256 by omega]

theorem repeated_end_preserves_empty_arm (start total : Nat) (h : start ≤ total) :
    checkedSpan start start total = some (start, 0) := by
  simp [checkedSpan, h]

theorem direct_start_uses_previous_row : indexedStart 0 [5, 8, 8, 264] 3 = some 8 := by decide
theorem full_lane_domain_is_accepted : checkedSpan 8 264 264 = some (8, 256) := by decide

#print axioms direct_start_equals_accumulated_prefix
#print axioms accepted_span_has_exact_bounds
#print axioms decreasing_end_is_rejected
#print axioms outside_storage_is_rejected
#print axioms too_many_lanes_is_rejected
#print axioms repeated_end_preserves_empty_arm
#print axioms direct_start_uses_previous_row
#print axioms full_lane_domain_is_accepted

end Hibana.ArmRowIndex
