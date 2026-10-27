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

-- Scope selection reads one frozen kernel cut. Its memoized previews are pure
-- queries of that cut; candidate filtering performs no transport or commit.
def admitThenSelect {α β : Type} (admit : α → Bool) (candidate : α → Option β) :
    List α → Option β
  | [] => none
  | row :: tail =>
    if admit row then
      match candidate row with
      | some selected => some selected
      | none => admitThenSelect admit candidate tail
    else admitThenSelect admit candidate tail

def selectThenAdmit {α β : Type} (admit : α → Bool) (candidate : α → Option β) :
    List α → Option β
  | [] => none
  | row :: tail =>
    match candidate row with
    | none => selectThenAdmit admit candidate tail
    | some selected =>
      if admit row then some selected else selectThenAdmit admit candidate tail

theorem scope_filter_before_admission_preserves_first_candidate {α β : Type}
    (rows : List α) (admit : α → Bool) (candidate : α → Option β) :
    admitThenSelect admit candidate rows = selectThenAdmit admit candidate rows := by
  induction rows with
  | nil => rfl
  | cons row tail induction =>
    cases decision : admit row <;> cases selected : candidate row <;>
      simp [admitThenSelect, selectThenAdmit, decision, selected, induction]

theorem excluded_context_needs_no_admission {α β : Type}
    (rows : List α) (admit excluded : α → Bool) (candidate : α → Option β)
    (irrelevant : ∀ row, excluded row = true → candidate row = none) :
    selectThenAdmit admit candidate rows =
      selectThenAdmit (fun row => !excluded row && admit row) candidate rows := by
  induction rows with
  | nil => rfl
  | cons row tail induction =>
    cases ignored : excluded row with
    | true =>
      have absent := irrelevant row ignored
      simp [selectThenAdmit, absent, induction]
    | false =>
      cases selected : candidate row <;> simp [selectThenAdmit, ignored, selected, induction]

#print axioms scope_filter_before_admission_preserves_first_candidate
#print axioms excluded_context_needs_no_admission

end Hibana.ImmutableAdmission
