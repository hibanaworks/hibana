import Hibana.GlobalSemantics

namespace SendContinuation
open Hibana

/-- Descriptor entry identity is distinct from membership in its arm interval.
Source bridge: send_preview_is_at_controller_arm_entry must compare the selected
index with controller_arm_entry_by_arm, not route_arm_for_index. The other two
decision boundaries (route start and unlabeled node) remain unchanged. -/
def decision (start unlabeled : Bool) (entries : List Nat) (index : Nat) : Bool :=
  start || unlabeled || entries.contains index

def locate (boundary : Bool) (candidate rediscovered : Nat) : Nat :=
  if boundary then rediscovered else candidate

theorem continuation_keeps_event_identity
    (entries : List Nat) (index rediscovered : Nat)
    (notEntry : index ∉ entries) :
    locate (decision false false entries index) index rediscovered = index := by
  simp [locate, decision, List.contains_eq_mem, notEntry]

theorem actual_entry_remains_a_decision
    (entries : List Nat) (index : Nat) (entry : index ∈ entries) :
    decision false false entries index = true := by
  simp [decision, List.contains_eq_mem, entry]

theorem route_start_remains_a_decision (entries : List Nat) (index : Nat) :
    decision true false entries index = true := by simp [decision]

theorem unlabeled_node_remains_a_decision (entries : List Nat) (index : Nat) :
    decision false true entries index = true := by simp [decision]

def choreography : Choreo := .seq
  (.roll (.route .intrinsic
    (.seq (.send 0 1 171 0)
      (.route .intrinsic
        (.seq (.send 1 0 188 0) (.send 0 1 184 0))
        (.seq (.route .intrinsic (.send 1 0 182 0) (.send 1 0 183 0))
          (.send 0 1 184 0))))
    (.send 0 1 185 0)))
  (.seq (.send 1 0 186 0) (.send 0 1 187 0))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 choreography) trace).isSome

def applied : List GlobalOperation := [.send 0, .recv 0, .send 3, .recv 3]
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem applied_continuation_is_valid : accepted (applied ++ [.send 5, .recv 5]) = true := by decide
theorem unchosen_same_label_is_invalid : accepted (applied ++ [.send 2]) = false := by decide
theorem continuation_requires_result : accepted [.send 0, .recv 0, .send 5] = false := by decide
theorem duplicate_continuation_is_invalid :
    accepted (applied ++ [.send 5, .recv 5, .send 5]) = false := by decide
theorem fresh_request_allows_alternating_result :
    accepted (applied ++ [.send 5, .recv 5, .roll 0, .send 0, .recv 0,
      .send 4, .recv 4, .send 5, .recv 5]) = true := by decide
theorem first_retire_is_valid :
    accepted [.send 6, .recv 6, .send 7, .recv 7, .send 8, .recv 8] = true := by decide

#print axioms continuation_keeps_event_identity
#print axioms actual_entry_remains_a_decision
#print axioms route_start_remains_a_decision
#print axioms unlabeled_node_remains_a_decision
#print axioms applied_continuation_is_valid
#print axioms unchosen_same_label_is_invalid
#print axioms continuation_requires_result
#print axioms duplicate_continuation_is_invalid
#print axioms fresh_request_allows_alternating_result
#print axioms first_retire_is_valid
end SendContinuation
