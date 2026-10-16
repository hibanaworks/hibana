import Init.Data.List.Nat.Basic

namespace OfferPrefixDependency

def previewArm (explicit candidate : Option Bool) : Option Bool :=
  explicit.or candidate

inductive Admission where
  | invalid
  | waiting
  | enabled
  deriving DecidableEq

def checkEvent (identity conflict fresh head dependency : Bool) : Admission :=
  if identity && conflict && fresh then
    if !dependency then .waiting else if head then .enabled else .invalid
  else .invalid

inductive Action where
  | reject
  | retain
  | preview (arm : Bool)
  deriving DecidableEq

-- Exact, unique matching and first-visible branch classification are premises.
def action : Admission → Option Bool → Action
  | .invalid, _ => .reject
  | .waiting, _ => .retain
  | .enabled, none => .retain
  | .enabled, some arm => .preview arm

-- Enabled candidates choose a branch. Pending aliases only retain its bytes.
def selectIngress (enabled : List Bool) (pending : Nat) : Action :=
  match enabled with
  | [arm] => .preview arm
  | [] => if pending = 0 then .reject else .retain
  | _ => .reject

structure State (Frame : Type) where
  position : Nat
  selected : Option Bool
  receive : Nat → List Frame

def requeue (s : State Frame) (lane : Nat) (frame : Frame) : State Frame :=
  { s with receive := fun other =>
      if other = lane then frame :: s.receive other else s.receive other }

-- This is a scan local to one poll, separate from protocol progress and cache.
def remaining (lanes : List Nat) (deferred : Nat) : List Nat :=
  lanes.filter (fun lane => decide (lane ≠ deferred))

theorem unseen_choice_uses_candidate (candidate : Option Bool) :
    previewArm none candidate = candidate := rfl

theorem explicit_choice_is_preserved (arm : Bool) (candidate : Option Bool) :
    previewArm (some arm) candidate = some arm := rfl

theorem absent_path_cannot_invent_choice : previewArm none none = none := rfl

theorem pending_requires_independent_guards (i c f h d : Bool)
    (waiting : checkEvent i c f h d = .waiting) :
    i = true ∧ c = true ∧ f = true ∧ d = false := by
  cases i <;> cases c <;> cases f <;> cases h <;> cases d <;> simp_all [checkEvent]

theorem predecessor_wait_does_not_require_current_head :
    checkEvent true true true false false = .waiting := rfl

theorem enabled_requires_all_guards (i c f h d : Bool)
    (enabled : checkEvent i c f h d = .enabled) :
    i = true ∧ c = true ∧ f = true ∧ h = true ∧ d = true := by
  cases i <;> cases c <;> cases f <;> cases h <;> cases d <;> simp_all [checkEvent]

theorem pending_aliases_retain_without_choosing (count : Nat) :
    selectIngress [] (count + 1) = .retain := by simp [selectIngress]

theorem pending_aliases_do_not_override_enabled (arm : Bool) (count : Nat) :
    selectIngress [arm] count = .preview arm := rfl

theorem enabled_ambiguity_remains_rejected (a b : Bool) (rest : List Bool) (count : Nat) :
    selectIngress (a :: b :: rest) count = .reject := rfl

theorem pending_frame_has_no_authority (arm : Option Bool) :
    action .waiting arm = .retain := rfl

theorem invalid_frame_is_rejected (arm : Option Bool) :
    action .invalid arm = .reject := rfl

theorem preview_requires_enabled (admission : Admission) (arm : Option Bool) (chosen : Bool)
    (preview : action admission arm = .preview chosen) : admission = .enabled := by
  cases admission <;> cases arm <;> simp_all [action]

theorem requeue_preserves_progress (s : State Frame) (lane : Nat) (frame : Frame) :
    (requeue s lane frame).position = s.position ∧
    (requeue s lane frame).selected = s.selected := ⟨rfl, rfl⟩

theorem requeue_keeps_exact_frame (s : State Frame) (lane : Nat) (frame : Frame) :
    (requeue s lane frame).receive lane = frame :: s.receive lane := by
  simp [requeue]

theorem requeue_preserves_other_lane (s : State Frame) (lane other : Nat)
    (frame : Frame) (different : other ≠ lane) :
    (requeue s lane frame).receive other = s.receive other := by
  simp [requeue, different]

theorem deferred_lane_is_not_repolled (lanes : List Nat) (lane : Nat) :
    lane ∉ remaining lanes lane := by
  simp [remaining]

theorem independent_lane_remains_polled (lanes : List Nat) (lane other : Nat)
    (different : other ≠ lane) (present : other ∈ lanes) :
    other ∈ remaining lanes lane := by
  simp [remaining, different, present]

theorem deferral_decreases_remaining_lanes (lanes : List Nat) (lane : Nat)
    (present : lane ∈ lanes) : (remaining lanes lane).length < lanes.length := by
  apply List.length_filter_lt_length_iff_exists.mpr
  exact ⟨lane, present, by simp⟩

-- A different entry of the next visit can arrive while a nested operation of
-- the committed visit is still live. These are descriptor/history facts, not
-- a new progress owner. Exact lane/peer/wire identity is checked by the caller.
structure FutureEntry where
  reentrant : Bool
  firstOnLane : Bool
  differentArm : Bool
  currentWasReceive : Bool
  currentEntryDone : Bool
  currentArmDone : Bool

def waitsForCurrent (g : FutureEntry) : Bool :=
  g.reentrant && g.firstOnLane && g.differentArm && g.currentWasReceive &&
    g.currentEntryDone && !g.currentArmDone

def observeFuture (g : FutureEntry) (admission : Admission)
    (arm : Option Bool) : Action :=
  match admission with
  | .invalid => if waitsForCurrent g then .retain else .reject
  | other => action other arm

theorem future_wait_requires_accepted_unfinished_visit (g : FutureEntry)
    (waiting : waitsForCurrent g = true) :
    g.reentrant = true ∧ g.firstOnLane = true ∧ g.differentArm = true ∧
    g.currentWasReceive = true ∧ g.currentEntryDone = true ∧ g.currentArmDone = false := by
  simpa [waitsForCurrent, Bool.and_eq_true, and_assoc] using waiting

theorem future_wait_without_current_entry_is_rejected (g : FutureEntry)
    (notAccepted : g.currentEntryDone = false) (arm : Option Bool) :
    observeFuture g .invalid arm = .reject := by
  simp [observeFuture, waitsForCurrent, notAccepted]

theorem completed_visit_uses_ordinary_admission (g : FutureEntry)
    (completed : g.currentArmDone = true) (arm : Option Bool) :
    observeFuture g .invalid arm = .reject := by
  simp [observeFuture, waitsForCurrent, completed]

theorem future_retention_cannot_preview (g : FutureEntry) (arm : Option Bool)
    (chosen : Bool) : observeFuture g .invalid arm ≠ .preview chosen := by
  simp [observeFuture]
  split <;> simp

theorem deferred_entry_still_requires_enabled_admission (g : FutureEntry)
    (admission : Admission) (arm : Option Bool) (chosen : Bool)
    (preview : observeFuture g admission arm = .preview chosen) : admission = .enabled := by
  cases admission with
  | invalid => exact False.elim (future_retention_cannot_preview g arm chosen preview)
  | waiting => simp [observeFuture, action] at preview
  | enabled => rfl

theorem early_next_arm_is_retained (arm : Option Bool) :
    observeFuture ⟨true, true, true, true, true, false⟩ .invalid arm = .retain := rfl

#print axioms unseen_choice_uses_candidate
#print axioms explicit_choice_is_preserved
#print axioms absent_path_cannot_invent_choice
#print axioms pending_requires_independent_guards
#print axioms predecessor_wait_does_not_require_current_head
#print axioms enabled_requires_all_guards
#print axioms pending_aliases_retain_without_choosing
#print axioms pending_aliases_do_not_override_enabled
#print axioms enabled_ambiguity_remains_rejected
#print axioms pending_frame_has_no_authority
#print axioms invalid_frame_is_rejected
#print axioms preview_requires_enabled
#print axioms requeue_preserves_progress
#print axioms requeue_keeps_exact_frame
#print axioms requeue_preserves_other_lane
#print axioms deferred_lane_is_not_repolled
#print axioms independent_lane_remains_polled
#print axioms deferral_decreases_remaining_lanes
#print axioms future_wait_requires_accepted_unfinished_visit
#print axioms future_wait_without_current_entry_is_rejected
#print axioms completed_visit_uses_ordinary_admission
#print axioms future_retention_cannot_preview
#print axioms deferred_entry_still_requires_enabled_admission
#print axioms early_next_arm_is_retained
end OfferPrefixDependency
