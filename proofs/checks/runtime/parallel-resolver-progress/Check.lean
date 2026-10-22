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

def enclosingParallel : Choreo := .roll (.seq (.send 0 1 231 0)
  (.roll (.par (.send 0 1 232 0) (.send 0 2 233 0))))

def acceptedEnclosing (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 enclosingParallel) trace).isSome

def enclosingVisit : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .send 2, .recv 1, .recv 2]

theorem an_enclosing_repeat_does_not_force_its_prefix_before_an_inner_repeat :
    acceptedEnclosing (enclosingVisit ++ [.roll 0, .send 2, .send 1, .recv 2, .recv 1]) = true := by decide

theorem a_completed_body_can_instead_restart_its_enclosing_prefix :
    acceptedEnclosing (enclosingVisit ++ [.roll 1, .send 0, .recv 0]) = true := by decide

theorem a_fresh_enclosing_visit_still_requires_its_prefix :
    acceptedEnclosing (enclosingVisit ++ [.roll 1, .send 1]) = false := by decide

theorem an_unfinished_parallel_arm_cannot_restart_the_enclosing_prefix :
    acceptedEnclosing [.send 0, .recv 0, .send 1, .recv 1, .roll 1] = false := by decide

def prefixedExchange : Choreo := .roll (.par
  (.seq (.send 0 1 231 0) (.roll (.seq (.send 0 1 232 0) (.send 1 0 233 0))))
  (.send 0 2 234 0))

def acceptedExchange (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 prefixedExchange) trace).isSome

def completedExchange : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2, .send 3, .recv 3]

theorem restarting_the_parallel_parent_requires_its_own_prefix :
    acceptedExchange (completedExchange ++ [.roll 1, .send 1]) = false := by decide

theorem restarting_only_the_exchange_keeps_its_parent_prefix :
    acceptedExchange (completedExchange ++ [.roll 0, .send 1, .recv 1, .send 2, .recv 2]) = true := by decide

theorem a_fresh_parallel_visit_can_start_with_its_independent_arm :
    acceptedExchange (completedExchange ++ [.roll 1, .send 3, .recv 3,
      .send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2]) = true := by decide

theorem a_fresh_parallel_visit_can_start_with_its_prefix :
    acceptedExchange (completedExchange ++ [.roll 1, .send 0, .recv 0,
      .send 1, .recv 1, .send 2, .recv 2, .send 3, .recv 3]) = true := by decide

def choiceWrappedParallel : Choreo := .roll (.route (.dynamic 7)
  (.par (.roll (.seq (.send 0 1 232 0) (.send 1 0 233 0))) (.send 0 2 234 0))
  (.par (.roll (.seq (.send 0 1 241 0) (.send 1 0 242 0))) (.send 0 2 243 0)))

def acceptedWrapped (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 choiceWrappedParallel) trace).isSome

def completedWrappedLeft : List GlobalOperation :=
  [.resolve 0 0 7 .left, .send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2,
    .roll 0, .send 0, .recv 0, .send 1, .recv 1]

theorem a_choice_wrapped_parallel_body_can_repeat_the_same_arm :
    acceptedWrapped (completedWrappedLeft ++ [.roll 2, .resolve 0 0 7 .left,
      .send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2]) = true := by decide

theorem a_choice_wrapped_parallel_body_can_change_its_next_arm :
    acceptedWrapped (completedWrappedLeft ++ [.roll 2, .resolve 0 0 7 .right,
      .send 3, .recv 3, .send 4, .recv 4, .send 5, .recv 5]) = true := by decide

theorem a_choice_wrapped_fresh_body_does_not_reuse_its_old_request :
    acceptedWrapped (completedWrappedLeft ++ [.roll 2, .resolve 0 0 7 .left,
      .send 1]) = false := by decide

def nestedExchange : Choreo := .roll (.seq (.send 0 1 231 0)
  (.roll (.par (.roll (.seq (.send 0 1 232 0) (.send 1 0 233 0)))
    (.send 0 2 234 0))))

def acceptedNested (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 nestedExchange) trace).isSome

def nestedVisit : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2, .send 3, .recv 3]

def anotherExchange : List GlobalOperation := [.send 1, .recv 1, .send 2, .recv 2]

-- These prefixes have identical communication but different roll entries.
-- Their next operations must be compared at those distinct source cuts.
theorem an_old_parallel_group_can_finish_after_another_inner_exchange :
    acceptedNested (nestedVisit ++ [.roll 0] ++ anotherExchange ++
      [.roll 2, .send 0, .recv 0]) = true := by decide

theorem a_new_parallel_group_still_requires_its_tick :
    acceptedNested (nestedVisit ++ [.roll 1] ++ anotherExchange ++
      [.roll 2, .send 0]) = false := by decide

theorem that_new_parallel_group_can_finish_after_its_tick :
    acceptedNested (nestedVisit ++ [.roll 1] ++ anotherExchange ++
      [.send 3, .recv 3, .roll 2, .send 0, .recv 0]) = true := by decide

def sameLaneRepeat : Choreo := .roll (.seq (.send 0 1 231 0)
  (.seq (.roll (.seq (.send 0 1 232 0) (.send 1 0 233 0)))
    (.send 0 1 234 0)))

def acceptedSameLane (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 sameLaneRepeat) trace).isSome

theorem a_child_repeat_after_its_same_lane_tail_keeps_the_old_prefix :
    acceptedSameLane (nestedVisit ++ [.roll 0] ++ anotherExchange) = true := by decide

theorem the_completed_child_then_allows_a_new_enclosing_prefix :
    acceptedSameLane (nestedVisit ++ [.roll 0] ++ anotherExchange ++
      [.roll 1, .send 0, .recv 0]) = true := by decide

theorem an_unfinished_child_still_blocks_the_enclosing_restart :
    acceptedSameLane (nestedVisit ++ [.roll 0, .send 1, .recv 1, .roll 1]) = false := by decide

theorem a_new_enclosing_visit_cannot_skip_its_prefix :
    acceptedSameLane (nestedVisit ++ [.roll 1, .send 1]) = false := by decide

def crossLaneRepeat : Choreo := .roll (.seq (.send 0 1 231 0)
  (.seq (.roll (.par (.send 0 1 232 0) (.send 0 2 234 0)))
    (.par (.send 0 1 244 0) (.send 0 2 245 0))))

def acceptedCrossLane (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 3 crossLaneRepeat) trace).isSome

def completedCrossLane : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .send 2, .recv 1, .recv 2,
    .send 3, .send 4, .recv 3, .recv 4]

theorem a_child_repeat_keeps_its_prefix_before_both_parallel_heads :
    acceptedCrossLane (completedCrossLane ++ [.roll 0, .send 2, .recv 2, .send 1, .recv 1]) = true := by decide

theorem a_fresh_parent_cannot_skip_the_prefix_on_another_lane :
    acceptedCrossLane (completedCrossLane ++ [.roll 1, .send 2]) = false := by decide

def fourWayChoice : Choreo := .par (.send 0 1 80 0)
  (.route .intrinsic
    (.par (.par (.par (.send 0 1 81 0) (.send 0 1 82 0))
      (.send 0 1 83 0)) (.send 0 1 84 0))
    (.send 0 1 85 0))

def acceptedFourWay (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 fourWayChoice) trace).isSome

theorem every_parallel_head_can_reveal_the_enclosing_choice :
    [1, 2, 3, 4].all (fun event => acceptedFourWay [.send event, .recv event]) = true := by decide

theorem the_selected_parallel_arm_can_finish_before_its_outer_sibling :
    acceptedFourWay [.send 3, .recv 3, .send 4, .recv 4,
      .send 2, .recv 2, .send 1, .recv 1, .send 0, .recv 0] = true := by decide

theorem a_parallel_head_does_not_authorize_the_other_choice_arm :
    acceptedFourWay [.send 1, .recv 1, .send 5] = false := by decide

def laterNestedChoice : Choreo := .route .intrinsic
  (.route .intrinsic (.send 0 1 0 0)
    (.par (.route .intrinsic (.send 0 1 1 0) (.send 0 1 2 0)) (.send 0 1 3 0)))
  (.par (.send 0 1 4 0) (.send 0 1 5 0))

def acceptedLaterChoice (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 laterNestedChoice) trace).isSome

theorem completing_a_parallel_sibling_keeps_the_nested_choice :
    acceptedLaterChoice [.send 3, .recv 3, .send 1, .recv 1] = true := by decide

theorem that_sibling_does_not_authorize_another_committed_branch :
    acceptedLaterChoice [.send 3, .recv 3, .send 4] = false := by decide

theorem that_sibling_does_not_allow_a_completed_receive_to_repeat :
    acceptedLaterChoice [.send 3, .recv 3, .send 1, .recv 1, .recv 1] = false := by decide

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
#print axioms an_enclosing_repeat_does_not_force_its_prefix_before_an_inner_repeat
#print axioms a_completed_body_can_instead_restart_its_enclosing_prefix
#print axioms a_fresh_enclosing_visit_still_requires_its_prefix
#print axioms an_unfinished_parallel_arm_cannot_restart_the_enclosing_prefix
#print axioms restarting_the_parallel_parent_requires_its_own_prefix
#print axioms restarting_only_the_exchange_keeps_its_parent_prefix
#print axioms a_fresh_parallel_visit_can_start_with_its_independent_arm
#print axioms a_fresh_parallel_visit_can_start_with_its_prefix
#print axioms a_choice_wrapped_parallel_body_can_repeat_the_same_arm
#print axioms a_choice_wrapped_parallel_body_can_change_its_next_arm
#print axioms a_choice_wrapped_fresh_body_does_not_reuse_its_old_request
#print axioms an_old_parallel_group_can_finish_after_another_inner_exchange
#print axioms a_new_parallel_group_still_requires_its_tick
#print axioms that_new_parallel_group_can_finish_after_its_tick
#print axioms a_child_repeat_after_its_same_lane_tail_keeps_the_old_prefix
#print axioms the_completed_child_then_allows_a_new_enclosing_prefix
#print axioms an_unfinished_child_still_blocks_the_enclosing_restart
#print axioms a_new_enclosing_visit_cannot_skip_its_prefix
#print axioms a_child_repeat_keeps_its_prefix_before_both_parallel_heads
#print axioms a_fresh_parent_cannot_skip_the_prefix_on_another_lane
#print axioms every_parallel_head_can_reveal_the_enclosing_choice
#print axioms the_selected_parallel_arm_can_finish_before_its_outer_sibling
#print axioms a_parallel_head_does_not_authorize_the_other_choice_arm
#print axioms completing_a_parallel_sibling_keeps_the_nested_choice
#print axioms that_sibling_does_not_authorize_another_committed_branch
#print axioms that_sibling_does_not_allow_a_completed_receive_to_repeat
end ParallelResolverProgress
