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

end Hibana.RowPrefix

#print axioms Hibana.RowPrefix.prefix_decomposition
#print axioms Hibana.RowPrefix.range_count_exact
