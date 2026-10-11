import Std

/-
The table maps a zero-based route slot to the complete decoded little-endian
u16 raw scope ID. Route scopes occupy precisely [0, 8192). This model includes
malformed raw IDs and duplicate-match failure, not just successful lookups.
-/
namespace Hibana.RouteIndex

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

/-- Same left-to-right scan and uniqueness check as route_scope_slot. -/
def scan (table : Nat → Nat) (query : Nat) : Nat → Result
  | 0 => .absent
  | n + 1 => step query n (table n) (scan table query n)

/-- Raw scope ordinal is the index; wrong-kind and absent encodings are >=8192. -/
def direct (query count : Nat) : Result :=
  if query < count then .found query else .absent

/-- The implementation performs this check once over immutable descriptor bytes. -/
def checkRows (table : Nat → Nat) : Nat → Bool
  | 0 => true
  | n + 1 => checkRows table n && (table n == n)

def certificate (table : Nat → Nat) (count : Nat) : Bool :=
  decide (count ≤ capacity) && checkRows table count

 theorem checkRows_sound (table : Nat → Nat) (count : Nat)
    (h : checkRows table count = true) : ∀ i, i < count → table i = i := by
  induction count with
  | zero => omega
  | succ n ih =>
    simp only [checkRows, Bool.and_eq_true, beq_iff_eq] at h
    intro i hi
    by_cases e : i = n
    · simpa [e] using h.2
    · exact ih h.1 i (by omega)

 theorem scan_dense (table : Nat → Nat) (count query : Nat)
    (bound : count ≤ capacity)
    (dense : ∀ i, i < count → table i = i) :
    scan table query count = direct query count := by
  induction count with
  | zero => simp [scan, direct]
  | succ n ih =>
    have smaller : n ≤ capacity := by omega
    have denseSmaller : ∀ i, i < n → table i = i := by
      intro i hi
      exact dense i (by omega)
    have current : table n = n := dense n (by omega)
    have valid : n < capacity := by omega
    rw [scan, current, ih smaller denseSmaller]
    unfold step direct
    by_cases before : query < n
    · simp only [before, ↓reduceIte]
      have different : n ≠ query := by omega
      have within : query < n + 1 := by omega
      simp [valid, different, within]
    · simp only [before, ↓reduceIte]
      by_cases equal : n = query
      · subst query
        simp [valid]
      · have outside : ¬ query < n + 1 := by omega
        simp [valid, equal, outside]

 theorem certified_scan_equals_direct (table : Nat → Nat) (count query : Nat)
    (h : certificate table count = true) :
    scan table query count = direct query count := by
  simp only [certificate, Bool.and_eq_true, decide_eq_true_eq] at h
  exact scan_dense table count query h.1 (checkRows_sound table count h.2)

/-- A failed certificate does not admit anything: it retains the original scan. -/
def indexed (table : Nat → Nat) (count query : Nat) : Result :=
  if certificate table count then direct query count else scan table query count

 theorem indexed_equals_scan (table : Nat → Nat) (count query : Nat) :
    indexed table count query = scan table query count := by
  unfold indexed
  split
  · exact (certified_scan_equals_direct table count query ‹_›).symm
  · rfl

 theorem wrong_kind_or_absent_is_rejected (table : Nat → Nat) (count query : Nat)
    (h : certificate table count = true) (wrong : capacity ≤ query) :
    indexed table count query = .absent := by
  have bounds : count ≤ capacity := by
    have parts := h
    simp only [certificate, Bool.and_eq_true, decide_eq_true_eq] at parts
    exact parts.1
  simp [indexed, h, direct, show ¬ query < count by omega]

-- Positive and negative nonvacuity witnesses evaluated by the kernel.
example : certificate id 34 = true := by decide
example : indexed id 34 17 = .found 17 := by decide
example : indexed id 34 34 = .absent := by decide
example : indexed id 34 8192 = .absent := by decide
example : indexed id 34 65535 = .absent := by decide
example : indexed id 0 0 = .absent := by decide
example : certificate (fun _ => 0) 2 = false := by decide
example : indexed (fun _ => 0) 2 0 = .invalid := by decide
example : indexed (fun _ => 0) 2 1 = .absent := by decide
example : indexed (fun _ => 8192) 1 0 = .invalid := by decide
example : indexed (fun i => 1 - i) 2 0 = .found 1 := by decide
example : indexed (fun i => i + 7) 2 8 = .found 1 := by decide

#print axioms indexed_equals_scan
#print axioms certified_scan_equals_direct
end Hibana.RouteIndex
