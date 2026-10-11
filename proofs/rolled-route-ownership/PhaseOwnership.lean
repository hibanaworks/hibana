import Hibana.GlobalSemantics
namespace PhaseOwnership
open Hibana
def work (base more failed prepared cont : Nat) : Choreo :=
 .roll (.route .intrinsic
  (.seq (.send 0 1 base 4)
   (.seq (.route .intrinsic (.send 1 0 more 4) (.send 1 0 failed 4))
    (.seq (.route .intrinsic (.send 1 0 prepared 4) (.send 1 0 cont 4))
     (.send 0 1 162 4))))
  (.seq (.send 0 1 (base+1) 4)
   (.seq (.send 1 0 165 4) (.send 0 1 162 4))))
def choreography : Choreo := .seq (work 0 180 200 190 210)
 (.seq (.send 1 0 2 4) (work 8 181 201 191 211))
#eval choreography.globalEvents.map (fun e => (e.label,e.lane))
#eval choreography.globalRolls.map (fun r => (r.events,r.conflicts))
def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
 | current, [] => some current
 | current, action :: rest => do
   let next ← current.step? action
   run next rest
-- First phase input, failed outcome, prepared outcome, ResultTaken, Grant;
-- second phase input, More outcome. Runtime wire then carries Continue211.
def initialTrace : List GlobalOperation := [.send 0,.recv 0,.send 2,.recv 2,
 .send 3,.recv 3,.send 5,.recv 5,.send 9,.recv 9,
 .send 10,.recv 10,.send 11,.recv 11]
def accepted (suffix : List GlobalOperation) : Bool :=
 (run (GlobalConfig.initial 1 2 choreography) (initialTrace ++ suffix)).isSome
#eval accepted []
#eval accepted [.send 14,.recv 14]
#eval accepted [.send 14,.recv 4]
#eval accepted [.send 4]
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
theorem valid_continuation : accepted [.send 14,.recv 14] = true := by decide
theorem old_label_cannot_consume_current_packet : accepted [.send 14,.recv 4] = false := by decide
#print axioms valid_continuation
#print axioms old_label_cannot_consume_current_packet
end PhaseOwnership
namespace PhaseOwnership
#eval accepted [.roll 0,.send 0,.recv 0,.send 4,.recv 4]
#eval accepted [.roll 0,.send 0,.recv 0,.send 1,.recv 1,.send 4,.recv 4]
theorem old_suffix_without_new_input_is_rejected : accepted [.send 4] = false := by decide
theorem earlier_roll_remains_elastic_after_a_fresh_input :
 accepted [.roll 0,.send 0,.recv 0,.send 1,.recv 1,.send 4,.recv 4] = true := by decide
#print axioms old_suffix_without_new_input_is_rejected
#print axioms earlier_roll_remains_elastic_after_a_fresh_input
end PhaseOwnership
