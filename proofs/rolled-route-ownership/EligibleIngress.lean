import Std.Tactic
namespace RolledAdjacent
/- Source-linked abstraction, not a theorem about compiled Rust.
Guards map to recv/matching.rs and cursor/scope_route/event_progress.rs.
The caller must derive guards from immutable descriptors and current runtime
state, including the existing elastic-reentry conflict preview. No guard is
supplied by a packet or presumed merely from cursor position. -/
structure Candidate where
  index : Nat
  exactFrame : Bool
  rowMatches : Bool
  dependency : Bool
  conflict : Bool
  laneHead : Bool
  done : Bool
  beforeProgress : Bool
  reentry : Bool
  -- First visible passive receive, or the route arm's first receive on this lane.
  offerEntry : Bool
  deriving DecidableEq, Repr

def eligible (c : Candidate) : Bool :=
  c.exactFrame && c.rowMatches && c.dependency && c.conflict &&
  c.laneHead && (!(c.done || c.beforeProgress) || c.reentry) && c.offerEntry

def select (cs : List Candidate) : Option Candidate :=
  match cs.filter eligible with
  | [c] => some c
  | _ => none

theorem selected_has_all_guards (cs : List Candidate) (c : Candidate)
    (h : select cs = some c) : eligible c = true := by
  unfold select at h
  split at h
  next c' heq =>
    have eq : c' = c := Option.some.inj h
    subst c'
    have mem : c ∈ cs.filter eligible := by rw [heq]; simp
    exact (List.mem_filter.mp mem).2
  next heq => contradiction

theorem selected_is_unique (cs : List Candidate) (c : Candidate)
    (h : select cs = some c) : cs.filter eligible = [c] := by
  unfold select at h
  split at h
  next c' heq => simpa [Option.some.inj h] using heq
  next heq => contradiction

theorem blocked_candidate_not_selected (cs : List Candidate) (c : Candidate)
    (blocked : eligible c = false) : select cs ≠ some c := by
  intro h
  have := selected_has_all_guards cs c h
  simp [blocked] at this

def target : Candidate := ⟨3,true,true,true,true,true,false,false,false,true⟩
def staleAnchor : Candidate := ⟨1,false,true,true,true,true,false,false,true,true⟩
def alreadyDone : Candidate := { target with index := 0, done := true }
def elastic : Candidate := { alreadyDone with reentry := true }

theorem positive_current_sibling : select [staleAnchor,target] = some target := by decide
theorem rejects_blocked_future : select [{target with laneHead := false}] = none := by decide
theorem rejects_missing_dependency : select [{target with dependency := false}] = none := by decide
theorem rejects_inactive_nonelastic_arm : select [{target with conflict := false}] = none := by decide
theorem rejects_completed_without_reentry : select [alreadyDone] = none := by decide
theorem rejects_unselected_past_suffix :
    select [{target with beforeProgress := true}] = none := by decide
theorem admits_authorized_elastic_reentry : select [elastic] = some elastic := by decide
theorem rejects_wrong_full_frame_key : select [staleAnchor] = none := by decide
theorem rejects_ambiguous_descriptor_match : select [target,{target with index := 4}] = none := by decide

/-- Exact old fallback: same-lane unfinished cursor overrides pending lane head. -/
def oldAnchor (sameLane done : Bool) (current head : Nat) : Nat :=
  if sameLane && !done then current else head
def scopeForFrame3 (index : Nat) : Option Nat :=
  if 2 ≤ index ∧ index < 4 then some 2 else none

theorem old_fallback_rejects_valid_frame : scopeForFrame3 (oldAnchor true false 1 0) = none := by decide
theorem target_anchor_finds_correct_scope : scopeForFrame3 target.index = some 2 := by decide
#print axioms selected_has_all_guards
#print axioms selected_is_unique
#print axioms blocked_candidate_not_selected
#print axioms positive_current_sibling
end RolledAdjacent
