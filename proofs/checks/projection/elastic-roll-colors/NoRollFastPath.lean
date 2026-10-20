-- Read-only preprocessing is total on SourceLowering's initialized rows.
-- It does not mutate source. This theorem does not remove validation of rows.
namespace NoRollFastPath
def late {A B : Type} (scratch : A → B) (hasRoll : Bool) (finish : A → A) (src : A) :=
  let _ := scratch src
  if hasRoll then finish src else src
def early {A B : Type} (scratch : A → B) (hasRoll : Bool) (finish : A → A) (src : A) :=
  if hasRoll then
    let _ := scratch src
    finish src
  else src
theorem exact {A B : Type} (scratch : A → B) (hasRoll : Bool) (finish : A → A) (src : A) :
  late scratch hasRoll finish src = early scratch hasRoll finish src := by
  cases hasRoll <;> rfl
#print axioms exact
end NoRollFastPath
