import Hibana.GlobalSemantics

namespace SharedSuffix
open Hibana

/- Mirrors the committed/preview distinction in roll_body_reentry_scope_for_step.
The completed visit is checked before its candidate conflict is used at the
new head. This is an admission/reset model, not a universal Rust refinement. -/
def previewChoice (committed candidate : Option Bool) (completed : Bool) : Option Bool :=
  match committed, candidate with
  | some old, some next => if completed && (old != next) then none else some old
  | old, _ => old

def headChoice (old next completed : Bool) : Bool :=
  (previewChoice (some old) (some next) completed).getD next

theorem completed_visit_uses_the_candidate (old next : Bool) :
    headChoice old next true = next := by
  cases old <;> cases next <;> rfl

theorem incomplete_visit_keeps_its_selection (old next : Bool) :
    previewChoice (some old) (some next) false = some old := by
  cases old <;> cases next <;> rfl

theorem missing_candidate_cannot_erase_history (old : Option Bool) (completed : Bool) :
    previewChoice old none completed = old := by
  cases old <;> rfl

/- A completed enclosing body uses Head admission, even if its lane cursor is
parked at a completed suffix. Progress in the previous visit is not a prefix
of the candidate visit. -/
def atHead (start target : Nat) (live : Nat → Prop) : Prop :=
  ¬ ∃ prior, start ≤ prior ∧ prior < target ∧ live prior

theorem a_required_prefix_rejects_suffix_reentry (start prior target : Nat)
    (live : Nat → Prop) (within : start ≤ prior) (before : prior < target)
    (required : live prior) : ¬ atHead start target live := by
  intro head
  exact head ⟨prior, within, before, required⟩

theorem the_actual_head_is_admitted (start : Nat) (live : Nat → Prop) :
    atHead start start live := by
  intro ⟨prior, within, before, _⟩
  exact Nat.not_lt_of_ge within before

def reset (start finish : Nat) (done : Nat → Bool) (index : Nat) : Bool :=
  if start ≤ index ∧ index < finish then false else done index

theorem reset_clears_the_shared_reply (start finish reply : Nat) (done : Nat → Bool)
    (within : start ≤ reply ∧ reply < finish) :
    reset start finish done reply = false := by
  simp [reset, within]

theorem reset_preserves_the_parallel_lane (start finish index : Nat) (done : Nat → Bool)
    (outside : ¬ (start ≤ index ∧ index < finish)) :
    reset start finish done index = done index := by
  simp [reset, outside]

def choreography : Choreo := .par
  (.roll (.seq
    (.route .intrinsic (.send 0 1 112 0) (.send 0 1 136 0))
    (.route .intrinsic (.send 1 0 113 0) (.send 1 0 114 0))))
  (.par
    (.roll (.seq
      (.route .intrinsic (.send 0 1 117 1) (.send 0 1 137 1))
      (.route .intrinsic (.send 1 0 118 1) (.send 1 0 119 1))))
    (.seq (.send 1 2 120 2) (.send 2 1 121 2)))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 choreography) trace).isSome

def queryARecord : List GlobalOperation := [.send 0, .recv 0, .send 2, .recv 2]
def queryBRecord : List GlobalOperation := [.send 1, .recv 1, .send 2, .recv 2]
def queryBEmpty : List GlobalOperation := [.send 1, .recv 1, .send 3, .recv 3]
def releaseA : List GlobalOperation := [.send 4, .recv 4, .send 6, .recv 6]
def releaseB : List GlobalOperation := [.send 5, .recv 5, .send 7, .recv 7]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem a_to_b_shared_reply_is_valid :
    accepted (queryARecord ++ [.roll 0] ++ queryBRecord) = true := by decide
theorem b_to_a_shared_reply_is_valid :
    accepted (queryBRecord ++ [.roll 0] ++ queryARecord) = true := by decide
theorem the_original_parallel_release_history_is_valid :
    accepted (queryBEmpty ++ [.roll 0] ++ queryARecord ++ [.roll 0] ++ queryARecord ++
      releaseB ++ [.roll 1] ++ releaseA ++ [.roll 0] ++ queryBRecord) = true := by decide
theorem a_duplicate_reply_is_rejected :
    accepted (queryARecord ++ [.send 2]) = false := by decide
theorem a_new_visit_requires_a_fresh_query :
    accepted (queryARecord ++ [.roll 0, .send 2]) = false := by decide
theorem an_incomplete_visit_cannot_restart :
    accepted [.send 0, .recv 0, .roll 0] = false := by decide
theorem a_parallel_release_does_not_complete_the_query :
    accepted ([.send 0, .recv 0] ++ releaseB ++ [.roll 0]) = false := by decide

#print axioms completed_visit_uses_the_candidate
#print axioms incomplete_visit_keeps_its_selection
#print axioms missing_candidate_cannot_erase_history
#print axioms a_required_prefix_rejects_suffix_reentry
#print axioms the_actual_head_is_admitted
#print axioms reset_clears_the_shared_reply
#print axioms reset_preserves_the_parallel_lane
#print axioms a_to_b_shared_reply_is_valid
#print axioms b_to_a_shared_reply_is_valid
#print axioms the_original_parallel_release_history_is_valid
#print axioms a_duplicate_reply_is_rejected
#print axioms a_new_visit_requires_a_fresh_query
#print axioms an_incomplete_visit_cannot_restart
#print axioms a_parallel_release_does_not_complete_the_query
end SharedSuffix
