import Std

namespace EndpointDelegation.ReceiptWait

/-- Read-only classification of the pending source receipt catalog. These
functions interpret current observations, not another runtime owner table.
An earlier receipt covers a later self-dependency only when it returns the
resource to this actor and source causality puts it before that dependency. -/
def check (receipts : List α) (returnsHere borrowed : α → Bool)
    (sourceBefore : α → α → Bool) : Bool :=
  !receipts.isEmpty && receipts.all fun receipt =>
    !borrowed receipt || receipts.any fun prior =>
      returnsHere prior && sourceBefore prior receipt

theorem checked_wait_covers_every_borrowed_receipt
    {receipts : List α} {returnsHere borrowed : α → Bool}
    {sourceBefore : α → α → Bool} :
    check receipts returnsHere borrowed sourceBefore = true ↔
      receipts ≠ [] ∧ ∀ receipt ∈ receipts, borrowed receipt = true →
        ∃ prior ∈ receipts, returnsHere prior = true ∧ sourceBefore prior receipt = true := by
  constructor
  · intro checked
    have nonempty : receipts ≠ [] := by
      intro empty
      simp [check, empty] at checked
    have all := (Bool.and_eq_true_iff.mp checked).2
    refine ⟨nonempty, ?_⟩
    intro receipt member held
    have covered := List.all_eq_true.mp all receipt member
    have earlier : receipts.any
        (fun prior => returnsHere prior && sourceBefore prior receipt) = true := by
      simpa only [held, Bool.not_true, Bool.false_or] using covered
    obtain ⟨prior, belongs, facts⟩ := List.any_eq_true.mp earlier
    exact ⟨prior, belongs, Bool.and_eq_true_iff.mp facts⟩
  · rintro ⟨nonempty, covered⟩
    apply Bool.and_eq_true_iff.mpr
    constructor
    · cases receipts <;> simp_all
    · apply List.all_eq_true.mpr
      intro receipt member
      cases held : borrowed receipt with
      | false => simp
      | true =>
        obtain ⟨prior, belongs, here, before⟩ := covered receipt member held
        have earlier : receipts.any
            (fun prior => returnsHere prior && sourceBefore prior receipt) = true :=
          List.any_eq_true.mpr ⟨prior, belongs, Bool.and_eq_true_iff.mpr ⟨here, before⟩⟩
        simp only [Bool.not_true, Bool.false_or, earlier]

theorem an_uncovered_self_dependency_rejects_wait
    {receipts : List α} {returnsHere borrowed : α → Bool}
    {sourceBefore : α → α → Bool} {receipt : α}
    (member : receipt ∈ receipts) (held : borrowed receipt = true)
    (uncovered : ∀ prior ∈ receipts,
      returnsHere prior = true → sourceBefore prior receipt = false) :
    check receipts returnsHere borrowed sourceBefore = false := by
  cases result : check receipts returnsHere borrowed sourceBefore with
  | false => rfl
  | true =>
      obtain ⟨prior, belongs, here, before⟩ :=
        (checked_wait_covers_every_borrowed_receipt.mp result).2 receipt member held
      rw [uncovered prior belongs here] at before
      contradiction

theorem receipt_wait_does_not_depend_on_catalog_order
    {left right : List α} {returnsHere borrowed : α → Bool}
    {sourceBefore : α → α → Bool}
    (same : ∀ receipt, receipt ∈ left ↔ receipt ∈ right) :
    check left returnsHere borrowed sourceBefore =
      check right returnsHere borrowed sourceBefore := by
  have transport (first second : List α)
      (members : ∀ receipt, receipt ∈ first ↔ receipt ∈ second)
      (accepted : check first returnsHere borrowed sourceBefore = true) :
      check second returnsHere borrowed sourceBefore = true := by
    obtain ⟨nonempty, covered⟩ := checked_wait_covers_every_borrowed_receipt.mp accepted
    apply checked_wait_covers_every_borrowed_receipt.mpr
    constructor
    · intro empty
      cases first with
      | nil => exact nonempty rfl
      | cons head tail =>
          have belongs : head ∈ second := (members head).mp (by simp)
          simp [empty] at belongs
    · intro receipt member held
      obtain ⟨prior, belongs, here, before⟩ :=
        covered receipt ((members receipt).mpr member) held
      exact ⟨prior, (members prior).mp belongs, here, before⟩
  have equivalence := Iff.intro (transport left right same)
    (transport right left fun receipt => (same receipt).symm)
  cases leftResult : check left returnsHere borrowed sourceBefore <;>
    cases rightResult : check right returnsHere borrowed sourceBefore <;> simp_all

#print axioms checked_wait_covers_every_borrowed_receipt
#print axioms an_uncovered_self_dependency_rejects_wait
#print axioms receipt_wait_does_not_depend_on_catalog_order

end EndpointDelegation.ReceiptWait
