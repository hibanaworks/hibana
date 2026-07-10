import Std

namespace Hibana

def maxLeaseGeneration : Nat := 2 ^ 32 - 1

/-- The fail-closed successor used by the resident endpoint lease ledger. -/
def nextLeaseGeneration? (current : Nat) : Option Nat :=
  if current < maxLeaseGeneration then some (current + 1) else none

theorem next_lease_generation_strictly_increases
    {current next : Nat}
    (advanced : nextLeaseGeneration? current = some next) : current < next := by
  unfold nextLeaseGeneration? at advanced
  split at advanced
  next below =>
    simp at advanced
    subst next
    exact Nat.lt_succ_self current
  next exhausted => simp at advanced

theorem next_lease_generation_stays_nonzero
    {current next : Nat}
    (advanced : nextLeaseGeneration? current = some next) : 0 < next := by
  have increases := next_lease_generation_strictly_increases advanced
  exact Nat.zero_lt_of_lt increases

theorem next_lease_generation_stays_in_domain
    {current next : Nat}
    (advanced : nextLeaseGeneration? current = some next) :
    next <= maxLeaseGeneration := by
  unfold nextLeaseGeneration? at advanced
  split at advanced
  next below =>
    simp at advanced
    subst next
    exact below
  next exhausted => simp at advanced

theorem max_lease_generation_is_exhausted :
    nextLeaseGeneration? maxLeaseGeneration = none := by
  decide

inductive SessionFault where
  | transportClosed
  | decodeFailed
  | protocolViolation
  | endpointDropped
  | progressInvariantViolated
  deriving DecidableEq

inductive SessionGenerationState where
  | live
  | poisoned (cause : SessionFault)
  | retired
  deriving DecidableEq

/-- Publication after external code returns is allowed only while the same
session generation remains live. -/
def SessionGenerationState.publicationPermitted : SessionGenerationState -> Bool
  | .live => true
  | .poisoned _ => false
  | .retired => false

theorem poisoned_generation_publish_rejected (cause : SessionFault) :
    SessionGenerationState.publicationPermitted (.poisoned cause) = false := rfl

theorem publication_permitted_implies_live
    {state : SessionGenerationState}
    (permitted : state.publicationPermitted = true) :
    state = .live := by
  cases state <;> simp_all [SessionGenerationState.publicationPermitted]

inductive SessionGenerationAction where
  | attach
  | poison (cause : SessionFault)
  | releaseRemaining
  | releaseLast

/-- First fault wins; no transition can make a poisoned generation live again. -/
def applySessionGenerationAction :
    SessionGenerationState -> SessionGenerationAction -> Option SessionGenerationState
  | .live, .attach => some .live
  | .live, .poison cause => some (.poisoned cause)
  | .live, .releaseRemaining => some .live
  | .live, .releaseLast => some .retired
  | .poisoned _, .attach => none
  | .poisoned cause, .poison _ => some (.poisoned cause)
  | .poisoned cause, .releaseRemaining => some (.poisoned cause)
  | .poisoned _, .releaseLast => some .retired
  | .retired, _ => none

def checkSessionGenerationTrace :
    SessionGenerationState -> List SessionGenerationAction -> Bool
  | _, [] => true
  | state, action :: rest =>
      match applySessionGenerationAction state action with
      | none => false
      | some next => checkSessionGenerationTrace next rest

def runSessionGenerationTrace :
    SessionGenerationState -> List SessionGenerationAction -> Option SessionGenerationState
  | state, [] => some state
  | state, action :: rest =>
      match applySessionGenerationAction state action with
      | none => none
      | some next => runSessionGenerationTrace next rest

def SessionGenerationTrace :
    SessionGenerationState -> List SessionGenerationAction -> SessionGenerationState -> Prop
  | state, [], final => state = final
  | state, action :: rest, final =>
      exists next,
        applySessionGenerationAction state action = some next /\
        SessionGenerationTrace next rest final

theorem session_generation_run_sound
    {state final : SessionGenerationState} {actions : List SessionGenerationAction}
    (accepted : runSessionGenerationTrace state actions = some final) :
    SessionGenerationTrace state actions final := by
  induction actions generalizing state with
  | nil =>
      simp [runSessionGenerationTrace] at accepted
      exact accepted
  | cons action rest ih =>
      simp only [runSessionGenerationTrace] at accepted
      cases step : applySessionGenerationAction state action with
      | none => simp [step] at accepted
      | some next =>
          simp [step] at accepted
          exact ⟨next, step, ih accepted⟩

theorem session_generation_trace_checker_sound
    {state : SessionGenerationState} {actions : List SessionGenerationAction}
    (accepted : checkSessionGenerationTrace state actions = true) :
    exists final, SessionGenerationTrace state actions final := by
  induction actions generalizing state with
  | nil => exact ⟨state, rfl⟩
  | cons action rest ih =>
      simp only [checkSessionGenerationTrace] at accepted
      cases step : applySessionGenerationAction state action with
      | none => simp [step] at accepted
      | some next =>
          simp [step] at accepted
          obtain ⟨final, trace⟩ := ih accepted
          exact ⟨final, next, step, trace⟩

theorem poisoned_generation_step_never_revives
    {cause : SessionFault} {action : SessionGenerationAction}
    {next : SessionGenerationState}
    (step : applySessionGenerationAction (.poisoned cause) action = some next) :
    next = .poisoned cause \/ next = .retired := by
  cases action with
  | attach => simp [applySessionGenerationAction] at step
  | poison newCause =>
      simp [applySessionGenerationAction] at step
      exact Or.inl step.symm
  | releaseRemaining =>
      simp [applySessionGenerationAction] at step
      exact Or.inl step.symm
  | releaseLast =>
      simp [applySessionGenerationAction] at step
      exact Or.inr step.symm

theorem poisoned_generation_attach_rejected (cause : SessionFault) :
    applySessionGenerationAction (.poisoned cause) .attach = none := rfl

theorem poisoned_generation_trace_never_revives
    {cause : SessionFault} {actions : List SessionGenerationAction}
    {final : SessionGenerationState}
    (trace : SessionGenerationTrace (.poisoned cause) actions final) :
    final = .poisoned cause \/ final = .retired := by
  induction actions generalizing cause with
  | nil =>
      simp [SessionGenerationTrace] at trace
      exact Or.inl trace.symm
  | cons action rest ih =>
      simp only [SessionGenerationTrace] at trace
      obtain ⟨next, step, tail⟩ := trace
      rcases poisoned_generation_step_never_revives step with same | retired
      · subst next
        exact ih tail
      · subst next
        cases rest with
        | nil =>
            simp [SessionGenerationTrace] at tail
            exact Or.inr tail.symm
        | cons nextAction tailActions =>
            simp [SessionGenerationTrace, applySessionGenerationAction] at tail

end Hibana
