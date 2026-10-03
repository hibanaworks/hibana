import Hibana.GlobalSemantics
namespace StreamMinimalDirect
open Hibana
-- Exact ten-event shape of ../runtime/minimal_direct.rs, with roles 0,1.
-- The uniform [u8;16] wire schema is represented by opaque schema 16.
def body (bootstrapRolled activeRolled : Bool) : Choreo :=
  let bootstrap := .route .intrinsic (.send 0 1 52 16) (.send 0 1 42 16)
  let active := .route .intrinsic (.send 0 1 55 16) (.send 0 1 72 16)
  .seq (.send 0 1 40 16)
    (.seq (.send 1 0 41 16)
      (.seq (if bootstrapRolled then .roll bootstrap else bootstrap)
        (.route .intrinsic
          (.seq (.seq (.send 1 0 78 16) (.send 0 1 71 16))
            (if activeRolled then .roll active else active))
          (.seq (.send 1 0 73 16) (.send 0 1 74 16)))))
def pairs (ids : List Nat) : List GlobalOperation :=
  ids.flatMap fun id => [.send id, .recv id]
def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest
def accepted (bootstrapRolled activeRolled : Bool) (ids : List Nat) : Bool :=
  (run (GlobalConfig.initial 1 2 (body bootstrapRolled activeRolled)) (pairs ids)).isSome
-- 0 Install; 1 Installed; 2 Inspect; 3 PeerReady; 4 Ready; 5 ResultTaken; 6 Open.
#eval [true,false].flatMap fun b => [true,false].map fun a => (b,a,accepted b a [0,1,3,4,5,6])
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
theorem direct_trace_is_legal_for_all_four_roll_variants :
    accepted true true [0,1,3,4,5,6] = true ∧
    accepted true false [0,1,3,4,5,6] = true ∧
    accepted false true [0,1,3,4,5,6] = true ∧
    accepted false false [0,1,3,4,5,6] = true := by decide
theorem bootstrap_roll_still_requires_one_body_visit :
    accepted true true [0,1,4] = false := by decide
theorem ready_receipt_cannot_be_skipped :
    accepted true true [0,1,3,4,6] = false := by decide
#print axioms direct_trace_is_legal_for_all_four_roll_variants
#print axioms bootstrap_roll_still_requires_one_body_visit
#print axioms ready_receipt_cannot_be_skipped
end StreamMinimalDirect

/- Additional exact-shape checks using the unchanged upstream GlobalConfig.step?.
   The explicit global roll-reset action models reentry; this is not a proof
   that Rust's event_enabled function equals this global semantics. -/
namespace StreamMinimalDirect
open Hibana

def readyPrefix : List GlobalOperation := pairs [0,1,3,4,5]
def acceptsAfterPrefix (suffix : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 2 (body true true)) (readyPrefix ++ suffix)).isSome

theorem ready_receipt_prefix_is_legal : acceptsAfterPrefix [] = true := by decide

theorem current_open_is_legal : acceptsAfterPrefix (pairs [6]) = true := by decide

theorem prior_bootstrap_inspect_can_reenter :
    acceptsAfterPrefix ([.roll 0] ++ pairs [2]) = true := by decide

theorem prior_reentry_preserves_current_open :
    acceptsAfterPrefix ([.roll 0] ++ pairs [2,6]) = true := by decide

theorem inspect_cannot_consume_queued_open :
    acceptsAfterPrefix [.send 6, .recv 2] = false := by decide

theorem explicit_old_reset_still_cannot_consume_queued_open_as_inspect :
    acceptsAfterPrefix [.send 6, .roll 0, .recv 2] = false := by decide

theorem active_roll_cannot_reset_without_a_body_visit :
    acceptsAfterPrefix [.roll 1] = false := by decide

#print axioms ready_receipt_prefix_is_legal
#print axioms current_open_is_legal
#print axioms prior_bootstrap_inspect_can_reenter
#print axioms prior_reentry_preserves_current_open
#print axioms inspect_cannot_consume_queued_open
#print axioms explicit_old_reset_still_cannot_consume_queued_open_as_inspect
#print axioms active_roll_cannot_reset_without_a_body_visit
end StreamMinimalDirect
