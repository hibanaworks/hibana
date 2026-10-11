import Std

namespace DispatchSubtree

/-- Forward child enumeration, expressed as ancestry from a selected root. -/
inductive Down (edge : Nat → Nat → Prop) (root : Nat) : Nat → Prop where
  | here : Down edge root root
  | step : Down edge root parent → edge parent child → Down edge root child

/-- The original candidate-to-parent walk. -/
inductive Up (edge : Nat → Nat → Prop) : Nat → Nat → Prop where
  | here : Up edge root root
  | step : edge parent child → Up edge parent root → Up edge child root

theorem forward_implies_parent (edge : Nat → Nat → Prop) (root child : Nat)
    (h : Down edge root child) : Up edge child root := by
  induction h with
  | here => exact Up.here
  | step _ e ih => exact Up.step e ih

theorem parent_implies_forward (edge : Nat → Nat → Prop) (root child : Nat)
    (h : Up edge child root) : Down edge root child := by
  induction h with
  | here => exact Down.here
  | step e _ ih => exact Down.step ih e

theorem same_descendants (edge : Nat → Nat → Prop) (root child : Nat) :
    Down edge root child ↔ Up edge child root :=
  ⟨forward_implies_parent edge root child, parent_implies_forward edge root child⟩

theorem certified_ancestry_is_monotone (edge : Nat → Nat → Prop)
    (ordered : ∀ a b, edge a b → a < b)
    (h : Up edge child root) : root ≤ child := by
  induction h with
  | here => exact Nat.le_refl _
  | step e _ ih =>
    have lt := ordered _ _ e
    omega

theorem no_distinct_cycle (edge : Nat → Nat → Prop)
    (ordered : ∀ a b, edge a b → a < b)
    (forward : Up edge child root) (backward : Up edge root child) : root = child := by
  have a := certified_ancestry_is_monotone edge ordered forward
  have b := certified_ancestry_is_monotone edge ordered backward
  omega

/-- Same candidate membership gives the same absent/unique/ambiguous answer.
The two production consumers use set-like OR/UniqueMatch accumulation. -/
def unique (candidate : Nat → Prop) (value : Nat) : Prop :=
  candidate value ∧ ∀ other, candidate other → other = value

theorem same_unique_result (a b : Nat → Prop)
    (same : ∀ x, a x ↔ b x) (value : Nat) : unique a value ↔ unique b value := by
  constructor
  · intro h
    exact ⟨(same value).mp h.1, fun other hb => h.2 other ((same other).mpr hb)⟩
  · intro h
    exact ⟨(same value).mpr h.1, fun other ha => h.2 other ((same other).mp ha)⟩

#print axioms same_descendants
#print axioms certified_ancestry_is_monotone
#print axioms no_distinct_cycle
#print axioms same_unique_result
end DispatchSubtree
