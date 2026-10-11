import Std.Tactic
namespace ReentryAdmission
/- Source bridge: event_progress.rs validate_event_enabled_reentry tests both
relocatable_step_done and event_progress_passed, retaining
roll_reentry_event_allows_index as the only reset authorization. Other
row/dependency/conflict/lane guards remain conjuncts. This is not Rust refinement. -/

structure ProgressEvent where
 lane : Nat
 step : Nat
 done : Bool

def passed (lane step : Nat) (events : List ProgressEvent) : Bool :=
 events.any (fun e => decide (e.lane = lane ∧ step < e.step ∧ e.done = true))

def progressPassed (head : Option Nat) (lane step : Nat) (events : List ProgressEvent) : Bool :=
 match head with
 | some current => decide (step < current)
 | none => passed lane step events

theorem fresh_reset_bounds_the_new_visit :
 progressPassed (some 0) 0 0 [⟨0, 11, true⟩] = false := by decide

theorem parked_progress_uses_commit_evidence (lane step : Nat) (events : List ProgressEvent) :
 progressPassed none lane step events = passed lane step events := by rfl

theorem passed_requires_a_later_committed_event
 (lane step : Nat) (events : List ProgressEvent) (h : passed lane step events = true) :
 ∃ e ∈ events, e.lane = lane ∧ step < e.step ∧ e.done = true := by
 obtain ⟨e, mem, guard⟩ := List.any_eq_true.mp h
 exact ⟨e, mem, of_decide_eq_true guard⟩

theorem parked_lane_without_later_commits_is_not_past
 (lane step : Nat) (events : List ProgressEvent)
 (noLater : ∀ e ∈ events, e.lane = lane → step < e.step → e.done = false) :
 passed lane step events = false := by
 cases h : passed lane step events with
 | false => rfl
 | true =>
   obtain ⟨e, mem, sameLane, later, done⟩ := passed_requires_a_later_committed_event lane step events h
   have := noLater e mem sameLane later
   simp [done] at this

theorem independent_lane_commit_does_not_pass_a_suffix :
 passed 0 4 [⟨1, 11, true⟩] = false := by decide

theorem later_same_lane_commit_passes_an_unchosen_suffix :
 passed 0 4 [⟨0, 11, true⟩] = true := by decide
def old (done reentry : Bool) : Bool := !done || reentry
def revised (done beforeProgress reentry : Bool) : Bool :=
 !(done || beforeProgress) || reentry

theorem old_unselected_past_suffix_is_admitted : old false false = true := by decide
theorem revised_unselected_past_suffix_is_rejected : revised false true false = false := by decide
theorem current_unfinished_sibling_preserved : revised false false false = true := by decide

theorem preserves_forward (done reentry : Bool) :
 revised done false reentry = old done reentry := by cases done <;> cases reentry <;> decide

theorem preserves_done (beforeProgress reentry : Bool) :
 revised true beforeProgress reentry = old true reentry := by
 cases beforeProgress <;> cases reentry <;> decide

theorem preserves_authorized_elastic (done beforeProgress : Bool) :
 revised done beforeProgress true = true := by cases done <;> cases beforeProgress <;> decide

theorem past_requires_reentry (done reentry : Bool)
 (admitted : revised done true reentry = true) : reentry = true := by
 cases done <;> cases reentry <;> simp_all [revised]

theorem no_new_admissions (done beforeProgress reentry : Bool)
 (admitted : revised done beforeProgress reentry = true) : old done reentry = true := by
 cases done <;> cases beforeProgress <;> cases reentry <;> simp_all [revised,old]

/-- The unified observed-frame selector uses the same event eligibility as
typed receive. Offer entry includes first-visible and per-lane route arm
route entries; it does not bypass event eligibility. -/
def offer (exact offerEntry eventEnabled : Bool) : Bool :=
 exact && offerEntry && eventEnabled

theorem offer_cannot_select_disabled (exact offerEntry : Bool) :
 offer exact offerEntry false = false := by cases exact <;> cases offerEntry <;> decide
#print axioms past_requires_reentry
#print axioms no_new_admissions
#print axioms preserves_authorized_elastic
#print axioms offer_cannot_select_disabled
#print axioms passed_requires_a_later_committed_event
#print axioms parked_lane_without_later_commits_is_not_past
#print axioms fresh_reset_bounds_the_new_visit
end ReentryAdmission
