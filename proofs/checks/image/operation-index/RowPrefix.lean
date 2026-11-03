import Std

namespace Hibana.RowPrefix

def countBefore : List Nat → Nat → Nat
  | [], _ => 0
  | _ :: _, 0 => 0
  | count :: rest, boundary + 1 => count + countBefore rest boundary

def interval : List Nat → Nat → Nat → Nat
  | [], _, _ => 0
  | counts, 0, stop => countBefore counts stop
  | _ :: rest, start + 1, stop + 1 => interval rest start stop
  | _, _, 0 => 0

/-- Cumulative local-row counts split exactly at every source boundary. -/
theorem prefix_decomposition (counts : List Nat) (start stop : Nat)
    (ordered : start ≤ stop) :
    countBefore counts start + interval counts start stop = countBefore counts stop := by
  induction counts generalizing start stop with
  | nil => simp [countBefore, interval]
  | cons count rest ih =>
      cases start with
      | zero => simp [countBefore, interval]
      | succ start =>
          cases stop with
          | zero => omega
          | succ stop =>
              have equality := ih start stop (by omega)
              simpa only [countBefore, interval, Nat.add_assoc] using
                congrArg (fun total => count + total) equality

/-- Image construction can replace repeated counting by two prefix reads and
    subtraction. This includes empty intervals and the final source boundary. -/
theorem range_count_exact (counts : List Nat) (start stop : Nat)
    (ordered : start ≤ stop) :
    countBefore counts stop - countBefore counts start = interval counts start stop := by
  have equality := prefix_decomposition counts start stop ordered
  omega

/-- Checked monotone prefixes cannot wrap a compact subtraction. -/
theorem compact_count_bounds (first stop : Nat)
    (bound : stop < 65536) : stop - first ≤ stop ∧ stop - first < 65536 :=
  ⟨Nat.sub_le stop first, Nat.lt_of_le_of_lt (Nat.sub_le stop first) bound⟩

def packedStart (first stop : Nat) : Nat := if stop - first = 0 then 0 else first

theorem nonempty_packed_range_has_exact_end (first stop : Nat) (ordered : first < stop) :
    packedStart first stop + (stop - first) = stop := by
  simp [packedStart, show stop - first ≠ 0 by omega]
  omega

theorem empty_packed_range_is_canonical (first : Nat) : packedStart first first = 0 := by
  simp [packedStart]

end Hibana.RowPrefix

#print axioms Hibana.RowPrefix.prefix_decomposition
#print axioms Hibana.RowPrefix.range_count_exact
#print axioms Hibana.RowPrefix.compact_count_bounds
#print axioms Hibana.RowPrefix.nonempty_packed_range_has_exact_end
#print axioms Hibana.RowPrefix.empty_packed_range_is_canonical
