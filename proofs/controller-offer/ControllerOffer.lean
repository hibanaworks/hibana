import Std

/-! Source-linked models of controller offer descent and conflict-chain priority.
They prove the mathematical selection rules for arbitrary finite trees/chains.
Production descriptor decoding, affine preview ownership and atomic publication
remain checked by Rust regressions and the existing kernel refinement contracts.
-/
namespace ControllerOffer

inductive Tree where
  | leaf (label : Nat)
  | route (resolver : Nat) (left right : Tree)
  deriving DecidableEq

abbrev Selection := Nat × List (Nat × Bool)

def withDecision (resolver : Nat) (arm : Bool) : Option Selection → Option Selection
  | none => none
  | some (label, path) => some (label, (resolver, arm) :: path)

def select (decide : Nat → Option Bool) : Tree → Option Selection
  | .leaf label => some (label, [])
  | .route resolver left right =>
    match decide resolver with
    | none => none
    | some false => withDecision resolver false (select decide left)
    | some true => withDecision resolver true (select decide right)

def firstLabel : Tree → Nat
  | .leaf label => label
  | .route _ left _ => firstLabel left

theorem leaf_has_no_decisions (decide : Nat → Option Bool) (label : Nat) :
    select decide (.leaf label) = some (label, []) := rfl

theorem rejected_parent_cannot_reach_a_descendant
    (decide : Nat → Option Bool) (resolver : Nat) (left right : Tree)
    (rejected : decide resolver = none) :
    select decide (.route resolver left right) = none := by
  simp [select, rejected]

theorem every_selected_ancestor_has_its_own_decision
    (decide : Nat → Option Bool) (tree : Tree) (label : Nat) (path : List (Nat × Bool))
    (selected : select decide tree = some (label, path)) :
    ∀ resolver arm, (resolver, arm) ∈ path → decide resolver = some arm := by
  induction tree generalizing label path with
  | leaf value =>
    simp [select] at selected
    rcases selected with ⟨_, rfl⟩
    simp
  | route resolver left right leftIH rightIH =>
    cases decision : decide resolver with
    | none => simp [select, decision] at selected
    | some arm =>
      cases arm <;>
        simp only [select, decision] at selected
      · cases child : select decide left with
        | none => simp [child, withDecision] at selected
        | some pair =>
          rcases pair with ⟨childLabel, childPath⟩
          simp only [child, withDecision, Option.some.injEq, Prod.mk.injEq] at selected
          rcases selected with ⟨rfl, rfl⟩
          intro query value membership
          rcases List.mem_cons.mp membership with same | inherited
          · rcases Prod.mk.inj same with ⟨rfl, rfl⟩
            exact decision
          · exact leftIH childLabel childPath child query value inherited
      · cases child : select decide right with
        | none => simp [child, withDecision] at selected
        | some pair =>
          rcases pair with ⟨childLabel, childPath⟩
          simp only [child, withDecision, Option.some.injEq, Prod.mk.injEq] at selected
          rcases selected with ⟨rfl, rfl⟩
          intro query value membership
          rcases List.mem_cons.mp membership with same | inherited
          · rcases Prod.mk.inj same with ⟨rfl, rfl⟩
            exact decision
          · exact rightIH childLabel childPath child query value inherited

structure Membership where
  scope : Nat
  required : Bool
  selected : Option Bool

def Membership.pending (row : Membership) : Bool :=
  match row.selected with
  | none => true
  | some selected => selected != row.required

/-- Resident conflict chains are visited from the event's inner scope outward. -/
def priority (path : List Membership) : Option Nat :=
  path.foldl (fun candidate row => if row.pending then some row.scope else candidate) none

theorem pending_ancestor_precedes_every_descendant
    (descendants : List Membership) (ancestor : Membership)
    (pending : ancestor.pending = true) :
    priority (descendants ++ [ancestor]) = some ancestor.scope := by
  simp [priority, List.foldl_append, pending]

theorem coherent_ancestor_preserves_descendant_priority
    (descendants : List Membership) (ancestor : Membership)
    (coherent : ancestor.pending = false) :
    priority (descendants ++ [ancestor]) = priority descendants := by
  simp [priority, List.foldl_append, coherent]

theorem an_unselected_ancestor_is_pending (scope : Nat) (required : Bool) :
    (Membership.mk scope required none).pending = true := rfl

theorem a_selected_matching_ancestor_is_coherent (scope : Nat) (arm : Bool) :
    (Membership.mk scope arm (some arm)).pending = false := by
  cases arm <;> rfl

/-- Source admission guard: a different pending scope must be at a route entry.
Membership in the old arm alone does not establish that entry. -/
def align (different atEntry : Bool) (current canonical : Nat) : Nat :=
  if different && !atEntry then canonical else current

theorem pending_ancestor_realigns_to_its_entry (current canonical : Nat) :
    align true false current canonical = canonical := rfl

theorem an_actual_entry_keeps_its_cursor (different : Bool) (current canonical : Nat) :
    align different true current canonical = current := by
  cases different <;> rfl

/-- Source offer-refresh guard: only a completed reentry scope that contains
this node supersedes its old nested scope. Completion is descriptor-derived. -/
def enclosingReentry (complete contains : Bool) (node enclosing : Nat) : Nat :=
  if complete && contains then enclosing else node

theorem completed_envelope_owns_the_new_visit (node enclosing : Nat) :
    enclosingReentry true true node enclosing = enclosing := rfl

theorem incomplete_envelope_keeps_its_child (contains : Bool) (node enclosing : Nat) :
    enclosingReentry false contains node enclosing = node := by
  cases contains <;> rfl

def observedTree : Tree := .route 101 (.leaf 1) (.route 102 (.leaf 2) (.leaf 3))
def observedDecision (_ : Nat) : Option Bool := some true

theorem old_first_label_is_an_unchosen_descendant :
    firstLabel (.route 102 (.leaf 2) (.leaf 3)) = 2 := rfl

theorem nested_right_decisions_select_the_actual_leaf :
    select observedDecision observedTree = some (3, [(101, true), (102, true)]) := by
  decide

#print axioms leaf_has_no_decisions
#print axioms rejected_parent_cannot_reach_a_descendant
#print axioms every_selected_ancestor_has_its_own_decision
#print axioms pending_ancestor_precedes_every_descendant
#print axioms coherent_ancestor_preserves_descendant_priority
#print axioms an_unselected_ancestor_is_pending
#print axioms a_selected_matching_ancestor_is_coherent
#print axioms pending_ancestor_realigns_to_its_entry
#print axioms an_actual_entry_keeps_its_cursor
#print axioms completed_envelope_owns_the_new_visit
#print axioms incomplete_envelope_keeps_its_child
#print axioms old_first_label_is_an_unchosen_descendant
#print axioms nested_right_decisions_select_the_actual_leaf
end ControllerOffer
