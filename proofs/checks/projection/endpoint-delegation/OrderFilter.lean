import Std

namespace Hibana.SourceOrderFilter

/-- Inspect a pair unless its unconditional answers agree, or its two
    operations belong to incompatible arms of one source choice. -/
def pending (expected possible certain excluded : Bool) : Bool :=
  ((expected && !certain) || (!expected && possible)) && !excluded

/-- Dependency bounds are evaluated on paths containing the target row.
    A guard settled by that row's source choices can be certain or impossible. -/
def targetBounds (required : Option Bool) : Bool × Bool :=
  match required with
  | none => (true, false)
  | some value => (value, value)

/-- A required choice supplies both bounds whenever the target exists.
    Unknown choices keep the conservative possible/certain bounds. -/
theorem required_choice_supplies_dependency_bounds (required : Option Bool) (actual : Bool)
    (correct : ∀ value, required = some value → actual = value) :
    ((targetBounds required).2 = true → actual = true) ∧
      (actual = true → (targetBounds required).1 = true) := by
  cases required with
  | none => simp [targetBounds]
  | some value =>
    have same := correct value rfl
    cases value <;> simp_all [targetBounds]

/-- The first row's source choices refine interval bounds. Same-lane order
    survives an inactive guard, and a true guard never extends an interval. -/
theorem first_choice_refines_dependency_bounds
    (inside sameLane guard lower upper knownTrue knownFalse : Bool)
    (baseLower : lower = true → guard = true)
    (baseUpper : guard = true → upper = true)
    (firstTrue : knownTrue = true → guard = true)
    (firstFalse : knownFalse = true → guard = false) :
    let actual := sameLane || (inside && guard)
    let possible := sameLane || (inside && upper && !knownFalse)
    let certain := sameLane || (inside && lower) || (inside && upper && knownTrue)
    (certain = true → actual = true) ∧ (actual = true → possible = true) := by
  cases sameLane <;> cases inside <;> cases guard <;> simp_all

/-- The interval sweep gives lower and upper bounds for each active row pair. -/
theorem omitted_pair_is_exact (expected actual possible certain excluded : Bool)
    (lower : certain = true → actual = true)
    (upper : actual = true → possible = true)
    (omitted : pending expected possible certain excluded = false)
    (compatible : excluded = false) : expected = actual := by
  cases expected with
  | false =>
    cases actual with
    | false => rfl
    | true =>
      have present : possible = true := upper rfl
      simp [pending, present, compatible] at omitted
  | true =>
    cases actual with
    | false =>
      cases certainValue : certain with
      | false => simp [pending, certainValue, compatible] at omitted
      | true => have impossible := lower certainValue; contradiction
    | true => rfl

/-- Checking only the retained pairs proves the same compatible-pair
    obligations as checking every pair. This does not replace the source,
    ownership or conditional dependency checks for retained pairs. -/
theorem candidate_check_is_complete (expected actual possible certain excluded : Bool)
    (lower : certain = true → actual = true)
    (upper : actual = true → possible = true)
    (checked : pending expected possible certain excluded = true → expected = actual)
    (compatible : excluded = false) : expected = actual := by
  cases h : pending expected possible certain excluded with
  | false => exact omitted_pair_is_exact _ _ _ _ _ lower upper h compatible
  | true => exact checked h

/-- A row pair imposes an obligation only on paths containing both source
    events. Seeding their required choices first preserves every such path;
    unresolved holder choices still have to be checked on those paths. -/
theorem seeding_required_choices_is_exact {Path : Type}
    (first second selected valid : Path → Prop)
    (selection : ∀ path, selected path ↔ first path ∧ second path) :
    (∀ path, first path → second path → valid path) ↔
      (∀ path, selected path → valid path) := by
  constructor
  · intro checked path chosen
    have both := (selection path).mp chosen
    exact checked path both.1 both.2
  · intro checked path left right
    exact checked path ((selection path).mpr ⟨left, right⟩)

end Hibana.SourceOrderFilter

#print axioms Hibana.SourceOrderFilter.omitted_pair_is_exact
#print axioms Hibana.SourceOrderFilter.candidate_check_is_complete
#print axioms Hibana.SourceOrderFilter.seeding_required_choices_is_exact
#print axioms Hibana.SourceOrderFilter.required_choice_supplies_dependency_bounds
#print axioms Hibana.SourceOrderFilter.first_choice_refines_dependency_bounds
