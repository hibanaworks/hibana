import Std

namespace Hibana.ReceiveIndex

/- Model of the half-open lower-bound search over immutable receive keys.
   Sorting, ordinal coverage and row decoding are checked by the Rust image
   constructor and independent decoded-row tests. Live admission is unchanged. -/
def lower (key : Nat → Nat) (query lo hi : Nat) : Nat → Nat
  | 0 => lo
  | fuel + 1 =>
      if lo < hi then
        let mid := lo + (hi - lo) / 2
        if key mid < query then lower key query (mid + 1) hi fuel
        else lower key query lo mid fuel
      else lo

def Ordered (key : Nat → Nat) (n : Nat) : Prop :=
  ∀ i j, i ≤ j → j < n → key i ≤ key j

theorem lower_partition (key : Nat → Nat) (n query fuel : Nat)
    (ordered : Ordered key n) (lo hi : Nat)
    (bounds : lo ≤ hi ∧ hi ≤ n) (room : hi - lo < fuel)
    (prior : ∀ i, i < lo → key i < query)
    (suffix : ∀ i, hi ≤ i → i < n → query ≤ key i) :
    let result := lower key query lo hi fuel
    lo ≤ result ∧ result ≤ hi ∧
      ∀ i, i < n → (key i < query ↔ i < result) := by
  induction fuel generalizing lo hi with
  | zero =>
      have impossible : False := by omega
      exact False.elim impossible
  | succ fuel ih =>
      rcases Nat.lt_or_ge lo hi with active | inactive
      · let mid := lo + (hi - lo) / 2
        have middle : lo ≤ mid ∧ mid < hi := by constructor <;> (dsimp [mid]; omega)
        rcases Nat.lt_or_ge (key mid) query with right | left
        · have nextPrefix : ∀ i, i < mid + 1 → key i < query := by
            intro i h
            have hle : key i ≤ key mid := ordered i mid (by omega) (by omega)
            omega
          have result := ih (mid + 1) hi (by constructor <;> omega)
            (by omega) nextPrefix suffix
          dsimp only [mid] at result
          simp only [lower, active, mid, right, ↓reduceIte]
          exact ⟨by omega, result.2⟩
        · have right : ¬key mid < query := by omega
          have nextSuffix : ∀ i, mid ≤ i → i < n → query ≤ key i := by
            intro i h hin
            have hle := ordered mid i h hin
            omega
          have result := ih lo mid (by constructor <;> omega) (by omega) prior nextSuffix
          dsimp only [mid] at result
          simp only [lower, active, mid, right, ↓reduceIte]
          exact ⟨result.1, by omega, result.2.2⟩
      · have active : ¬lo < hi := by omega
        simp only [lower, active, ↓reduceIte]
        refine ⟨by omega, by omega, ?_⟩
        intro i hin
        constructor
        · intro h
          rcases Nat.lt_or_ge i lo with before | outside
          · exact before
          · have hs := suffix i (by omega) hin
            omega
        · exact prior i

theorem lower_exact (key : Nat → Nat) (n query : Nat) (ordered : Ordered key n) :
    lower key query 0 n (n + 1) ≤ n ∧
      ∀ i, i < n → (key i < query ↔ i < lower key query 0 n (n + 1)) := by
  have result := lower_partition key n query (n + 1) ordered 0 n
    (by constructor <;> omega) (by omega) (by intro i h; omega) (by intro i h hn; omega)
  exact result.2

/-- Every matching key, including duplicates, is retained; no other key enters. -/
theorem candidate_range_exact (key : Nat → Nat) (n query i : Nat)
    (ordered : Ordered key n) (inside : i < n) :
    (lower key query 0 n (n + 1) ≤ i ∧
      i < lower key (query + 1) 0 n (n + 1)) ↔ key i = query := by
  have left := (lower_exact key n query ordered).2 i inside
  have right := (lower_exact key n (query + 1) ordered).2 i inside
  constructor
  · intro range
    have notBefore : ¬key i < query := fun h => Nat.not_lt_of_ge range.1 (left.mp h)
    have beforeUpper := right.mpr range.2
    omega
  · intro same
    constructor
    · apply Nat.le_of_not_gt
      intro below
      have conflict := left.mpr below
      omega
    · exact right.mp (by omega)

/-- Narrowing candidates preserves any subsequent ownership/branch/roll check. -/
theorem live_candidates_exact (key : Nat → Nat) (n query i : Nat)
    (ordered : Ordered key n) (inside : i < n) (admitted : Prop) :
    (lower key query 0 n (n + 1) ≤ i ∧
      i < lower key (query + 1) 0 n (n + 1) ∧ admitted) ↔
      key i = query ∧ admitted := by
  have exactRange := candidate_range_exact key n query i ordered inside
  constructor
  · rintro ⟨a, b, live⟩
    exact ⟨exactRange.mp ⟨a, b⟩, live⟩
  · rintro ⟨same, live⟩
    obtain ⟨a, b⟩ := exactRange.mpr same
    exact ⟨a, b, live⟩

/-- The immutable key binds both message fields, without depending on schema. -/
theorem message_key_exact (role label wantedRole wantedLabel : Nat)
    (labelBound : label < 256) (wantedBound : wantedLabel < 256) :
    role * 256 + label = wantedRole * 256 + wantedLabel ↔
      role = wantedRole ∧ label = wantedLabel := by
  constructor
  · intro same
    constructor <;> omega
  · rintro ⟨rfl, rfl⟩
    rfl

/-- Elimination preserves both acceptance and a legitimate wait for another
view. Rust tests check both rejection conditions against full admission. -/
theorem completion_prefilter_preserves_admission
    (repeatable completed admitted otherViewPending : Bool)
    (disabled : repeatable = false → completed = true →
      admitted = false ∧ otherViewPending = false) :
    (((repeatable || !completed) && admitted),
      ((repeatable || !completed) && otherViewPending)) = (admitted, otherViewPending) := by
  cases repeatable <;> cases completed <;> simp_all

#print axioms lower_partition
#print axioms candidate_range_exact
#print axioms live_candidates_exact
#print axioms message_key_exact
#print axioms completion_prefilter_preserves_admission

end Hibana.ReceiveIndex
