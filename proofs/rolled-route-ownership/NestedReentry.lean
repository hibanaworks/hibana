import Hibana.GlobalSemantics

namespace NestedReentry
open Hibana

def choreography : Choreo := .seq
  (.roll (.route .intrinsic
    (.seq (.send 0 1 0 0)
      (.seq (.route .intrinsic (.send 1 0 190 0)
        (.route .intrinsic (.send 1 0 180 0) (.send 1 0 200 0)))
        (.send 0 1 162 0)))
    (.seq (.send 0 1 1 0) (.send 1 0 165 0))))
  (.send 1 0 2 0)

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 choreography) trace).isSome

def firstVisit : List GlobalOperation :=
  [.send 0, .recv 0, .send 2, .recv 2, .send 4, .recv 4]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
#eval choreography.globalEvents.map (fun e => (e.label, e.lane))

theorem fresh_request_then_alternating_nested_result :
    accepted (firstVisit ++ [.roll 0, .send 0, .recv 0,
      .send 3, .recv 3, .send 4, .recv 4]) = true := by decide

theorem nested_result_without_fresh_request_is_rejected :
    accepted (firstVisit ++ [.roll 0, .send 3]) = false := by decide

#print axioms fresh_request_then_alternating_nested_result
#print axioms nested_result_without_fresh_request_is_rejected
end NestedReentry
