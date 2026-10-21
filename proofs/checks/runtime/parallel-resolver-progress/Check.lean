import Hibana.GlobalSemantics

namespace ParallelResolverProgress
open Hibana

/- Scheduling an enabled entry does not choose its route arm. Eligibility uses
committed progress and the existing live selection. The actual resolver remains
an explicit separate transition. This is a source-linked model, not a proof of
all Rust execution. -/
structure Entry where
  id : Nat
  eligible : Bool
  ready : Bool

def firstReady : List Entry → Option Entry
  | [] => none
  | head :: tail => if head.eligible && head.ready = true then some head else firstReady tail

theorem a_scheduled_entry_is_eligible (entries : List Entry) (entry : Entry)
    (chosen : firstReady entries = some entry) :
    entry ∈ entries ∧ entry.eligible = true ∧ entry.ready = true := by
  induction entries with
  | nil => simp [firstReady] at chosen
  | cons head tail ih =>
    simp only [firstReady] at chosen
    split at chosen
    · rename_i allowed
      have both : head.eligible = true ∧ head.ready = true := by
        cases he : head.eligible <;> cases hr : head.ready <;> simp_all
      cases chosen
      exact ⟨by simp, both.1, both.2⟩
    · obtain ⟨mem, eligible, ready⟩ := ih chosen
      exact ⟨by simp [mem], eligible, ready⟩

def armAllowed (live : Option RouteArm) (candidate : RouteArm) : Bool :=
  match live with
  | none => true
  | some selected => decide (selected = candidate)

theorem an_in_progress_choice_cannot_change_arm (selected candidate : RouteArm)
    (allowed : armAllowed (some selected) candidate = true) : selected = candidate := by
  cases selected <;> cases candidate <;> simp_all [armAllowed]

def choreography : Choreo := .par
  (.roll (.seq (.send 1 0 1 0)
    (.route (.dynamic 1)
      (.seq (.send 0 0 2 0) (.send 0 1 3 0)) (.send 0 1 4 0))))
  (.roll (.seq (.send 2 0 11 0)
    (.route (.dynamic 2)
      (.seq (.send 0 0 12 0) (.send 0 2 13 0)) (.send 0 2 14 0))))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 choreography) trace).isSome

def requests : List GlobalOperation := [.send 0, .recv 0, .send 4, .recv 4]
def left : List GlobalOperation := [.resolve 0 0 1 .left, .localAction 1, .send 2, .recv 2]
def right : List GlobalOperation := [.resolve 0 1 2 .left, .localAction 5, .send 6, .recv 6]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem right_then_left_remains_enabled : accepted (requests ++ right ++ left) = true := by decide
theorem left_then_right_remains_enabled : accepted (requests ++ left ++ right) = true := by decide
theorem progress_on_another_lane_is_not_resolver_authority :
    accepted (requests ++ right ++ [.localAction 1]) = false := by decide
theorem the_resolver_cannot_change_an_unfinished_choice :
    accepted (requests ++ [.resolve 0 0 1 .left, .localAction 1, .resolve 0 0 1 .right]) = false := by decide
theorem a_completed_effect_cannot_repeat_without_a_fresh_request :
    accepted (requests ++ right ++ left ++ [.roll 0, .resolve 0 0 1 .left, .localAction 1]) = false := by decide

def sendThenReply : Choreo := .roll (.seq
  (.route .intrinsic (.send 0 1 81 0) (.send 0 1 82 0))
  (.route .intrinsic (.send 1 0 83 0)
    (.route .intrinsic (.send 1 0 84 0) (.send 1 0 85 0))))

def acceptedReply (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 sendThenReply) trace).isSome

theorem a_completed_send_choice_allows_its_stopped_reply :
    acceptedReply [.send 0, .recv 0, .send 3, .recv 3] = true := by decide
theorem the_other_send_choice_allows_its_accepted_reply :
    acceptedReply [.send 1, .recv 1, .send 2, .recv 2] = true := by decide
theorem completing_a_send_does_not_authorize_its_other_arm :
    acceptedReply [.send 0, .recv 0, .send 1] = false := by decide

def prefixThenParallel : Choreo := .seq
  (.roll (.seq (.send 0 1 21 0) (.send 1 0 22 0)))
  (.par (.send 0 1 33 0) (.send 1 0 23 0))

def acceptedParallel (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 prefixThenParallel) trace).isSome

def completedPrefix : List GlobalOperation := [.send 0, .recv 0, .send 1, .recv 1]

theorem a_completed_roll_allows_the_parallel_continuation :
    acceptedParallel (completedPrefix ++ [.send 2, .send 3, .recv 3, .recv 2]) = true := by decide

theorem the_parallel_continuation_keeps_the_other_schedule :
    acceptedParallel (completedPrefix ++ [.send 3, .send 2, .recv 2, .recv 3]) = true := by decide

theorem an_unfinished_prefix_does_not_authorize_the_next_send :
    acceptedParallel [.send 0, .recv 0, .send 2] = false := by decide

theorem a_new_unfinished_visit_still_blocks_the_continuation_send :
    acceptedParallel (completedPrefix ++ [.roll 0, .send 0, .recv 0, .send 2]) = false := by decide

#print axioms a_scheduled_entry_is_eligible
#print axioms an_in_progress_choice_cannot_change_arm
#print axioms right_then_left_remains_enabled
#print axioms left_then_right_remains_enabled
#print axioms progress_on_another_lane_is_not_resolver_authority
#print axioms the_resolver_cannot_change_an_unfinished_choice
#print axioms a_completed_effect_cannot_repeat_without_a_fresh_request
#print axioms a_completed_send_choice_allows_its_stopped_reply
#print axioms the_other_send_choice_allows_its_accepted_reply
#print axioms completing_a_send_does_not_authorize_its_other_arm
#print axioms a_completed_roll_allows_the_parallel_continuation
#print axioms the_parallel_continuation_keeps_the_other_schedule
#print axioms an_unfinished_prefix_does_not_authorize_the_next_send
#print axioms a_new_unfinished_visit_still_blocks_the_continuation_send
end ParallelResolverProgress
