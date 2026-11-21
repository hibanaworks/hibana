import Std

namespace Hibana.PendingEvents

def sourceMask (start finish word : Nat) : BitVec 64 :=
  let base := word * 64
  if finish ≤ base ∨ base + 64 ≤ start then 0
  else (BitVec.allOnes 64 <<< (start - base)) &&&
    (BitVec.allOnes 64 >>> (64 - min (finish - base) 64))

theorem source_word_bounds (word bit : Nat) (w : word < 1024) (b : bit < 64) :
    word * 64 + bit < 65536 ∧ word * 64 + 64 ≤ 65536 := by omega

/-- Exact half-open membership for every compact source word and endpoint. -/
theorem source_mask_exact (start finish word bit : Nat) (within : bit < 64) :
    (sourceMask start finish word).getLsbD bit = true ↔
      start ≤ word * 64 + bit ∧ word * 64 + bit < finish := by
  dsimp only [sourceMask]
  split
  · rename_i outside
    simp
    omega
  · simp only [BitVec.getLsbD_and, BitVec.getLsbD_shiftLeft,
      BitVec.getLsbD_ushiftRight, BitVec.getLsbD_allOnes,
      Bool.and_eq_true, decide_eq_true_eq]
    by_cases low : bit < start - word * 64 <;> simp [low] <;> omega

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

private theorem set_bit_after_decrement (n i : Nat) (setBit : n.testBit i = true) :
    (n - 1).testBit i = true ↔ ∃ j < i, n.testBit j = true := by
  induction i generalizing n with
  | zero =>
    simp only [Nat.testBit_zero, decide_eq_true_eq] at setBit ⊢
    simp only [Nat.not_lt_zero, false_and, exists_false, iff_false]
    omega
  | succ i ih =>
    rw [Nat.testBit_succ] at setBit ⊢
    by_cases odd : n % 2 = 1
    · rw [show (n - 1) / 2 = n / 2 by omega, setBit]
      exact ⟨fun _ => ⟨0, Nat.zero_lt_succ _, by simp [odd]⟩, fun _ => rfl⟩
    · rw [show (n - 1) / 2 = n / 2 - 1 by omega, ih _ setBit]
      constructor
      · rintro ⟨j, lt, yes⟩
        exact ⟨j + 1, by omega, by rwa [Nat.testBit_succ]⟩
      · rintro ⟨j, lt, yes⟩
        cases j with
        | zero => simp [odd] at yes
        | succ j => exact ⟨j, by omega, by rwa [Nat.testBit_succ] at yes⟩

private theorem word_decrement (w : BitVec 32) (nz : w ≠ 0#32) :
    (w - 1#32).toNat = w.toNat - 1 := by
  have pos : 0 < w.toNat := by
    exact Nat.pos_of_ne_zero (fun h => nz (BitVec.eq_of_toNat_eq h))
  exact BitVec.toNat_sub_of_le (by simpa [BitVec.le_def] using pos : 1#32 ≤ w)

theorem advance_word_exact (pending : BitVec 32) (nz : pending ≠ 0#32) (i : Nat) :
    (pending &&& (pending - 1#32)).getLsbD i = true ↔
      pending.getLsbD i = true ∧ pending.ctz.toNat < i := by
  rw [BitVec.getLsbD_and, Bool.and_eq_true]
  constructor
  · rintro ⟨setBit, stays⟩
    change (pending - 1#32).toNat.testBit i = true at stays
    rw [word_decrement _ nz] at stays
    obtain ⟨j, lower, yes⟩ := (set_bit_after_decrement _ _ setBit).mp stays
    have : pending.ctz.toNat ≤ j := by
      apply Nat.le_of_not_gt
      intro h
      have absent := BitVec.getLsbD_false_of_lt_ctz (x := pending) (i := j) (by omega)
      change pending.getLsbD j = true at yes
      simp [absent] at yes
    exact ⟨setBit, by omega⟩
  · rintro ⟨setBit, higher⟩
    refine ⟨setBit, ?_⟩
    change (pending - 1#32).toNat.testBit i = true
    rw [word_decrement _ nz]
    exact (set_bit_after_decrement _ _ setBit).mpr
      ⟨_, higher, BitVec.getLsbD_true_ctz_of_ne_zero nz⟩

private def wordSteps (w : BitVec 32) : List Nat :=
  if _zero : w = 0#32 then []
  else w.ctz.toNat :: wordSteps (w &&& (w - 1#32))
termination_by w.toNat
decreasing_by
  rw [BitVec.toNat_and, word_decrement _ _zero]
  have bound := Nat.and_le_right (n := w.toNat) (m := w.toNat - 1)
  have : w.toNat ≠ 0 := fun h => _zero (BitVec.eq_of_toNat_eq h)
  omega

theorem word_steps_membership (w : BitVec 32) (i : Nat) :
    i ∈ wordSteps w ↔ w.getLsbD i = true := by
  fun_induction wordSteps w with
  | case1 => simp
  | case2 word nz ih =>
    rw [List.mem_cons, ih, advance_word_exact _ nz]
    constructor
    · rintro (rfl | ⟨atI, _⟩)
      · exact BitVec.getLsbD_true_ctz_of_ne_zero nz
      · exact atI
    · intro atI
      by_cases low : i < word.ctz.toNat
      · simp [BitVec.getLsbD_false_of_lt_ctz low] at atI
      · by_cases same : i = word.ctz.toNat
        · exact Or.inl same
        · exact Or.inr ⟨atI, by omega⟩

theorem word_steps_strict (w : BitVec 32) : (wordSteps w).Pairwise (· < ·) := by
  fun_induction wordSteps w with
  | case1 => simp
  | case2 word nz ih =>
    rw [List.pairwise_cons]
    exact ⟨fun i inside => ((advance_word_exact word nz i).mp
      ((word_steps_membership _ i).mp inside)).2, ih⟩

/-- Each visited word contributes its unfinished bits in increasing order.
The first and last masks restrict the scan to the half-open caller range. -/
private def pendingSteps (done : Nat → BitVec 32) (start finish : Nat) : List Nat :=
  (List.range' (start / 32) ((finish + 31) / 32 - start / 32)).flatMap fun word =>
    let base := word * 32
    (wordSteps (~~~(done word) &&& rangeMask (start - base) (min (finish - base) 32))).map
      (fun bit => base + bit)

theorem scanned_step_iff_unfinished (done : Nat → BitVec 32) (start finish i : Nat) :
    i ∈ pendingSteps done start finish ↔
      start ≤ i ∧ i < finish ∧ (done (i / 32)).getLsbD (i % 32) = false := by
  simp only [pendingSteps, List.mem_flatMap, List.mem_map, List.mem_range'_1, word_steps_membership]
  constructor
  · rintro ⟨w, _, b, active, rfl⟩
    have width : b < 32 := by
      apply Nat.lt_of_not_ge
      intro outside
      simp [BitVec.getLsbD_of_ge _ _ outside] at active
    obtain ⟨undone, low, high⟩ := (mask_exact _ _ _ _ width).mp active
    have idx : (w * 32 + b) / 32 = w := by omega
    have rem : (w * 32 + b) % 32 = b := by omega
    rw [idx, rem]
    exact ⟨by omega, by omega, undone⟩
  · rintro ⟨low, high, undone⟩
    refine ⟨i / 32, ⟨by omega, by omega⟩, i % 32, ?_, by omega⟩
    exact (mask_exact _ _ _ _ (by omega)).mpr ⟨undone, by omega, by omega⟩

-- A word selected by a nonempty compact range always has a backing word.
theorem word_in_backing (start finish length : Nat)
    (nonempty : start < finish) (bound : finish ≤ length) :
    start / 32 < (length + 31) / 32 := by
  omega

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

-- The same jump preserves the nonempty local domain required by roll
-- completion. An excluded arm cannot supply its participation witness.
theorem skipping_an_excluded_prefix_preserves_participation
    (live : Nat → Bool) (start next finish : Nat) (forward : start ≤ next)
    (excluded : ∀ i, start ≤ i → i < next → live i = false) :
    (∃ i, start ≤ i ∧ i < finish ∧ live i = true) ↔
    (∃ i, next ≤ i ∧ i < finish ∧ live i = true) := by
  constructor
  · rintro ⟨i, low, high, active⟩
    by_cases after : next ≤ i
    · exact ⟨i, after, high, active⟩
    · have dead := excluded i low (Nat.lt_of_not_ge after)
      simp [dead] at active
  · rintro ⟨i, low, high, active⟩
    exact ⟨i, Nat.le_trans forward low, high, active⟩

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
#print axioms source_mask_exact
#print axioms source_word_bounds
#print axioms advance_word_exact
#print axioms word_steps_membership
#print axioms word_steps_strict
#print axioms scanned_step_iff_unfinished
#print axioms word_in_backing
#print axioms scope_completion_equivalent
#print axioms compact_word_arithmetic
#print axioms local_event_words_fit_storage
#print axioms compact_completion_extent_fits_usize
#print axioms skipping_an_excluded_prefix_preserves_pending_blockers
#print axioms skipping_an_excluded_prefix_preserves_participation
#print axioms excluded_arm_jump_is_strictly_forward
#print axioms a_conflicting_ancestor_excludes_its_event

end Hibana.PendingEvents
