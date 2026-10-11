import Std

namespace Hibana.SortedRouteIndex

def capacity : Nat := 8192
inductive Result where
  | invalid
  | absent
  | found (slot : Nat)
  deriving DecidableEq, Repr

def step (query slot raw : Nat) (old : Result) : Result :=
  match old with
  | .invalid => .invalid
  | other =>
    if raw < capacity then
      if raw = query then
        match other with
        | .absent => .found slot
        | .found _ | .invalid => .invalid
      else other
    else .invalid

def scan (table : Nat → Nat) (query : Nat) : Nat → Result
  | 0 => .absent
  | n + 1 => step query n (table n) (scan table query n)

def Sorted (t : Nat → Nat) (n : Nat) : Prop :=
  ∀ i j, i < j → j < n → t i < t j

def Valid (t : Nat → Nat) (n : Nat) : Prop :=
  ∀ i, i < n → t i < capacity

def Spec (t : Nat → Nat) (n q : Nat) (r : Result) : Prop :=
  (r = .absent ∧ ∀ i, i < n → t i ≠ q) ∨
  ∃ i, r = .found i ∧ i < n ∧ t i = q

 theorem scan_spec (t : Nat → Nat) (n q : Nat) (hs : Sorted t n) (hv : Valid t n) :
    Spec t n q (scan t q n) := by
  induction n with
  | zero => exact Or.inl ⟨rfl, by intro i h; omega⟩
  | succ n ih =>
    have hs' : Sorted t n := by intro i j hij hj; exact hs i j hij (by omega)
    have hv' : Valid t n := by intro i hi; exact hv i (by omega)
    have vn := hv n (by omega)
    rcases ih hs' hv' with ⟨he, none⟩ | ⟨i, he, hi, hit⟩
    · by_cases hn : t n = q
      · have vq : q < capacity := by omega
        exact Or.inr ⟨n, by simp [scan, he, step, hn, vq], by omega, hn⟩
      · apply Or.inl
        refine ⟨by simp [scan, he, step, vn, hn], ?_⟩
        intro i hi
        by_cases e : i = n
        · simpa [e] using hn
        · exact none i (by omega)
    · have hn : t n ≠ q := by
        have order := hs i n hi (by omega)
        omega
      exact Or.inr ⟨i, by simp [scan, he, step, vn, hn], by omega, hit⟩

 theorem spec_unique (t : Nat → Nat) (n q : Nat) (hs : Sorted t n)
    (a b : Result) (ha : Spec t n q a) (hb : Spec t n q b) : a = b := by
  rcases ha with ⟨ae, an⟩ | ⟨i, ae, hi, hit⟩
  · rcases hb with ⟨be, _⟩ | ⟨j, _, hj, hjt⟩
    · simp [ae, be]
    · exact False.elim (an j hj hjt)
  · rcases hb with ⟨_, bn⟩ | ⟨j, be, hj, hjt⟩
    · exact False.elim (bn i hi hit)
    · have ij : i = j := by
        by_cases lt : i < j
        · have order := hs i j lt hj
          omega
        · by_cases gt : j < i
          · have order := hs j i gt hi
            omega
          · omega
      simp [ae, be, ij]

/-- A fuelled model of the source half-open binary-search loop. -/
def search (t : Nat → Nat) (q lo hi : Nat) : Nat → Result
  | 0 => .invalid
  | fuel + 1 =>
    if lo < hi then
      let mid := lo + (hi - lo) / 2
      if t mid < q then search t q (mid + 1) hi fuel
      else if q < t mid then search t q lo mid fuel
      else .found mid
    else .absent

 theorem search_spec (t : Nat → Nat) (n q fuel : Nat) (hs : Sorted t n)
    (lo hi : Nat) (bounds : lo ≤ hi ∧ hi ≤ n)
    (room : hi - lo < fuel)
    (inside : ∀ i, i < n → t i = q → lo ≤ i ∧ i < hi) :
    Spec t n q (search t q lo hi fuel) := by
  induction fuel generalizing lo hi with
  | zero => omega
  | succ fuel ih =>
    by_cases active : lo < hi
    · let mid := lo + (hi - lo) / 2
      have midbounds : lo ≤ mid ∧ mid < hi := by dsimp [mid]; omega
      have mn : mid < n := by omega
      by_cases right : t mid < q
      · have newinside : ∀ i, i < n → t i = q → mid + 1 ≤ i ∧ i < hi := by
          intro i hi' hit
          have prior := inside i hi' hit
          have after : mid < i := by
            by_cases before : i < mid
            · have order := hs i mid before mn
              omega
            · by_cases same : i = mid
              · subst i; omega
              · omega
          omega
        simpa [search, active, mid, right] using
          ih (mid + 1) hi (by omega) (by omega) newinside
      · by_cases left : q < t mid
        · have newinside : ∀ i, i < n → t i = q → lo ≤ i ∧ i < mid := by
            intro i hi' hit
            have prior := inside i hi' hit
            have before : i < mid := by
              by_cases after : mid < i
              · have order := hs mid i after hi'
                omega
              · by_cases same : i = mid
                · subst i; omega
                · omega
            omega
          simpa [search, active, mid, right, left] using
            ih lo mid (by omega) (by omega) newinside
        · exact Or.inr ⟨mid, by simp [search, active, mid, right, left], mn, by omega⟩
    · apply Or.inl
      refine ⟨by simp [search, active], ?_⟩
      intro i hi' hit
      have prior := inside i hi' hit
      omega

/-- Exactly the adjacent-row certificate the const constructor computes. -/
def certificate (t : Nat → Nat) : Nat → Bool
  | 0 => true
  | n + 1 => certificate t n && decide (t n < capacity) &&
      (decide (n = 0) || decide (t (n - 1) < t n))

 theorem certificate_sound (t : Nat → Nat) (n : Nat) (h : certificate t n = true) :
    Sorted t n ∧ Valid t n := by
  induction n with
  | zero => constructor <;> intro <;> omega
  | succ n ih =>
    simp only [certificate, Bool.and_eq_true, Bool.or_eq_true, decide_eq_true_eq] at h
    obtain ⟨oldSorted, oldValid⟩ := ih h.1.1
    constructor
    · intro i j hij hj
      by_cases jold : j < n
      · exact oldSorted i j hij jold
      · have je : j = n := by omega
        subst j
        have npos : 0 < n := by omega
        have adjacent : t (n - 1) < t n := by rcases h.2 with z | a; omega; exact a
        by_cases last : i = n - 1
        · simpa [last] using adjacent
        · have order := oldSorted i (n - 1) (by omega) (by omega)
          omega
    · intro i hi
      by_cases old : i < n
      · exact oldValid i old
      · have same : i = n := by omega
        simpa [same] using h.1.2

 theorem binary_equals_scan (t : Nat → Nat) (n q : Nat)
    (h : certificate t n = true) :
    search t q 0 n (n + 1) = scan t q n := by
  obtain ⟨hs, hv⟩ := certificate_sound t n h
  apply spec_unique t n q hs
  · exact search_spec t n q (n + 1) hs 0 n (by omega) (by omega)
      (by intro i hi _; omega)
  · exact scan_spec t n q hs hv

def indexed (t : Nat → Nat) (n q : Nat) : Result :=
  if certificate t n then search t q 0 n (n + 1) else scan t q n

 theorem indexed_equals_scan (t : Nat → Nat) (n q : Nat) :
    indexed t n q = scan t q n := by
  unfold indexed
  split
  · exact binary_equals_scan t n q ‹_›
  · rfl

example : certificate (fun i => i + 1) 34 = true := by decide
example : indexed (fun i => i + 1) 34 17 = .found 16 := by decide
example : indexed (fun i => 2 * i + 1) 34 18 = .absent := by decide
example : indexed (fun i => 2 * i + 1) 34 8192 = .absent := by decide
example : indexed (fun i => 2 * i + 1) 34 65535 = .absent := by decide
example : indexed id 0 0 = .absent := by decide
example : certificate (fun _ => 0) 2 = false := by decide
example : indexed (fun _ => 0) 2 0 = .invalid := by decide
example : indexed (fun _ => 0) 2 1 = .absent := by decide
example : indexed (fun _ => 8192) 1 0 = .invalid := by decide
example : indexed (fun i => 1 - i) 2 0 = .found 1 := by decide

#print axioms indexed_equals_scan
#print axioms binary_equals_scan
end Hibana.SortedRouteIndex
