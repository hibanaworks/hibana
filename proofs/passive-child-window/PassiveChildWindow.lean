import Std

namespace Hibana.PassiveChildWindow

/- The source helper's half-open binary lower-bound loop. Duplicate offsets
   are allowed; selecting only one equal-offset marker would be incorrect. -/
def Sorted (t : Nat → Nat) (n : Nat) : Prop :=
  ∀ i j, i ≤ j → j < n → t i ≤ t j

def LowerBound (t : Nat → Nat) (q n b : Nat) : Prop :=
  b ≤ n ∧ (∀ i, i < b → t i < q) ∧ (∀ i, b ≤ i → i < n → q ≤ t i)

def search (t : Nat → Nat) (q lo hi : Nat) : Nat → Nat
  | 0 => lo
  | fuel + 1 =>
      if lo < hi then
        let mid := lo + (hi - lo) / 2
        if t mid < q then search t q (mid + 1) hi fuel
        else search t q lo mid fuel
      else lo

theorem search_lower_bound (t : Nat → Nat) (q n fuel : Nat)
    (sorted : Sorted t n) (lo hi : Nat)
    (bounds : lo ≤ hi ∧ hi ≤ n) (room : hi - lo < fuel)
    (before : ∀ i, i < lo → t i < q)
    (after : ∀ i, hi ≤ i → i < n → q ≤ t i) :
    LowerBound t q n (search t q lo hi fuel) := by
  induction fuel generalizing lo hi with
  | zero => omega
  | succ fuel ih =>
      by_cases active : lo < hi
      · let mid := lo + (hi - lo) / 2
        have midbounds : lo ≤ mid ∧ mid < hi := by dsimp [mid]; omega
        by_cases right : t mid < q
        · have before' : ∀ i, i < mid + 1 → t i < q := by
            intro i hi'
            by_cases old : i < lo
            · exact before i old
            · have order := sorted i mid (by omega) (by omega)
              omega
          simpa [search, active, mid, right] using
            ih (mid + 1) hi (by omega) (by omega) before' after
        · have after' : ∀ i, mid ≤ i → i < n → q ≤ t i := by
            intro i hi' hin
            have order := sorted mid i hi' hin
            omega
          simpa [search, active, mid, right] using
            ih lo mid (by omega) (by omega) before after'
      · have same : hi = lo := by omega
        simpa [search, active, LowerBound, same] using
          And.intro (show lo ≤ n by omega) (And.intro before after)

theorem initial_search_lower_bound (t : Nat → Nat) (q n : Nat)
    (sorted : Sorted t n) :
    LowerBound t q n (search t q 0 n (n + 1)) := by
  exact search_lower_bound t q n (n + 1) sorted 0 n
    (by omega) (by omega) (by intro i hi; omega) (by intro i hi hin; omega)

theorem no_equal_marker_skipped (t : Nat → Nat) (q n b i : Nat)
    (bound : LowerBound t q n b) (hin : i < n) (same : t i = q) : b ≤ i := by
  by_cases h : b ≤ i
  · exact h
  · have bad := bound.2.1 i (by omega)
    omega

theorem first_greater_ends_equal_group (t : Nat → Nat) (q n j i : Nat)
    (sorted : Sorted t n) (greater : q < t j) (later : j ≤ i) (hin : i < n) :
    t i ≠ q := by
  have order := sorted j i later hin
  omega

/- Preserve the exact stable fold over candidates. The state and visit function
   are arbitrary, so this includes the source's outermost tie selection and an
   error/invalid state: no commutativity, uniqueness, or tree assumption is used. -/
def visitAt {α σ : Type} (offset : α → Nat) (q : Nat)
    (visit : σ → α → σ) (state : σ) (row : α) : σ :=
  if offset row = q then visit state row else state

theorem fold_skips_unmatched {α σ : Type} (offset : α → Nat) (q : Nat)
    (visit : σ → α → σ) (rows : List α) (state : σ)
    (unmatched : ∀ row ∈ rows, offset row ≠ q) :
    rows.foldl (visitAt offset q visit) state = state := by
  induction rows with
  | nil => rfl
  | cons row tail ih =>
      have head : offset row ≠ q := unmatched row (by simp)
      have rest : ∀ x ∈ tail, offset x ≠ q := by
        intro x hx
        exact unmatched x (by simp [hx])
      simpa [List.foldl, visitAt, head] using ih rest

theorem stable_window_exact {α σ : Type} (offset : α → Nat) (q : Nat)
    (visit : σ → α → σ) (pre window suffix : List α) (state : σ)
    (before : ∀ row ∈ pre, offset row < q)
    (after : ∀ row ∈ suffix, q < offset row) :
    (pre ++ window ++ suffix).foldl (visitAt offset q visit) state =
      window.foldl (visitAt offset q visit) state := by
  rw [List.foldl_append, List.foldl_append]
  rw [fold_skips_unmatched offset q visit pre state
    (by intro row hr; have h := before row hr; omega)]
  exact fold_skips_unmatched offset q visit suffix _
    (by intro row hr; have h := after row hr; omega)

-- Equal-offset ties must all survive; an empty or absent group also works.
example : search (fun i => i / 3) 2 0 12 13 = 6 := by decide
example : search (fun _ => 4) 4 0 8 9 = 0 := by decide
example : search (fun _ => 4) 5 0 8 9 = 8 := by decide
example : search id 0 0 0 1 = 0 := by decide

#print axioms initial_search_lower_bound
#print axioms no_equal_marker_skipped
#print axioms first_greater_ends_equal_group
#print axioms stable_window_exact

end Hibana.PassiveChildWindow
