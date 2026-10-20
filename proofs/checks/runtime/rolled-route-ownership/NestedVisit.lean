import Hibana.GlobalSemantics

namespace NestedVisit
open Hibana

/- The prepared route suffix must start at the visit actually being entered.
Completeness of a containing visit alone does not authorize erasing its prefix.
This models descriptor admission and reset bounds, not arbitrary Rust effects. -/
def atHead (start target : Nat) (live : Nat → Prop) : Prop :=
  ¬ ∃ prior, start ≤ prior ∧ prior < target ∧ live prior

def reset (start finish : Nat) (done : Nat → Bool) (index : Nat) : Bool :=
  if start ≤ index ∧ index < finish then false else done index

theorem a_live_prefix_excludes_parent_reentry (start prior target : Nat)
    (live : Nat → Prop) (within : start ≤ prior) (earlier : prior < target)
    (required : live prior) : ¬ atHead start target live := by
  intro head
  exact head ⟨prior, within, earlier, required⟩

theorem the_actual_head_is_admitted (start : Nat) (live : Nat → Prop) :
    atHead start start live := by
  intro ⟨prior, within, earlier, _⟩
  exact Nat.not_lt_of_ge within earlier

theorem inner_reset_preserves_parent_prefix (start finish index : Nat)
    (done : Nat → Bool) (earlier : index < start) :
    reset start finish done index = done index := by
  simp [reset, Nat.not_le_of_gt earlier]

theorem inner_reset_clears_its_own_visit (start finish index : Nat)
    (done : Nat → Bool) (within : start ≤ index ∧ index < finish) :
    reset start finish done index = false := by
  simp [reset, within]

def admittedArm (fresh : Prop) (selected proposed : Nat) : Prop :=
  fresh ∨ selected = proposed

theorem a_retained_visit_cannot_change_arm (fresh : Prop) (selected proposed : Nat)
    (admitted : admittedArm fresh selected proposed) (retained : ¬ fresh) :
    selected = proposed := by
  rcases admitted with restart | same
  · exact False.elim (retained restart)
  · exact same

def choreography : Choreo := .roll (.route .intrinsic
  (.seq (.send 0 1 128 0) (.seq (.send 1 0 129 0)
    (.roll (.route .intrinsic
      (.seq (.send 0 1 110 0) (.send 1 0 111 0))
      (.seq (.send 0 1 130 0) (.send 1 0 131 0))))))
  (.seq (.send 0 1 134 0) (.send 1 0 135 0)))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest
def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 choreography) trace).isSome
def connected : List GlobalOperation := [.send 0, .recv 0, .send 1, .recv 1]
def retained : List GlobalOperation := [.send 2, .recv 2, .send 3, .recv 3]
def failed : List GlobalOperation := [.send 4, .recv 4, .send 5, .recv 5]
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem initial_failure_ack_is_valid : accepted (connected ++ failed) = true := by decide
theorem inner_reentry_then_failure_ack_is_valid :
    accepted (connected ++ retained ++ [.roll 0] ++ failed) = true := by decide
theorem repeated_inner_reentry_preserves_connection :
    accepted (connected ++ retained ++ [.roll 0] ++ retained ++ [.roll 0] ++ failed) = true := by decide
theorem parent_reset_requires_a_fresh_connection :
    accepted (connected ++ retained ++ [.roll 1, .send 4]) = false := by decide
theorem an_unretained_sample_cannot_switch_arm :
    accepted (connected ++ [.send 2, .recv 2, .roll 0, .send 4]) = false := by decide
theorem duplicate_failure_ack_is_rejected :
    accepted (connected ++ retained ++ [.roll 0] ++ failed ++ [.send 5]) = false := by decide
theorem outer_reentry_accepts_a_new_connection :
    accepted (connected ++ retained ++ [.roll 1] ++ connected ++ failed) = true := by decide

#print axioms a_live_prefix_excludes_parent_reentry
#print axioms the_actual_head_is_admitted
#print axioms inner_reset_preserves_parent_prefix
#print axioms inner_reset_clears_its_own_visit
#print axioms a_retained_visit_cannot_change_arm
#print axioms initial_failure_ack_is_valid
#print axioms inner_reentry_then_failure_ack_is_valid
#print axioms repeated_inner_reentry_preserves_connection
#print axioms parent_reset_requires_a_fresh_connection
#print axioms an_unretained_sample_cannot_switch_arm
#print axioms duplicate_failure_ack_is_rejected
#print axioms outer_reentry_accepts_a_new_connection
end NestedVisit
