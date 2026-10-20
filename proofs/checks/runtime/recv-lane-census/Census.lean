import Std

namespace Hibana.RecvLaneCensus

/- A row's eligibility is the existing, unchanged label/schema/origin and
   event_enabled predicate evaluated before transport polling. This model
   covers the ephemeral lane census, not Rust decoding or physical I/O. -/
structure Row where
  lane : Nat
  eligible : Bool

def hit (row : Row) (lane : Nat) : Bool :=
  row.eligible && decide (row.lane = lane)

def insert (lanes : Nat → Bool) (row : Row) : Nat → Bool :=
  fun lane => lanes lane || hit row lane

def census (rows : List Row) : Nat → Bool :=
  rows.foldl insert (fun _ => false)

theorem fold_exact (rows : List Row) (initial : Nat → Bool) (lane : Nat) :
    (rows.foldl insert initial) lane =
      (initial lane || rows.any (fun row => hit row lane)) := by
  induction rows generalizing initial with
  | nil => simp
  | cons row tail ih =>
      simp only [List.foldl_cons, ih, insert, List.any_cons]
      exact Bool.or_assoc _ _ _

theorem census_exact (rows : List Row) (lane : Nat) :
    census rows lane = rows.any (fun row => hit row lane) := by
  simpa [census] using fold_exact rows (fun _ => false) lane

theorem ordered_transport_polls_agree (rows : List Row) (bound : Nat) :
    (List.range bound).filter (census rows) =
      (List.range bound).filter (fun lane => rows.any (fun row => hit row lane)) := by
  have predicates : census rows = fun lane => rows.any (fun row => hit row lane) := by
    funext lane
    exact census_exact rows lane
  rw [predicates]

theorem census_has_actual_eligible_row (rows : List Row) (lane : Nat)
    (present : census rows lane = true) :
    ∃ row ∈ rows, row.eligible = true ∧ row.lane = lane := by
  rw [census_exact] at present
  obtain ⟨row, member, observed⟩ := List.any_eq_true.mp present
  exact ⟨row, member, by simpa [hit] using observed⟩

theorem wire_lane_word_bounds (lane : Nat) (bounded : lane < 256) :
    lane / 32 < 8 ∧ lane % 32 < 32 ∧ lane % 32 + lane / 32 * 32 = lane := by
  constructor
  · exact (Nat.div_lt_iff_lt_mul (by decide : 0 < 32)).mpr bounded
  constructor
  · exact Nat.mod_lt lane (by decide)
  · simpa [Nat.mul_comm] using Nat.mod_add_div lane 32

#print axioms fold_exact
#print axioms census_exact
#print axioms ordered_transport_polls_agree
#print axioms census_has_actual_eligible_row
#print axioms wire_lane_word_bounds

end Hibana.RecvLaneCensus
