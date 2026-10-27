import Std

namespace Hibana.RouteTopology

/- One source unfolding, independently defined by the source grammar. Parallel
   and choice siblings have no causal edge; rolls preserve their body's order.
   This is not an extraction or a proof of Rust marker decoding. -/
inductive Composition where
  | sequential | parallel | choice
  deriving DecidableEq

inductive Source where
  | atom
  | binary (kind : Composition) (left right : Source)
  | roll (body : Source)

def width : Source → Nat
  | .atom => 1
  | .binary _ left right => width left + width right
  | .roll body => width body

def precedes : Source → Nat → Nat → Prop
  | .atom, _, _ => False
  | .binary kind left right, before, after =>
      if after < width left then precedes left before after
      else if before < width left then kind = .sequential
      else precedes right (before - width left) (after - width left)
  | .roll body, before, after => precedes body before after

/-- An earlier applicable predecessor remains on the causal chain of a skipped
    nonpreceding sibling. This property follows from the grammar, rather than
    being assumed of an arbitrary partial order. -/
theorem source_order_interpolation (source : Source) (a q b : Nat)
    (ordered : a < q ∧ q < b) (inside : b < width source)
    (applicable : precedes source a b) (skipped : ¬ precedes source q b) :
    precedes source a q := by
  induction source generalizing a q b with
  | atom => exact False.elim applicable
  | roll body ih => exact ih a q b ordered inside applicable skipped
  | binary kind left right leftIH rightIH =>
      by_cases bLeft : b < width left
      · have qLeft : q < width left := by omega
        simp only [precedes, bLeft, qLeft, ↓reduceIte] at applicable skipped ⊢
        exact leftIH a q b ordered bLeft applicable skipped
      · by_cases qLeft : q < width left
        · have aLeft : a < width left := by omega
          simp only [precedes, bLeft, qLeft, aLeft, ↓reduceIte] at applicable skipped
          exact False.elim (skipped applicable)
        · by_cases aLeft : a < width left
          · simpa only [precedes, bLeft, qLeft, aLeft, ↓reduceIte] using applicable
          · simp only [precedes, bLeft, qLeft, aLeft, ↓reduceIte] at applicable skipped ⊢
            apply rightIH (a - width left) (q - width left) (b - width left)
            · constructor <;> omega
            · simp only [width] at inside
              omega
            · exact applicable
            · exact skipped

def SameViewPredecessor (source : Source) (view : Nat → Nat) (step prior : Nat) : Prop :=
  prior < step ∧ view prior = view step ∧ precedes source prior step

/- This invariant is the specification of the compiler-produced link, not an
   assumption claimed to have been proved about the Rust image. The finite
   independent grammar checker exercises its actual chain-building algorithm;
   decoded Rust/source correspondence is checked separately. -/
def GreatestPredecessor (source : Source) (view : Nat → Nat) (step : Nat) : Option Nat → Prop
  | none => ∀ prior, ¬ SameViewPredecessor source view step prior
  | some prior => SameViewPredecessor source view step prior ∧
      ∀ candidate, SameViewPredecessor source view step candidate → candidate ≤ prior

/-- One chain traversal step preserves every applicable predecessor in its
    search interval. Iterating cannot jump over a later applicable witness. -/
theorem skip_preserves_predecessors
    (source : Source) (view : Nat → Nat) (a q b : Nat) (link : Option Nat)
    (ordered : q < b) (inside : b < width source) (sameView : view q = view b)
    (skipped : ¬ precedes source q b)
    (greatest : GreatestPredecessor source view q link) :
    (a ≤ q ∧ SameViewPredecessor source view b a) ↔
      ((∃ prior, link = some prior ∧ a ≤ prior) ∧
        SameViewPredecessor source view b a) := by
  constructor
  · rintro ⟨below, applicable⟩
    have before : a < q := by
      have different : a ≠ q := by
        intro equal
        subst a
        exact skipped applicable.2.2
      omega
    have prior : SameViewPredecessor source view q a :=
      ⟨before, applicable.2.1.trans sameView.symm,
        source_order_interpolation source a q b ⟨before, ordered⟩ inside
          applicable.2.2 skipped⟩
    cases link with
    | none => exact False.elim (greatest a prior)
    | some p => exact ⟨⟨p, rfl, greatest.2 a prior⟩, applicable⟩
  · rintro ⟨⟨p, equal, below⟩, applicable⟩
    subst link
    exact ⟨by have := greatest.1.1; omega, applicable⟩

def CompressedEntry (start : Nat) : Option Nat → Prop
  | none => True
  | some prior => prior < start

/-- Given the same first view/lane candidate, the compressed predecessor test
    is exactly the original absence-of-a-predecessor test in its contiguous arm.
    Sorted lookup and correctness of the produced link remain separate claims. -/
theorem compressed_entry_exact (source : Source) (view : Nat → Nat)
    (start stop step : Nat) (link : Option Nat) (inside : start ≤ step ∧ step < stop)
    (greatest : GreatestPredecessor source view step link) :
    CompressedEntry start link ↔
      ¬ ∃ prior, start ≤ prior ∧ prior < stop ∧
        SameViewPredecessor source view step prior := by
  cases link with
  | none =>
      constructor
      · intro _ ⟨prior, _, _, candidate⟩
        exact greatest prior candidate
      · intro _
        trivial
  | some p =>
      constructor
      · intro outside ⟨prior, lower, _, candidate⟩
        have upper := greatest.2 prior candidate
        change p < start at outside
        omega
      · intro absent
        change p < start
        apply Nat.lt_of_not_ge
        intro notBefore
        exact absent ⟨p, by omega, by have := greatest.1.1; omega, greatest.1⟩

#print axioms source_order_interpolation
#print axioms skip_preserves_predecessors
#print axioms compressed_entry_exact

end Hibana.RouteTopology
