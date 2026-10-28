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

/-- A node denotes one route arm. Edges enter either arm of a passive child. -/
structure Interval where
  start : Nat
  stop : Nat

def inside (row : Interval) (target : Nat) : Prop :=
  row.start ≤ target ∧ target < row.stop

def contains (outer inner : Interval) : Prop :=
  outer.start ≤ inner.start ∧ inner.stop ≤ outer.stop

/-- The constructor checks the first receive against the whole arm range,
including rows belonging to nested routes. -/
def FirstReceive (row : Interval) (receives : Nat → Prop) : Option Nat → Prop
  | none => ∀ target, inside row target → ¬ receives target
  | some first => inside row first ∧ receives first ∧
      ∀ target, inside row target → receives target → first ≤ target

theorem descendant_interval_contained
    (edge : Nat → Nat → Prop) (rows : Nat → Interval)
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (reachable : Down edge root child) : contains (rows root) (rows child) := by
  induction reachable with
  | here => exact ⟨Nat.le_refl _, Nat.le_refl _⟩
  | step _ atChild ih =>
    have one := nested _ _ atChild
    exact ⟨Nat.le_trans ih.1 one.1, Nat.le_trans one.2 ih.2⟩

theorem first_receive_present_iff
    (row : Interval) (receives : Nat → Prop) (first : Option Nat)
    (checked : FirstReceive row receives first) :
    first.isSome = true ↔ ∃ target, inside row target ∧ receives target := by
  cases first with
  | none =>
    constructor
    · intro impossible
      cases impossible
    · rintro ⟨target, bounded, receive⟩
      exact False.elim (checked target bounded receive)
  | some target =>
    exact ⟨fun _ => ⟨target, checked.1, checked.2.1⟩, fun _ => rfl⟩

def DispatchedTarget (edge : Nat → Nat → Prop) (root : Nat)
    (first : Nat → Option Nat) (target : Nat) : Prop :=
  ∃ node, Down edge root node ∧ first node = some target

theorem dispatched_target_inside_root
    (edge : Nat → Nat → Prop) (rows : Nat → Interval)
    (receives : Nat → Prop) (first : Nat → Option Nat)
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (checked : ∀ node, FirstReceive (rows node) receives (first node))
    (dispatched : DispatchedTarget edge root first target) :
    inside (rows root) target ∧ receives target := by
  obtain ⟨node, reachable, found⟩ := dispatched
  have interval := descendant_interval_contained edge rows nested reachable
  have rowChecked := checked node
  rw [found] at rowChecked
  exact ⟨⟨Nat.le_trans interval.1 rowChecked.1.1,
    Nat.lt_of_lt_of_le rowChecked.1.2 interval.2⟩, rowChecked.2.1⟩

/-- Descendants cannot add an arm bit: their receives already occur in the
root's arm range, and the subtree visitor includes the root entry itself. -/
theorem subtree_mask_bit_exact
    (edge : Nat → Nat → Prop) (rows : Nat → Interval)
    (receives : Nat → Prop) (first : Nat → Option Nat)
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (checked : ∀ node, FirstReceive (rows node) receives (first node)) (root : Nat) :
    (∃ target, DispatchedTarget edge root first target) ↔ (first root).isSome = true := by
  constructor
  · rintro ⟨target, dispatched⟩
    exact (first_receive_present_iff (rows root) receives (first root) (checked root)).mpr
      ⟨target, dispatched_target_inside_root edge rows receives first nested checked dispatched⟩
  · intro present
    obtain ⟨target, found⟩ := Option.isSome_iff_exists.mp present
    exact ⟨target, root, .here, found⟩

def binaryMask (left right : Bool) : Nat :=
  (if left then 1 else 0) + (if right then 2 else 0)

/-- Retain only nodes whose source interval contains the received event. -/
inductive OnTarget (edge : Nat → Nat → Prop) (rows : Nat → Interval)
    (target root : Nat) : Nat → Prop where
  | here : inside (rows root) target → OnTarget edge rows target root root
  | step : OnTarget edge rows target root parent → edge parent child →
      inside (rows child) target → OnTarget edge rows target root child

theorem target_path_is_reachable (path : OnTarget edge rows target root child) :
    Down edge root child := by
  induction path with
  | here => exact .here
  | step _ edge _ ih => exact .step ih edge

theorem every_containing_path_is_retained
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (path : Down edge root child) :
    inside (rows child) target → OnTarget edge rows target root child := by
  induction path with
  | here => exact OnTarget.here
  | step _ edge ih =>
      intro bounded
      have enclosing := nested _ _ edge
      have parentBounded : inside (rows _) target :=
        ⟨Nat.le_trans enclosing.1 bounded.1,
          Nat.lt_of_lt_of_le bounded.2 enclosing.2⟩
      exact .step (ih parentBounded) edge bounded

theorem exact_target_dispatch_is_preserved
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (checked : ∀ node, FirstReceive (rows node) receives (first node)) :
    DispatchedTarget edge root first target ↔
      ∃ node, OnTarget edge rows target root node ∧ first node = some target := by
  constructor
  · rintro ⟨node, path, found⟩
    have rowChecked := checked node
    rw [found] at rowChecked
    exact ⟨node, every_containing_path_is_retained nested path rowChecked.1, found⟩
  · rintro ⟨node, path, found⟩
    exact ⟨node, target_path_is_reachable path, found⟩

theorem disjoint_arms_have_at_most_one_target
    (left : inside ⟨first, split⟩ target) : ¬ inside ⟨split, finish⟩ target := by
  intro right
  exact Nat.not_lt_of_ge right.1 left.2

private theorem bool_eq_of_true_iff (left right : Bool)
    (same : (left = true) ↔ (right = true)) : left = right := by
  cases left <;> cases right <;> simp_all

theorem binary_mask_exact
    (edge : Nat → Nat → Prop) (rows : Nat → Interval)
    (receives : Nat → Prop) (first : Nat → Option Nat)
    (nested : ∀ parent child, edge parent child → contains (rows parent) (rows child))
    (checked : ∀ node, FirstReceive (rows node) receives (first node))
    (leftRoot rightRoot : Nat) (left right : Bool)
    (leftScan : left = true ↔ ∃ target, DispatchedTarget edge leftRoot first target)
    (rightScan : right = true ↔ ∃ target, DispatchedTarget edge rightRoot first target) :
    binaryMask left right = binaryMask (first leftRoot).isSome (first rightRoot).isSome := by
  have leftSame := bool_eq_of_true_iff left (first leftRoot).isSome
    (leftScan.trans (subtree_mask_bit_exact edge rows receives first nested checked leftRoot))
  have rightSame := bool_eq_of_true_iff right (first rightRoot).isSome
    (rightScan.trans (subtree_mask_bit_exact edge rows receives first nested checked rightRoot))
  rw [leftSame, rightSame]

private def outsideRows (node : Nat) : Interval :=
  if node = 1 then ⟨1, 2⟩ else ⟨0, 0⟩

private def outsideFirst (node : Nat) : Option Nat :=
  if node = 1 then some 1 else none

private def outsideEdge (parent child : Nat) : Prop := parent = 0 ∧ child = 1

private theorem outside_first_checked (node : Nat) :
    FirstReceive (outsideRows node) (· = 1) (outsideFirst node) := by
  by_cases selected : node = 1
  · subst node
    change (1 ≤ 1 ∧ 1 < 2) ∧ 1 = 1 ∧
      ∀ target, (1 ≤ target ∧ target < 2) → target = 1 → 1 ≤ target
    exact ⟨⟨by decide, by decide⟩, rfl, fun _ bounded _ => bounded.1⟩
  · simp only [outsideRows, outsideFirst, if_neg selected, FirstReceive]
    intro target bounded _
    exact Nat.not_lt_zero target bounded.2

/-- Exact first-receive entries alone do not justify the shortcut when a
child's interval can escape its parent. Only containment is missing here. -/
theorem missing_containment_changes_mask_bit :
    (∀ node, FirstReceive (outsideRows node) (· = 1) (outsideFirst node)) ∧
    (∃ target, DispatchedTarget outsideEdge 0 outsideFirst target) ∧
    (outsideFirst 0).isSome = false ∧ ¬ contains (outsideRows 0) (outsideRows 1) := by
  refine ⟨outside_first_checked, ⟨1, 1, .step .here ⟨rfl, rfl⟩, rfl⟩, rfl, ?_⟩
  change ¬ (0 ≤ 1 ∧ 2 ≤ 0)
  decide

#print axioms same_descendants
#print axioms certified_ancestry_is_monotone
#print axioms no_distinct_cycle
#print axioms same_unique_result
#print axioms descendant_interval_contained
#print axioms first_receive_present_iff
#print axioms dispatched_target_inside_root
#print axioms subtree_mask_bit_exact
#print axioms binary_mask_exact
#print axioms missing_containment_changes_mask_bit
#print axioms target_path_is_reachable
#print axioms every_containing_path_is_retained
#print axioms exact_target_dispatch_is_preserved
#print axioms disjoint_arms_have_at_most_one_target
end DispatchSubtree
