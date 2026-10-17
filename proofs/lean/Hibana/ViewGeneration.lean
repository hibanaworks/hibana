import Hibana.Generation

namespace Hibana

/-- The actor-local view nonce is distinct from the session-run generation.
Odd nonces are live; the following even value retires that issuance. -/
def maxViewGeneration : Nat := 2 ^ 32 - 1

def nextViewGeneration? (current : Nat) : Option Nat :=
  if current % 2 = 1 ∧ current + 2 < maxViewGeneration then
    some (current + 2)
  else none

def retireViewGeneration (current : Nat) : Nat :=
  if current % 2 = 1 then current + 1 else current

def initialViewGeneration? (current candidate : Nat) : Option Nat :=
  if current = 0 ∧ candidate % 2 = 1 ∧ candidate < maxViewGeneration then
    some candidate
  else none

/-- Numeric part of EndpointHandle::matches_header for one fixed view record.
The record's actor address and logical role are unchanged by these operations. -/
def viewHandleMatches (run issued headerRun stored : Nat) : Bool :=
  run == (if stored % 2 = 1 then headerRun else 0) && issued == stored

theorem fresh_view_generation_is_exact
    {current next : Nat} (fresh : nextViewGeneration? current = some next) :
    current % 2 = 1 ∧ next = current + 2 ∧ next < maxViewGeneration := by
  unfold nextViewGeneration? at fresh
  split at fresh
  next valid =>
    have exact : next = current + 2 := Option.some.inj fresh.symm
    exact ⟨valid.1, exact, exact ▸ valid.2⟩
  next invalid => simp at fresh

theorem fresh_view_generation_strictly_increases
    {current next : Nat} (fresh : nextViewGeneration? current = some next) :
    current < next := by
  have exact := (fresh_view_generation_is_exact fresh).2.1
  omega

theorem fresh_view_generation_is_live
    {current next : Nat} (fresh : nextViewGeneration? current = some next) :
    next % 2 = 1 := by
  obtain ⟨odd, exact, _⟩ := fresh_view_generation_is_exact fresh
  omega

theorem fresh_view_generation_reserves_retirement
    {current next : Nat} (fresh : nextViewGeneration? current = some next) :
    retireViewGeneration next = next + 1 ∧ next + 1 < maxViewGeneration := by
  have odd := fresh_view_generation_is_live fresh
  have bound := (fresh_view_generation_is_exact fresh).2.2
  have maximumOdd : maxViewGeneration % 2 = 1 := by decide
  constructor
  · simp [retireViewGeneration, odd]
  · omega

theorem view_retirement_is_idempotent (current : Nat) :
    retireViewGeneration (retireViewGeneration current) = retireViewGeneration current := by
  by_cases live : current % 2 = 1
  · have retired : (current + 1) % 2 ≠ 1 := by omega
    simp [retireViewGeneration, live, retired]
  · simp [retireViewGeneration, live]

theorem view_retirement_invalidates_its_handle
    (run headerRun current : Nat) (live : current % 2 = 1) :
    viewHandleMatches run current headerRun (retireViewGeneration current) = false := by
  have different : (current == current + 1) = false :=
    beq_eq_false_iff_ne.mpr (Nat.ne_of_lt (Nat.lt_succ_self current))
  simp only [viewHandleMatches, retireViewGeneration, if_pos live, different, Bool.and_false]

theorem retired_view_cannot_issue_initial_again
    {current : Nat} (live : current % 2 = 1) (candidate : Nat) :
    initialViewGeneration? (retireViewGeneration current) candidate = none := by
  simp [initialViewGeneration?, retireViewGeneration, live]

/-- Ghost iteration of the existing actor counter, not another runtime ledger.
Other view records may consume intermediate issuances; stale handles must fail
after every nonempty successful sequence, regardless of its finite length. -/
def viewGenerationSequence? : Nat → Nat → Option Nat
  | current, 0 => some current
  | current, count + 1 => do
      let next ← nextViewGeneration? current
      viewGenerationSequence? next count

theorem view_generation_sequence_does_not_decrease
    {current count final : Nat}
    (accepted : viewGenerationSequence? current count = some final) : current ≤ final := by
  induction count generalizing current with
  | zero => exact Nat.le_of_eq (Option.some.inj accepted)
  | succ count ih =>
      cases step : nextViewGeneration? current with
      | none => simp [viewGenerationSequence?, step] at accepted
      | some next =>
          have tail : viewGenerationSequence? next count = some final := by
            simpa [viewGenerationSequence?, step] using accepted
          exact Nat.le_trans (Nat.le_of_lt (fresh_view_generation_strictly_increases step)) (ih tail)

theorem nonempty_view_generation_sequence_is_fresh
    {current count final : Nat}
    (accepted : viewGenerationSequence? current (count + 1) = some final) : current < final := by
  cases step : nextViewGeneration? current with
  | none => simp [viewGenerationSequence?, step] at accepted
  | some next =>
      have tail : viewGenerationSequence? next count = some final := by
        simpa [viewGenerationSequence?, step] using accepted
      exact Nat.lt_of_lt_of_le (fresh_view_generation_strictly_increases step)
        (view_generation_sequence_does_not_decrease tail)

theorem old_view_handle_rejected_after_any_nonempty_issuance_sequence
    {current count final issued : Nat} (run headerRun : Nat)
    (old : issued ≤ current)
    (accepted : viewGenerationSequence? current (count + 1) = some final) :
    viewHandleMatches run issued headerRun final = false := by
  have fresh := nonempty_view_generation_sequence_is_fresh accepted
  have different : issued ≠ final := Nat.ne_of_lt (Nat.lt_of_le_of_lt old fresh)
  simp [viewHandleMatches, different]

theorem maximum_issuable_view_generation_exhausts_the_counter :
    nextViewGeneration? (maxViewGeneration - 2) = none := by decide

end Hibana
