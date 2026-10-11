import Hibana.GlobalSemantics

namespace SendEntry
open Hibana

/- Intrinsic choice uses the actual controller arm entries. Membership of an
arbitrary later send in an arm does not grant authority to choose that arm.
This is a source-linked selector abstraction, not arbitrary Rust refinement. -/
structure Entry where
  index : Nat
  label : Nat
  schema : Nat
  deriving DecidableEq

abbrev accepts (entry : Entry) (label schema : Nat) : Prop :=
  entry.label = label ∧ entry.schema = schema

def select (left right : Entry) (label schema : Nat) : Option Entry :=
  if accepts left label schema then some left
  else if accepts right label schema then some right else none

theorem selected_is_an_actual_entry (left right found : Entry) (label schema : Nat)
    (selected : select left right label schema = some found) :
    found = left ∨ found = right := by
  unfold select at selected
  split at selected
  · simp_all
  · split at selected <;> simp_all

theorem an_interior_occurrence_cannot_be_selected (left right later : Entry) (label schema : Nat)
    (notLeft : later ≠ left) (notRight : later ≠ right) :
    select left right label schema ≠ some later := by
  intro selected
  rcases selected_is_an_actual_entry left right later label schema selected with same | same
  · exact notLeft same
  · exact notRight same

theorem matching_right_entry_is_selected (left right : Entry) (label schema : Nat)
    (leftDoesNotMatch : ¬ accepts left label schema) (rightMatches : accepts right label schema) :
    select left right label schema = some right := by
  simp [select, leftDoesNotMatch, rightMatches]

def choreography : Choreo := .roll (.route .intrinsic
  (.seq (.send 0 1 90 0) (.route .intrinsic
    (.seq (.send 1 0 91 0) (.send 0 1 92 0))
    (.seq (.send 1 0 93 0) (.seq (.send 0 1 94 0) (.send 1 0 95 0)))))
  (.seq (.send 0 1 94 0) (.send 1 0 95 0)))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest
def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 choreography) trace).isSome
def read : List GlobalOperation := [.send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2]
def normalReturn : List GlobalOperation := [.send 6, .recv 6, .send 7, .recv 7]
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem initial_return_is_valid : accepted normalReturn = true := by decide
theorem completed_read_then_return_is_valid : accepted (read ++ [.roll 0] ++ normalReturn) = true := by decide
theorem unchosen_inner_return_is_invalid : accepted (read ++ [.roll 0, .send 4]) = false := by decide
theorem failure_requires_the_inner_return :
    accepted [.send 0, .recv 0, .send 3, .recv 3, .send 4, .recv 4, .send 5, .recv 5] = true := by decide
theorem an_unfinished_read_cannot_return : accepted [.send 0, .recv 0, .send 6] = false := by decide
theorem reentry_allows_another_read_then_return :
    accepted (read ++ [.roll 0] ++ read ++ [.roll 0] ++ normalReturn) = true := by decide

#print axioms selected_is_an_actual_entry
#print axioms an_interior_occurrence_cannot_be_selected
#print axioms matching_right_entry_is_selected
#print axioms initial_return_is_valid
#print axioms completed_read_then_return_is_valid
#print axioms unchosen_inner_return_is_invalid
#print axioms failure_requires_the_inner_return
#print axioms an_unfinished_read_cannot_return
#print axioms reentry_allows_another_read_then_return
end SendEntry
