import Std.Tactic

/-!
Source-linked model of the fallback in
src/endpoint/kernel/offer/select_observed.rs, lines 75–88, Hibana 102fc47.

This is reached only after the materialized-current lookup, roll-reentry lookup,
and active-reentry lookup miss. Diagnostic-only execution observed precisely
these misses with current=4 (done BInstall) and pending head=5 (BUse).
The corrected policy below models the production guard. The model was checked
before the Rust change; it is not a universal Rust source-refinement proof.
-/

namespace HibanaQuic.OfferIngress

structure Context where
  current : Nat
  currentLane : Option Nat
  currentDone : Bool
  observedLane : Nat
  pendingHead : Option Nat
  deriving DecidableEq

/-- Exact existing Rust match: a same-lane cursor wins without a completion check. -/
def existingIndex (s : Context) : Option Nat :=
  match s.currentLane with
  | some lane => if lane = s.observedLane then some s.current else s.pendingHead
  | none => s.pendingHead

/-- Candidate semantic policy: only a still-live same-lane cursor supplies context. -/
def liveIndex (s : Context) : Option Nat :=
  match s.currentLane with
  | some lane =>
      if lane = s.observedLane ∧ s.currentDone = false then some s.current else s.pendingHead
  | none => s.pendingHead

/-- Concrete two-service descriptor ranges. Installation indices 0 and 4 are
outside the rolled route regions [1,4) and [5,8), respectively. The lane/frame
pair uniquely identifies a branch within the containing region. -/
def enclosingRoute (index lane frame : Nat) : Option Nat :=
  if lane = 0 ∧ 1 ≤ index ∧ index < 4 ∧ frame ≤ 1 then some 0
  else if lane = 1 ∧ 5 ≤ index ∧ index < 8 ∧ frame ≤ 1 then some 1
  else none

def existingSelection (s : Context) (frame : Nat) : Option Nat :=
  (existingIndex s).bind (fun index => enclosingRoute index s.observedLane frame)

def candidateSelection (s : Context) (frame : Nat) : Option Nat :=
  (liveIndex s).bind (fun index => enclosingRoute index s.observedLane frame)

/-- These exact values were printed by the diagnostic source copy. -/
def observed : Context := {
  current := 4
  currentLane := some 1
  currentDone := true
  observedLane := 1
  pendingHead := some 5
}

theorem actual_guard_selects_completed_installation : existingIndex observed = some 4 := by
  decide

theorem actual_guard_rejects_fresh_use : existingSelection observed 0 = none := by decide
theorem actual_guard_rejects_fresh_retire : existingSelection observed 1 = none := by decide
theorem pending_head_is_valid_for_fresh_use : enclosingRoute 5 1 0 = some 1 := by decide
theorem pending_head_is_valid_for_fresh_retire : enclosingRoute 5 1 1 = some 1 := by decide
theorem candidate_admits_fresh_use : candidateSelection observed 0 = some 1 := by decide
theorem candidate_admits_fresh_retire : candidateSelection observed 1 = some 1 := by decide

/-- Completion, not merely lane equality, determines whether stale cursor
context may override the descriptor-owned pending head. -/
theorem completed_context_uses_pending (s : Context) (done : s.currentDone = true) :
    liveIndex s = s.pendingHead := by
  cases laneCase : s.currentLane <;> simp [liveIndex, laneCase, done]

theorem live_context_behavior_is_preserved (s : Context) (live : s.currentDone = false) :
    liveIndex s = existingIndex s := by
  cases laneCase : s.currentLane <;> simp [liveIndex, existingIndex, laneCase, live]

theorem foreign_lane_behavior_is_preserved (s : Context)
    (foreign : s.currentLane ≠ some s.observedLane) :
    liveIndex s = existingIndex s := by
  cases laneCase : s.currentLane with
  | none => simp [liveIndex, existingIndex, laneCase]
  | some lane =>
      have different : lane ≠ s.observedLane := by
        intro equal
        apply foreign
        simpa [equal] using laneCase
      simp [liveIndex, existingIndex, laneCase, different]

theorem completed_context_selects_available_pending_scope
    (s : Context) (frame scope : Nat)
    (done : s.currentDone = true)
    (available : s.pendingHead.bind (fun index => enclosingRoute index s.observedLane frame) = some scope) :
    candidateSelection s frame = some scope := by
  unfold candidateSelection
  rw [completed_context_uses_pending s done]
  exact available

#print axioms actual_guard_selects_completed_installation
#print axioms actual_guard_rejects_fresh_use
#print axioms actual_guard_rejects_fresh_retire
#print axioms pending_head_is_valid_for_fresh_use
#print axioms pending_head_is_valid_for_fresh_retire
#print axioms candidate_admits_fresh_use
#print axioms candidate_admits_fresh_retire
#print axioms completed_context_uses_pending
#print axioms live_context_behavior_is_preserved
#print axioms foreign_lane_behavior_is_preserved
#print axioms completed_context_selects_available_pending_scope

end HibanaQuic.OfferIngress
