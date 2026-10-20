-- Scoped model: reporting the first failed obligation never changes acceptance.
-- No claim is made here that this is a verification of the Rust lowering code.
def firstFailure : List Bool → Option Nat
  | [] => none
  | b :: rest => if b then (firstFailure rest).map Nat.succ else some 0

theorem no_failure_iff (xs : List Bool) :
    firstFailure xs = none ↔ ∀ b ∈ xs, b = true := by
  induction xs with
  | nil => simp [firstFailure]
  | cons b rest ih => cases b <;> simp [firstFailure, ih]

theorem witness_in_bounds (xs : List Bool) (n : Nat)
    (h : firstFailure xs = some n) : n < xs.length := by
  induction xs generalizing n with
  | nil => simp [firstFailure] at h
  | cons b rest ih =>
    cases b with
    | false => simp [firstFailure] at h; subst n; simp
    | true =>
      cases hr : firstFailure rest with
      | none => simp [firstFailure, hr] at h
      | some k =>
        simp [firstFailure, hr] at h
        subst n
        exact Nat.succ_lt_succ (ih k hr)

theorem witness_is_failure (xs : List Bool) (n : Nat)
    (h : firstFailure xs = some n) : xs[n]? = some false := by
  induction xs generalizing n with
  | nil => simp [firstFailure] at h
  | cons b rest ih =>
    cases b with
    | false => simp [firstFailure] at h; subst n; rfl
    | true =>
      cases hr : firstFailure rest with
      | none => simp [firstFailure, hr] at h
      | some k =>
        simp [firstFailure, hr] at h
        subst n
        exact ih k hr
