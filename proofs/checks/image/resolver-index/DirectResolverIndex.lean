import Std

namespace Hibana.DirectResolverIndex

def insert (index : Nat → Option Nat) (key row : Nat) : Nat → Option Nat :=
  fun query => if query = key then
    match index query with
    | none => some row
    | some prior => some prior
  else index query

def build (table : Nat → Nat) : Nat → Nat → Option Nat
  | 0 => fun _ => none
  | n + 1 => insert (build table n) (table n) n

/-- Independent first-match specification, with no sortedness assumption. -/
def scan (table : Nat → Nat) (query : Nat) : Nat → Option Nat
  | 0 => none
  | n + 1 => match scan table query n with
    | some prior => some prior
    | none => if table n = query then some n else none

theorem construction_matches_scan (table : Nat → Nat) (n query : Nat) :
    build table n query = scan table query n := by
  induction n with
  | zero => rfl
  | succ n ih =>
    simp only [build, insert, scan, ih]
    cases previous : scan table query n <;> simp [previous, eq_comm]

theorem no_match_is_absent (table : Nat → Nat) (n query : Nat) :
    (∀ row, row < n → table row ≠ query) → scan table query n = none := by
  induction n with
  | zero => intro _; rfl
  | succ n ih =>
    intro missing
    have previous := ih (by intro row bound; exact missing row (by omega))
    simp [scan, previous, missing n (by omega)]

def lookup (index : Nat → Option Nat) (extent query : Nat) : Option Nat :=
  if query < extent then index query else none

theorem direct_lookup_matches_scan (table : Nat → Nat) (n extent query : Nat)
    (bounds : ∀ row, row < n → table row < extent) :
    lookup (build table n) extent query = scan table query n := by
  by_cases inside : query < extent
  · simp [lookup, inside, construction_matches_scan]
  · have missing : ∀ row, row < n → table row ≠ query := by
      intro row bound
      have := bounds row bound
      omega
    simp [lookup, inside, no_match_is_absent table n query missing]

theorem later_duplicates_preserve_the_first_row (index : Nat → Option Nat)
    (key row first : Nat) (present : index key = some first) :
    insert index key row key = some first := by
  simp [insert, present]

theorem wrong_kind_is_absent (index : Nat → Option Nat) (extent query : Nat)
    (bound : extent ≤ 8192) (wrong : 8192 ≤ query) :
    lookup index extent query = none := by
  have outside : ¬ query < extent := by omega
  simp [lookup, outside]

private def unsorted (row : Nat) : Nat := if row = 0 then 31 else if row = 1 then 1 else 7
private def duplicates (row : Nat) : Nat := if row < 2 then 1 else 7
example : lookup (build unsorted 3) 32 31 = some 0 := by decide
example : lookup (build unsorted 3) 32 1 = some 1 := by decide
example : lookup (build unsorted 3) 32 7 = some 2 := by decide
example : lookup (build unsorted 3) 32 0 = none := by decide
example : lookup (build unsorted 3) 32 32 = none := by decide
example : lookup (build duplicates 3) 8 1 = some 0 := by decide
example : lookup (build duplicates 3) 8 7 = some 2 := by decide
example : lookup (build duplicates 3) 8 8192 = none := by decide

#print axioms construction_matches_scan
#print axioms direct_lookup_matches_scan
#print axioms later_duplicates_preserve_the_first_row
#print axioms wrong_kind_is_absent
end Hibana.DirectResolverIndex
