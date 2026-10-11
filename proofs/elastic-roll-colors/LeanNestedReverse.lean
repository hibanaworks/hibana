import Hibana.GlobalSemantics
namespace AuditNestedReverse
open Hibana
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
-- Events A=0 and B=1; global roll order is inner=0, outer=1.
def choreo : Choreo := .roll (.seq (.send 0 1 10 16) (.roll (.send 0 1 11 16)))
def pairs (ids : List Nat) : List GlobalOperation := ids.flatMap fun id => [.send id, .recv id]
def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | c, [] => some c
  | c, op :: rest => do
    let next ← c.step? op
    run next rest
def accepts (suffix : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 choreo) (pairs [0,1] ++ suffix)).isSome
#eval choreo.globalRolls.map (fun r => r.events)
theorem initial_visit : accepts [] = true := by decide
theorem inner_head_can_repeat : accepts ([.roll 0] ++ pairs [1]) = true := by decide
theorem outer_prefix_can_repeat : accepts ([.roll 1] ++ pairs [0]) = true := by decide
theorem outer_reset_blocks_inner_without_prefix : accepts [.roll 1, .send 1] = false := by decide
theorem outer_reset_invalidates_inner_completion : accepts [.roll 1, .roll 0] = false := by decide
theorem fresh_prefix_then_inner_is_legal : accepts ([.roll 1] ++ pairs [0,1]) = true := by decide
#print axioms initial_visit
#print axioms inner_head_can_repeat
#print axioms outer_prefix_can_repeat
#print axioms outer_reset_blocks_inner_without_prefix
#print axioms outer_reset_invalidates_inner_completion
#print axioms fresh_prefix_then_inner_is_legal
end AuditNestedReverse
