import Hibana.GlobalSemantics

namespace RollContinuationDependency
open Hibana

/- An event's conflict says whether it belongs to this visit. A dependency's
conflict says whether a preceding parallel region must finish first. Those are
separate facts. These lemmas model that distinction, not all Rust execution. -/
def prerequisite (active complete : Bool) : Bool := !active || complete
def committedContinuation (sameLane later outsideChild done ownConflict : Bool) : Bool :=
  sameLane && later && outsideChild && done && ownConflict

theorem inactive_prerequisite_is_satisfied (complete : Bool) :
    prerequisite false complete = true := by cases complete <;> decide

theorem active_prerequisite_requires_completion (complete : Bool) :
    prerequisite true complete = complete := by cases complete <;> decide

theorem inactive_prerequisite_does_not_erase_a_committed_event :
    prerequisite false false && committedContinuation true true true true true = true := by decide

theorem incompatible_event_is_excluded (sameLane later outsideChild done : Bool) :
    committedContinuation sameLane later outsideChild done false = false := by
  cases sameLane <;> cases later <;> cases outsideChild <;> cases done <;> decide

theorem unfinished_event_is_not_completion (sameLane later outsideChild ownConflict : Bool) :
    committedContinuation sameLane later outsideChild false ownConflict = false := by
  cases sameLane <;> cases later <;> cases outsideChild <;> cases ownConflict <;> decide

def choreography : Choreo := .roll (.seq
  (.roll (.seq (.send 0 1 1 0) (.send 1 0 2 0)))
  (.route .intrinsic
    (.seq (.send 2 0 3 0) (.seq (.send 0 1 4 0)
      (.par (.roll (.send 0 1 5 0)) (.roll (.send 2 1 6 1)))))
    (.seq (.send 2 0 7 0) (.send 0 1 8 0))))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 choreography) trace).isSome

def visit : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .send 6, .recv 6, .send 7, .recv 7]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem unchosen_parallel_rolls_do_not_block_a_visit : accepted visit = true := by decide

theorem outer_reentry_allows_a_fresh_visit :
    accepted (visit ++ [.roll 3] ++ visit) = true := by decide

theorem an_incomplete_visit_cannot_restart :
    accepted [.send 0, .recv 0, .roll 3] = false := by decide

theorem reentry_cannot_skip_the_fresh_request :
    accepted (visit ++ [.roll 3, .send 7]) = false := by decide

#print axioms inactive_prerequisite_is_satisfied
#print axioms active_prerequisite_requires_completion
#print axioms inactive_prerequisite_does_not_erase_a_committed_event
#print axioms incompatible_event_is_excluded
#print axioms unfinished_event_is_not_completion
#print axioms unchosen_parallel_rolls_do_not_block_a_visit
#print axioms outer_reentry_allows_a_fresh_visit
#print axioms an_incomplete_visit_cannot_restart
#print axioms reentry_cannot_skip_the_fresh_request
end RollContinuationDependency
