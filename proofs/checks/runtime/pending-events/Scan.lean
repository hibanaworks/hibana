import Std

namespace Hibana.PendingEvents

def rangeMask (low high : Nat) : BitVec 32 :=
  (BitVec.allOnes 32 <<< low) &&& BitVec.ofNat 32 (2 ^ high - 1)

-- Exact correspondence for the Rust word mask, including a final full word.
theorem mask_exact (done : BitVec 32) (low high bit : Nat) (b : bit < 32) :
    (~~~done &&& rangeMask low high).getLsbD bit = true ↔
      (done.getLsbD bit = false ∧ low ≤ bit ∧ bit < high) := by
  have all : (BitVec.allOnes 32).getLsbD (bit - low) = true := by
    rw [BitVec.getLsbD_allOnes]
    simp only [decide_eq_true_eq]
    omega
  have highBit : (BitVec.ofNat 32 (2 ^ high - 1)).getLsbD bit = decide (bit < high) := by
    rw [BitVec.getLsbD_ofNat, Nat.testBit_two_pow_sub_one]
    simp only [b, decide_true, Bool.true_and]
  simp only [rangeMask, BitVec.getLsbD_and, BitVec.getLsbD_not,
    BitVec.getLsbD_shiftLeft, all, highBit, b, decide_true, Bool.true_and,
    Bool.and_true, Bool.and_eq_true, decide_eq_true_eq]
  cases d : done.getLsbD bit <;> by_cases lo : bit < low <;>
    simp [lo, Nat.le_of_not_gt, Nat.not_le_of_gt]

-- Clearing the lowest selected bit removes exactly that bit; no other pending
-- predecessor is dropped. Rust calls trailing_zeros only on a nonzero word.
theorem clear_selected (pending : BitVec 32) (bit i : Nat)
    (within : i < 32) :
    (pending &&& ~~~(BitVec.twoPow 32 bit)).getLsbD i =
      (pending.getLsbD i && decide (i ≠ bit)) := by
  by_cases equal : bit = i
  · simp [equal, within]
  · simp [within, Ne.symm equal]

theorem lowest_present (pending : BitVec 32) (nonzero : pending ≠ 0#32) :
    pending.getLsbD pending.ctz.toNat = true := by
  exact BitVec.getLsbD_true_ctz_of_ne_zero nonzero

-- A word selected by a nonempty compact range always has a backing word.
theorem word_in_backing (start finish length : Nat)
    (nonempty : start < finish) (bound : finish ≤ length) :
    start / 32 < (length + 31) / 32 := by
  omega

theorem yielded_step_in_range (start finish word bit : Nat)
    (low : start ≤ word * 32 + bit)
    (high : word * 32 + bit < finish) :
    start ≤ word * 32 + bit ∧ word * 32 + bit < finish := by
  exact ⟨low, high⟩

-- Replacing a scan of all rows with the exact uncompleted subset preserves
-- rejection for arbitrary lane/route predicates. No predicate is weakened.
theorem admission_equivalent (done live : Nat → Prop) (start finish : Nat) :
    (∀ i, start ≤ i → i < finish → (¬ done i → ¬ live i)) ↔
      (∀ i, (start ≤ i ∧ i < finish ∧ ¬ done i) → ¬ live i) := by
  constructor
  · intro all i h
    exact all i h.1 h.2.1 h.2.2
  · intro pending i low high incomplete
    exact pending i ⟨low, high, incomplete⟩

-- A completed scope must have a participant. Searching only unfinished events
-- for blockers preserves that requirement for arbitrary selected local domains.
theorem scope_completion_equivalent (done live : Nat → Bool) (start finish : Nat) :
    ((∃ i, start ≤ i ∧ i < finish ∧ live i = true) ∧
      (∀ i, start ≤ i → i < finish → live i = true → done i = true)) ↔
    ((¬ ∃ i, start ≤ i ∧ i < finish ∧ done i = false ∧ live i = true) ∧
      (∃ i, start ≤ i ∧ i < finish ∧ live i = true)) := by
  constructor
  · rintro ⟨present, complete⟩
    refine ⟨?_, present⟩
    rintro ⟨i, low, high, unfinished, participates⟩
    have finished := complete i low high participates
    rw [finished] at unfinished
    contradiction
  · rintro ⟨unblocked, present⟩
    refine ⟨present, ?_⟩
    intro i low high participates
    cases finished : done i
    · exact False.elim (unblocked ⟨i, low, high, finished, participates⟩)
    · rfl

-- Compact event indices, word advances and yielded steps stay within 32-bit
-- usize; the implementation introduces neither storage nor mutation authority.
theorem compact_word_arithmetic (length word : Nat)
    (l : length ≤ 65535) (w : word ≤ length / 32) :
    (word + 1) * 32 + 31 < 4294967296 := by
  omega

def completionBits (events rolls epochs : Nat) : Nat :=
  if epochs = 0 then events + rolls
  else ((events + rolls + 31) / 32) * 32 + 64 * epochs

-- The event bitmap is a prefix of the same allocation even when retained
-- roll bits and aligned epochs occupy the following words.
theorem local_event_words_fit_storage (events rolls epochs : Nat) :
    (events + 31) / 32 ≤ (completionBits events rolls epochs + 31) / 32 := by
  unfold completionBits
  split <;> omega

theorem compact_completion_extent_fits_usize (events rolls epochs : Nat)
    (e : events ≤ 65535) (r : rolls ≤ 65535) (ep : epochs ≤ rolls) :
    completionBits events rolls epochs + 31 < 4294967296 := by
  unfold completionBits
  split <;> omega

theorem skipping_an_excluded_prefix_preserves_pending_blockers
    (done live : Nat → Bool) (start next finish : Nat) (forward : start ≤ next)
    (excluded : ∀ i, start ≤ i → i < next → live i = false) :
    (∃ i, start ≤ i ∧ i < finish ∧ done i = false ∧ live i = true) ↔
    (∃ i, next ≤ i ∧ i < finish ∧ done i = false ∧ live i = true) := by
  constructor
  · rintro ⟨i, low, high, pending, active⟩
    by_cases after : next ≤ i
    · exact ⟨i, after, high, pending, active⟩
    · have dead := excluded i low (Nat.lt_of_not_ge after)
      simp [dead] at active
  · rintro ⟨i, low, high, pending, active⟩
    exact ⟨i, Nat.le_trans forward low, high, pending, active⟩

theorem excluded_arm_jump_is_strictly_forward (index armEnd queryEnd : Nat)
    (inside : index < armEnd) (queried : index < queryEnd) :
    index < min armEnd queryEnd := by omega

theorem a_conflicting_ancestor_excludes_its_event
    (selected : Nat → Option Bool) (context : List (Nat × Bool))
    (scope : Nat) (expected actual : Bool)
    (member : (scope, expected) ∈ context) (chosen : selected scope = some actual)
    (different : actual ≠ expected) :
    ¬ (∀ binding ∈ context,
      selected binding.1 = none ∨ selected binding.1 = some binding.2) := by
  intro live
  have agrees := live (scope, expected) member
  rcases agrees with absent | same
  · rw [chosen] at absent
    cases absent
  · exact different (Option.some.inj (chosen.symm.trans same))

#print axioms mask_exact
#print axioms clear_selected
#print axioms lowest_present
#print axioms word_in_backing
#print axioms yielded_step_in_range
#print axioms admission_equivalent
#print axioms scope_completion_equivalent
#print axioms compact_word_arithmetic
#print axioms local_event_words_fit_storage
#print axioms compact_completion_extent_fits_usize
#print axioms skipping_an_excluded_prefix_preserves_pending_blockers
#print axioms excluded_arm_jump_is_strictly_forward
#print axioms a_conflicting_ancestor_excludes_its_event

end Hibana.PendingEvents
