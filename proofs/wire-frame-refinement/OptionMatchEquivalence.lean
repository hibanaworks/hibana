import Hibana.DescriptorImage

namespace Hibana.WireOptionMatchEquivalence

/-- The owner projection retains the exact existing zero sentinel. -/
theorem owner_explicit_match_exact (selected : Option (Nat × Nat)) :
    (selected.map Prod.snd).getD 0 =
      (match selected with | some (_, owner) => owner | none => 0) := by
  cases selected with
  | none => rfl
  | some pair => cases pair; rfl

/-- Exhaustion remains the invalid byte value 256, never an empty result. -/
theorem color_explicit_match_exact (selected : Option Nat) :
    selected.getD 256 =
      (match selected with | some color => color | none => 256) := by
  cases selected <;> rfl

/-- Both rewrites hold under any observing context, including recursive
    allocation and the unchanged exact certificate equality. -/
theorem owner_context_exact (observe : Nat → α) (selected : Option (Nat × Nat)) :
    observe ((selected.map Prod.snd).getD 0) =
      observe (match selected with | some (_, owner) => owner | none => 0) :=
  congrArg observe (owner_explicit_match_exact selected)

theorem color_context_exact (observe : Nat → α) (selected : Option Nat) :
    observe (selected.getD 256) =
      observe (match selected with | some color => color | none => 256) :=
  congrArg observe (color_explicit_match_exact selected)

#print axioms owner_explicit_match_exact
#print axioms color_explicit_match_exact
#print axioms owner_context_exact
#print axioms color_context_exact
end Hibana.WireOptionMatchEquivalence
