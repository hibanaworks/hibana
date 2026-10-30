import Std

namespace Hibana.OwnerJoin

abbrev Key := BitVec 24
abbrev Actor := BitVec 8

inductive Owner where
  | actor : Actor → Owner
  | initial : Key → Owner
  deriving DecidableEq

def meaning (environment : Key → Actor) : Owner → Actor
  | .actor actor => actor
  | .initial key => environment key

def normalize (known : Key → Option Actor) : Owner → Owner
  | .actor actor => .actor actor
  | .initial key => match known key with
    | some actor => .actor actor
    | none => .initial key

def Sound (environment : Key → Actor) (known : Key → Option Actor) : Prop :=
  ∀ key actor, known key = some actor → environment key = actor

theorem normalization_preserves_ownership (environment : Key → Actor)
    (known : Key → Option Actor) (sound : Sound environment known) (owner : Owner) :
    meaning environment (normalize known owner) = meaning environment owner := by
  cases owner with
  | actor actor => rfl
  | initial key =>
    cases result : known key with
    | none => simp [normalize, result]
    | some actor => simp [normalize, meaning, result, sound key actor result]

theorem equal_normalized_choices_preserve_both_arms (environment : Key → Actor)
    (known : Key → Option Actor) (sound : Sound environment known)
    (left right : Owner) (equal : normalize known left = normalize known right)
    (choice : Bool) :
    meaning environment (if choice then left else right) =
      meaning environment (normalize known left) := by
  cases choice with
  | false =>
    calc
      meaning environment right = meaning environment (normalize known right) :=
        (normalization_preserves_ownership environment known sound right).symm
      _ = meaning environment (normalize known left) := congrArg _ equal.symm
  | true => exact (normalization_preserves_ownership environment known sound left).symm

theorem distinct_owners_cannot_be_merged (environment : Key → Actor)
    (known : Key → Option Actor) (sound : Sound environment known)
    (left right : Owner) (different : meaning environment left ≠ meaning environment right) :
    normalize known left ≠ normalize known right := by
  intro equal
  apply different
  calc
    meaning environment left = meaning environment (normalize known left) :=
      (normalization_preserves_ownership environment known sound left).symm
    _ = meaning environment (normalize known right) := congrArg _ equal
    _ = meaning environment right := normalization_preserves_ownership environment known sound right

/-- Receivers record the actual actor captured at each transfer. Moving a
    receiving role later does not change an earlier entry in this trace. -/
def finalOwner (initial : Actor) (receivers : List Actor) : Actor :=
  receivers.foldl (fun _ receiver => receiver) initial

/-- A validated trace ending at its entry owner cannot leave any departure
    without a later return. This is the return obligation used when a fresh
    visit sends no other endpoint and thus seeds no irreversible dependency. -/
theorem restoring_suffix_contains_a_return (initial departed : Actor)
    (receivers : List Actor) (different : departed ≠ initial)
    (restored : finalOwner departed receivers = initial) : initial ∈ receivers := by
  induction receivers generalizing departed with
  | nil => exact (different restored).elim
  | cons receiver rest ih =>
    by_cases same : receiver = initial
    · simp [same]
    · exact List.mem_cons.mpr (Or.inr (ih receiver same restored))

end Hibana.OwnerJoin

#print axioms Hibana.OwnerJoin.normalization_preserves_ownership
#print axioms Hibana.OwnerJoin.equal_normalized_choices_preserve_both_arms
#print axioms Hibana.OwnerJoin.distinct_owners_cannot_be_merged
#print axioms Hibana.OwnerJoin.restoring_suffix_contains_a_return
