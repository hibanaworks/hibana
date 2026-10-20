import Std

/-! Unbounded cardinality layer for the concrete category-major traversal.
`before` is exactly the list of membership tags at indices `< cursor` and
`after` is the remaining list. Every occurrence is one distinct event slot,
even when tags repeat. No hypothesis about event count or route shape is used.
-/
namespace Hibana.RoutePathRefinement.Loop

abbrev Tag := Fin 3

def countBy (test : Tag → Bool) : List Tag → Nat
  | [] => 0
  | t :: ts => (if test t then 1 else 0) + countBy test ts

def below (category : Nat) (tag : Tag) : Bool := decide (tag.val < category)
def through (category : Nat) (tag : Tag) : Bool := decide (tag.val ≤ category)

/-- The before has visited the current category, the after has only visited
strictly earlier categories. This is precisely the SMT `D` predicate count. -/
def processed (category : Nat) (before after : List Tag) : Nat :=
  countBy (through category) before + countBy (below category) after

theorem countBy_append (test : Tag → Bool) (xs ys : List Tag) :
    countBy test (xs ++ ys) = countBy test xs + countBy test ys := by
  induction xs with
  | nil => simp [countBy]
  | cons x xs ih => simp only [List.cons_append, countBy, ih]; omega

theorem countBy_bound (test : Tag → Bool) (xs : List Tag) :
    countBy test xs ≤ xs.length := by
  induction xs with
  | nil => simp [countBy]
  | cons x xs ih => simp only [countBy, List.length_cons]; split <;> omega

theorem processed_bound (category : Nat) (before after : List Tag) :
    processed category before after ≤ before.length + after.length := by
  have hp := countBy_bound (through category) before
  have hs := countBy_bound (below category) after
  simp only [processed]
  omega

theorem below_zero (xs : List Tag) : countBy (below 0) xs = 0 := by
  induction xs with
  | nil => rfl
  | cons x xs ih => simp [countBy, below, ih]

theorem initial_count (xs : List Tag) : processed 0 [] xs = 0 := by
  simp [processed, countBy, below_zero]

/-- Moving the actual next event slot across the cursor increments the count
exactly when the event belongs to the current category. Skips do not consume
a processed-event credit. -/
theorem event_step (category : Nat) (before after : List Tag) (tag : Tag) :
    processed category (before ++ [tag]) after =
      processed category before (tag :: after) + (if tag.val = category then 1 else 0) := by
  simp only [processed, countBy_append, countBy]
  by_cases he : tag.val = category
  · simp [through, below, he]
    omega
  · by_cases hl : tag.val < category
    · have hle : tag.val ≤ category := by omega
      simp [through, below, he, hl, hle]
      omega
    · have hle : ¬ tag.val ≤ category := by omega
      simp [through, below, he, hl, hle]

/-- At a matching slot the processed count is strictly smaller than the total
number of slots. This justifies fresh-ID allocation and sentinel safety. -/
theorem matching_has_credit (category : Nat) (before after : List Tag) (tag : Tag)
    (hm : tag.val = category) :
    processed category before (tag :: after) < before.length + (tag :: after).length := by
  have hp := countBy_bound (through category) before
  have hs := countBy_bound (below category) after
  simp [processed, countBy, below, hm]
  omega

theorem through_eq_next_below (category : Nat) (xs : List Tag) :
    countBy (through category) xs = countBy (below (category + 1)) xs := by
  induction xs with
  | nil => rfl
  | cons x xs ih =>
    have he : decide (x.val ≤ category) = decide (x.val < category + 1) := by
      simp only [Nat.lt_succ_iff]
    simpa only [countBy, through, below, he] using congrArg
      (fun v => (if decide (x.val < category + 1) then 1 else 0) + v) ih

/-- Resetting the remap array and restarting the cursor for the next category
does not reset either the processed count or the next-ID count. -/
theorem category_reset (category : Nat) (xs : List Tag) :
    processed category xs [] = processed (category + 1) [] xs := by
  simp only [processed, countBy, Nat.add_zero, Nat.zero_add]
  exact through_eq_next_below category xs

theorem below_three (xs : List Tag) : countBy (below 3) xs = xs.length := by
  induction xs with
  | nil => rfl
  | cons x xs ih => simp [countBy, below, x.isLt, ih, Nat.add_comm]

theorem final_count (xs : List Tag) : processed 3 [] xs = xs.length := by
  simp [processed, countBy, below_three]

/-- Finite-width safety follows without reducing the quantified event domain
or enumerating any small counts. -/
theorem fresh_id_safe (category : Nat) (before after : List Tag) (tag : Tag)
    (nextId : Nat) (hm : tag.val = category)
    (hc : nextId ≤ processed category before (tag :: after))
    (hn : before.length + (tag :: after).length ≤ 65535) :
    nextId < 65535 ∧ nextId + 1 ≤ 65535 := by
  have h := matching_has_credit category before after tag hm
  omega

#print axioms initial_count
#print axioms processed_bound
#print axioms event_step
#print axioms matching_has_credit
#print axioms category_reset
#print axioms final_count
#print axioms fresh_id_safe
end Hibana.RoutePathRefinement.Loop
