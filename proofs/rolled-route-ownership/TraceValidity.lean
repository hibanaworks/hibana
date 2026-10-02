import Hibana.GlobalSemantics
namespace RolledAdjacentTrace
open Hibana
def choreography : Choreo := .roll (.seq
  (.route .intrinsic (.send 1 0 180 4) (.send 1 0 200 4))
  (.route .intrinsic (.send 1 0 190 4) (.send 1 0 210 4)))
def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
 | current, [] => some current
 | current, action :: rest => do
   let next ← current.step? action
   run next rest
-- Exact accepted send/receive prefix, then expected second receive.
def validTrace : List GlobalOperation := [.send 0, .recv 0, .send 3, .recv 3]
def accepted : Bool := (run (GlobalConfig.initial 1 2 choreography) validTrace).isSome
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
#eval choreography.globalEvents.map (fun event => (event.label, event.lane))
theorem actual_public_trace_is_globally_admitted : accepted = true := by decide
#print axioms actual_public_trace_is_globally_admitted
end RolledAdjacentTrace
