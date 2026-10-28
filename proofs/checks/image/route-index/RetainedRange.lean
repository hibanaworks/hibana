import Std

namespace Hibana.RetainedRange

structure Roll where
  first : Nat
  finish : Nat
  retained : Bool

def contains (roll : Roll) (index : Nat) : Prop :=
  roll.first ≤ index ∧ index < roll.finish

def qualifies (roll : Roll) (predecessor target : Nat) : Prop :=
  roll.retained = true ∧ contains roll predecessor ∧ ¬contains roll target

def allows (rolls : List Roll) (predecessor target : Nat) : Prop :=
  ∃ roll ∈ rolls, qualifies roll predecessor target

-- The production query selects the greatest end among qualifying rolls.
-- The skip law needs only a qualifying witness, so it also covers equal ends
-- and requires no extra laminarity premise beyond the independently sealed
-- source interval facts used to construct a witness.
theorem qualifying_suffix (roll : Roll) (predecessor target next : Nat)
    (qualified : qualifies roll predecessor target)
    (later : predecessor ≤ next) (inside : next < roll.finish) :
    qualifies roll next target := by
  rcases qualified with ⟨retained, ⟨first, _⟩, outside⟩
  exact ⟨retained, ⟨Nat.le_trans first later, inside⟩, outside⟩

def complete (rolls : List Roll) (done : Nat → Prop) (target first limit : Nat) : Prop :=
  ∀ index, first ≤ index → index < limit → allows rolls index target ∨ done index

theorem skip_exact (rolls : List Roll) (done : Nat → Prop)
    (roll : Roll) (predecessor target limit : Nat)
    (member : roll ∈ rolls) (qualified : qualifies roll predecessor target)
    (within : predecessor < limit) :
    complete rolls done target predecessor limit ↔
      complete rolls done target (min roll.finish limit) limit := by
  have before : predecessor < roll.finish := qualified.2.1.2
  have lower : predecessor ≤ min roll.finish limit :=
    Nat.le_min.mpr ⟨Nat.le_of_lt before, Nat.le_of_lt within⟩
  constructor
  · intro original index after below
    exact original index (Nat.le_trans lower after) below
  · intro remaining index after below
    by_cases inside : index < roll.finish
    · exact Or.inl ⟨roll, member, qualifying_suffix roll predecessor target index qualified after inside⟩
    · have finishBefore : roll.finish ≤ index := by omega
      exact remaining index (Nat.le_trans (Nat.min_le_left _ _) finishBefore) below

theorem own_visit_cannot_qualify (roll : Roll) (predecessor target : Nat)
    (inside : contains roll target) : ¬qualifies roll predecessor target := by
  intro qualified
  exact qualified.2.2 inside

end Hibana.RetainedRange
