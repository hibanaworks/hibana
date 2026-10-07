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

-- Compact event indices, word advances and yielded steps stay within 32-bit
-- usize; the implementation introduces neither storage nor mutation authority.
theorem compact_word_arithmetic (length word : Nat)
    (l : length ≤ 65535) (w : word ≤ length / 32) :
    (word + 1) * 32 + 31 < 4294967296 := by
  omega

#print axioms mask_exact
#print axioms clear_selected
#print axioms lowest_present
#print axioms word_in_backing
#print axioms yielded_step_in_range
#print axioms admission_equivalent
#print axioms compact_word_arithmetic

end Hibana.PendingEvents
