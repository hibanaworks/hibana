import Std

namespace Hibana.RouteAncestry

def arm (first split boundary start stop : Nat) : Option Nat :=
  if first ≤ start ∧ stop ≤ split then some 0
  else if split ≤ start ∧ stop ≤ boundary then some 1
  else none

theorem left_exact (first split boundary start stop : Nat) :
    arm first split boundary start stop = some 0 ↔ first ≤ start ∧ stop ≤ split := by
  unfold arm
  split <;> simp_all

theorem right_exact (first split boundary start stop : Nat) (nonempty : start < stop) :
    arm first split boundary start stop = some 1 ↔ split ≤ start ∧ stop ≤ boundary := by
  unfold arm
  split
  · rename_i left
    have impossible : ¬ (split ≤ start ∧ stop ≤ boundary) := by omega
    simp [impossible]
  · split <;> simp_all

-- Following any number of parent links preserves interval inclusion.
inductive Nested : Nat → Nat → Nat → Nat → Prop where
  | same (start stop : Nat) : Nested start stop start stop
  | step {start stop innerFirst innerStop outerFirst outerStop : Nat}
      (inner : Nested start stop innerFirst innerStop)
      (lower : outerFirst ≤ innerFirst) (upper : innerStop ≤ outerStop) :
      Nested start stop outerFirst outerStop

theorem nested_bounds {start stop first boundary : Nat}
    (nested : Nested start stop first boundary) : first ≤ start ∧ stop ≤ boundary := by
  induction nested with
  | same => omega
  | step inner lower upper ih => omega

theorem preserves_left {first split boundary innerFirst innerStop start stop : Nat}
    (nested : Nested start stop innerFirst innerStop)
    (left : first ≤ innerFirst ∧ innerStop ≤ split) :
    arm first split boundary start stop = some 0 := by
  have := nested_bounds nested
  apply (left_exact first split boundary start stop).mpr
  omega

theorem preserves_right {first split boundary innerFirst innerStop start stop : Nat}
    (nested : Nested start stop innerFirst innerStop) (nonempty : start < stop)
    (right : split ≤ innerFirst ∧ innerStop ≤ boundary) :
    arm first split boundary start stop = some 1 := by
  have := nested_bounds nested
  apply (right_exact first split boundary start stop nonempty).mpr
  omega

theorem self_is_not_a_strict_ancestor (first split boundary : Nat)
    (left : first < split) (right : split < boundary) :
    arm first split boundary first boundary = none := by
  simp [arm, show ¬ boundary ≤ split by omega, show ¬ split ≤ first by omega]

theorem disjoint_is_not_an_ancestor (first split boundary start stop : Nat)
    (valid : first < split ∧ split < boundary) (nonempty : start < stop)
    (disjoint : stop ≤ first ∨ boundary ≤ start) :
    arm first split boundary start stop = none := by
  have left : ¬ (first ≤ start ∧ stop ≤ split) := by omega
  have right : ¬ (split ≤ start ∧ stop ≤ boundary) := by omega
  simp [arm, left, right]

end Hibana.RouteAncestry
