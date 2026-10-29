import Std

namespace Hibana.SourceOrderFilter

/-- Inspect a pair unless its unconditional answers agree, or its two
    operations belong to incompatible arms of one source choice. -/
def pending (expected possible certain excluded : Bool) : Bool :=
  ((expected && !certain) || (!expected && possible)) && !excluded

/-- The bounds are exactly those computed by the dependency interval sweep:
    an unconditional edge is present; every present edge is a possible edge. -/
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

end Hibana.SourceOrderFilter

#print axioms Hibana.SourceOrderFilter.omitted_pair_is_exact
#print axioms Hibana.SourceOrderFilter.candidate_check_is_complete
