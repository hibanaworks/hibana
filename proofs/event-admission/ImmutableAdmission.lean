import Std

namespace Hibana.ImmutableAdmission

structure Identity where
  effIndex : Nat
  label : Nat
  origin : Nat
  scope : Nat
  routeArm : Option Nat
  lane : Nat
  deriving DecidableEq

structure Row where
  identity : Identity
  next : Nat
  dependency : Nat
  conflict : Nat

structure Certificate where
  index : Nat
  progressStep : Nat
  next : Nat
  deriving DecidableEq

-- The observer state is arbitrary: a live route callback may have effects.
-- Pure immutable descriptor reads cannot modify it. The actual dependency,
-- conflict, reentry and lane-head predicates stay in the live continuation.
def repeatedAdmission {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (request : Identity)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ) :
    Option Certificate × σ :=
  match read index with
  | none => (none, observer)
  | some row =>
    if row.identity = request then
      match laneStep index request.lane with
      | none => (none, observer)
      | some progress =>
        match read index, read index, read index, read index with
        | some nextRow, some checkedNextRow, some conflictRow, some dependencyRow =>
          if nextRow.next = checkedNextRow.next then
            match laneStep index request.lane with
            | none => (none, observer)
            | some checkedProgress =>
              if progress = checkedProgress then
                live { identity := row.identity, next := nextRow.next, dependency := dependencyRow.dependency, conflict := conflictRow.conflict }
                  { index, progressStep := progress, next := nextRow.next } observer
              else (none, observer)
          else (none, observer)
        | _, _, _, _ => (none, observer)
    else (none, observer)

def singleAdmission {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (request : Identity)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ) :
    Option Certificate × σ :=
  match read index with
  | none => (none, observer)
  | some row =>
    if row.identity = request then
      match laneStep index request.lane with
      | none => (none, observer)
      | some progress =>
        live row { index, progressStep := progress, next := row.next } observer
    else (none, observer)

theorem immutable_read_reuse_preserves_live_observation {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (request : Identity)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ) :
    repeatedAdmission read laneStep index request live observer =
      singleAdmission read laneStep index request live observer := by
  cases decoded : read index with
  | none => simp [repeatedAdmission, singleAdmission, decoded]
  | some row =>
    by_cases matched : row.identity = request
    · cases step : laneStep index request.lane <;>
        simp only [repeatedAdmission, singleAdmission, decoded, if_pos matched, step, ↓reduceIte] <;> rfl
    · simp [repeatedAdmission, singleAdmission, decoded, matched]

theorem missing_row_rejects_without_live_observation {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (request : Identity)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ)
    (absent : read index = none) :
    singleAdmission read laneStep index request live observer = (none, observer) := by
  simp [singleAdmission, absent]

theorem identity_mismatch_rejects_without_live_observation {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (request : Identity) (row : Row)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ)
    (decoded : read index = some row) (mismatch : row.identity ≠ request) :
    singleAdmission read laneStep index request live observer = (none, observer) := by
  simp [singleAdmission, decoded, mismatch]

-- No admission result is retained across operations. A later call always
-- consults its supplied live continuation, even with the same static row.
theorem later_admission_uses_its_current_live_check {σ : Type}
    (read : Nat → Option Row) (laneStep : Nat → Nat → Option Nat)
    (index : Nat) (row : Row) (progress : Nat)
    (live : Row → Certificate → σ → Option Certificate × σ) (observer : σ)
    (decoded : read index = some row) (step : laneStep index row.identity.lane = some progress) :
    singleAdmission read laneStep index row.identity live observer =
      live row { index, progressStep := progress, next := row.next } observer := by
  simp [singleAdmission, decoded, step]

#print axioms immutable_read_reuse_preserves_live_observation
#print axioms missing_row_rejects_without_live_observation
#print axioms identity_mismatch_rejects_without_live_observation
#print axioms later_admission_uses_its_current_live_check

end Hibana.ImmutableAdmission
