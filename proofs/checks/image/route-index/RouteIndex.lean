import Std

namespace Hibana.DirectRouteIndex

def absent : Nat := 65535

def lookup (index : Nat → Nat) (extent query : Nat) : Option Nat :=
  if query < extent then
    if index query = absent then none else some (index query)
  else none

structure Coherent (routes index : Nat → Nat) (count extent : Nat) : Prop where
  extent_bound : extent ≤ 8192
  forward : ∀ slot, slot < count →
    routes slot < extent ∧ index (routes slot) = slot ∧ slot < absent
  backward : ∀ query, query < extent → index query ≠ absent →
    index query < count ∧ routes (index query) = query

theorem lookup_exact (routes index : Nat → Nat) (count extent query slot : Nat)
    (valid : Coherent routes index count extent) :
    lookup index extent query = some slot ↔ slot < count ∧ routes slot = query := by
  constructor
  · intro found
    unfold lookup at found
    split at found
    · rename_i inside
      split at found
      · contradiction
      · rename_i present
        have same : index query = slot := Option.some.inj found
        have result := valid.backward query inside present
        simpa [same] using result
    · contradiction
  · rintro ⟨within, source⟩
    obtain ⟨inside, stored, bound⟩ := valid.forward slot within
    rw [source] at inside stored
    have present : slot ≠ absent := by omega
    simp [lookup, inside, stored, present]

theorem wrong_kind_is_absent (routes index : Nat → Nat) (count extent query : Nat)
    (valid : Coherent routes index count extent) (wrong : 8192 ≤ query) :
    lookup index extent query = none := by
  have outside : ¬ query < extent := by have := valid.extent_bound; omega
  simp [lookup, outside]

theorem unique_slot (routes index : Nat → Nat) (count extent left right : Nat)
    (valid : Coherent routes index count extent)
    (hl : left < count) (hr : right < count) (same : routes left = routes right) :
    left = right := by
  have l := (valid.forward left hl).2.1
  have r := (valid.forward right hr).2.1
  rw [same] at l
  omega

end Hibana.DirectRouteIndex

namespace Hibana.DirectRouteIndex
private def sparse (raw : Nat) : Nat :=
  if raw = 7 then 0 else if raw = 0 then 1 else if raw = 3 then 2 else absent
example : lookup sparse 8 7 = some 0 := by decide
example : lookup sparse 8 0 = some 1 := by decide
example : lookup sparse 8 3 = some 2 := by decide
example : lookup sparse 8 2 = none := by decide
example : lookup sparse 8 8 = none := by decide
example : lookup sparse 8 8192 = none := by decide
example : lookup sparse 8 65535 = none := by decide
end Hibana.DirectRouteIndex
