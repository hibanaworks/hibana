import Std.Tactic
namespace ReentryAdmission
/- Model of validate_event_enabled_reentry in event_progress.rs. Completion,
committed choices and explicit or automatic roll reentry determine admission.
A resident lane's lookup position is not progress. Row, source dependencies,
actor ownership and lane binding are additional guards. This is not Rust refinement. -/

def admission (done completedRoll compatible explicit headAllowed reentryAllowed : Bool) : Bool :=
 if explicit then headAllowed
 else if done || completedRoll then reentryAllowed
 else compatible

theorem an_unfinished_current_event_is_admitted (headAllowed reentryAllowed : Bool) :
 admission false false true false headAllowed reentryAllowed = true := by
 cases headAllowed <;> cases reentryAllowed <;> decide

theorem a_committed_branch_cannot_be_replaced (headAllowed reentryAllowed : Bool) :
 admission false false false false headAllowed reentryAllowed = false := by
 cases headAllowed <;> cases reentryAllowed <;> decide

theorem a_completed_event_requires_reentry
 (completedRoll compatible headAllowed reentryAllowed : Bool)
 (accepted : admission true completedRoll compatible false headAllowed reentryAllowed = true) :
 reentryAllowed = true := by simpa [admission] using accepted

theorem a_completed_roll_requires_reentry
 (done compatible headAllowed reentryAllowed : Bool)
 (accepted : admission done true compatible false headAllowed reentryAllowed = true) :
 reentryAllowed = true := by
 cases done <;> simpa [admission] using accepted

theorem explicit_reentry_requires_its_head
 (done completedRoll compatible headAllowed reentryAllowed : Bool)
 (accepted : admission done completedRoll compatible true headAllowed reentryAllowed = true) :
 headAllowed = true := by simpa [admission] using accepted

theorem an_authorized_fresh_head_can_replace_old_choices
 (done completedRoll compatible reentryAllowed : Bool) :
 admission done completedRoll compatible true true reentryAllowed = true := by rfl

theorem authorized_automatic_reentry_preserves_completed_events
 (completedRoll compatible headAllowed : Bool) :
 admission true completedRoll compatible false headAllowed true = true := by rfl

/-- Offer uses the same event eligibility as typed receive, in addition to its
exact frame and branch-entry checks. -/
def offer (exact offerEntry eventEnabled : Bool) : Bool :=
 exact && offerEntry && eventEnabled

theorem offer_cannot_select_disabled (exact offerEntry : Bool) :
 offer exact offerEntry false = false := by cases exact <;> cases offerEntry <;> decide

#print axioms an_unfinished_current_event_is_admitted
#print axioms a_committed_branch_cannot_be_replaced
#print axioms a_completed_event_requires_reentry
#print axioms a_completed_roll_requires_reentry
#print axioms explicit_reentry_requires_its_head
#print axioms an_authorized_fresh_head_can_replace_old_choices
#print axioms authorized_automatic_reentry_preserves_completed_events
#print axioms offer_cannot_select_disabled
end ReentryAdmission
