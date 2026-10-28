import Std

namespace Hibana.ScopeParent

/-- Active route ends decrease from outer to inner. Covering routes therefore
    form a prefix, including routes with equal source ranges. -/
def Partition (covers : Nat → Bool) (size cut : Nat) : Prop :=
  cut ≤ size ∧
  (∀ index, index < cut → covers index = true) ∧
  (∀ index, cut ≤ index → index < size → covers index = false)

def search (covers : Nat → Bool) : Nat → Nat → Nat → Nat
  | 0, low, _ => low
  | fuel + 1, low, high =>
      if low < high then
        let middle := low + (high - low) / 2
        if covers middle then search covers fuel (middle + 1) high
        else search covers fuel low middle
      else low

/-- The compiler's binary search returns the complete covering prefix. Fuel
    bounds the mathematical recursion; Rust uses the same shrinking interval. -/
theorem search_exact (covers : Nat → Bool) (size cut fuel low high : Nat)
    (partition : Partition covers size cut)
    (bracket : low ≤ cut ∧ cut ≤ high) (inside : high ≤ size)
    (budget : high - low ≤ fuel) : search covers fuel low high = cut := by
  induction fuel generalizing low high with
  | zero =>
      have equal : low = cut := by omega
      simp [search, equal]
  | succ fuel ih =>
      by_cases active : low < high
      · let middle := low + (high - low) / 2
        have lower : low ≤ middle := by omega
        have upper : middle < high := by omega
        by_cases covered : covers middle = true
        · have before : middle < cut := by
            rcases Nat.lt_or_ge middle cut with before | after
            · exact before
            · have miss := partition.2.2 middle after (by omega)
              simp [covered] at miss
          simp only [search, active, ↓reduceIte, middle, covered]
          exact ih (middle + 1) high ⟨by omega, bracket.2⟩ inside (by omega)
        · have after : cut ≤ middle := by
            rcases Nat.lt_or_ge middle cut with before | after
            · exact False.elim (covered (partition.2.1 middle before))
            · exact after
          simp only [search, active, ↓reduceIte, middle, covered, Bool.false_eq_true]
          exact ih low middle ⟨bracket.1, after⟩ (by omega) (by omega)
      · have equal : low = cut := by omega
        simp only [search, active, ↓reduceIte]
        exact equal

theorem complete_search_exact (covers : Nat → Bool) (size cut : Nat)
    (partition : Partition covers size cut) :
    search covers size 0 size = cut :=
  search_exact covers size cut size 0 size partition ⟨Nat.zero_le cut, partition.1⟩
    (Nat.le_refl size) (by omega)

/-- Searching to the current marker misses an inner route that begins at the
    same offset. The whole marker group must be collected before querying. -/
example : search (fun _ => true) 0 0 0 = 0 ∧
    search (fun _ => true) 1 0 1 = 1 := by decide

end Hibana.ScopeParent

#print axioms Hibana.ScopeParent.search_exact
#print axioms Hibana.ScopeParent.complete_search_exact
