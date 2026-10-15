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

#print axioms a_scheduled_entry_is_eligible
#print axioms an_in_progress_choice_cannot_change_arm
#print axioms right_then_left_remains_enabled
#print axioms left_then_right_remains_enabled
#print axioms progress_on_another_lane_is_not_resolver_authority
#print axioms the_resolver_cannot_change_an_unfinished_choice
#print axioms a_completed_effect_cannot_repeat_without_a_fresh_request
end ParallelResolverProgress
