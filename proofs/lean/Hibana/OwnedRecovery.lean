import Hibana.DescriptorImage

namespace Hibana

/-- All projected owned receives, grouped by resource and then local row.
This is derived independently from direction flags and decoded global atoms. -/
def RustDescriptorImage.expectedOwnedReceipts?
    (image : RustDescriptorImage) : Option (List (Nat × Nat)) := do
  if image.rollEntryViewCount = 0 then return []
  let atoms ← image.decodeAtoms?
  let rows ← (List.range image.eventCount).mapM fun row => do
    let flags ← image.eventFlags? row
    let effIndex ← image.eventEffIndex? row
    let atom ← resolveUnique? (atoms.filter fun atom => atom.effIndex = effIndex)
    pure (if flags / 2 = 2 ∧ 4294967296 ≤ atom.schema then
      some (atom.schema - 4294967296, row) else none)
  let receipts := rows.filterMap id
  pure ((List.range image.roleCount).flatMap fun resource =>
    receipts.filter fun row => row.1 == resource)

def RustDescriptorImage.decodeOwnedReceipts?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.ownedReceiptCount).mapM fun row =>
    readU16LE? image.roleBytes (image.roleOwnedReceiptOffset + row * productionRoleU16Stride)

def RustDescriptorImage.decodeEntryReceipts?
    (image : RustDescriptorImage) (entry : Nat) : Option (Nat × Nat × Nat) := do
  if entry < image.rollEntryViewCount then pure () else none
  let offset := image.roleRollEntryViewOffset + entry * productionRoleRollEntryStride
  let resource ← readByte? image.roleBytes offset
  let range ← readU32LE? image.roleBytes (offset + 3)
  pure (resource, range / 65536, range % 65536)

def RustDescriptorImage.ownedEntryReceiptsCheck
    (image : RustDescriptorImage) (expected : List (Nat × Nat))
    (actual : List Nat) (entry : Nat) : Bool :=
  match image.decodeEntryReceipts? entry, image.decodeAtoms? with
  | some (resource, first, len), some atoms =>
    resource < image.roleCount &&
      atoms.any (fun atom => atom.schema == 4294967296 + resource) &&
      first + len ≤ image.ownedReceiptCount &&
      (actual.drop first).take len ==
        ((expected.filter fun row => row.1 == resource).map Prod.snd)
  | _, _ => false

/-- Catalog membership is checked, not inferred from the current holder.
An accepted entry slice contains every receive that can issue its resource. -/
def RustDescriptorImage.ownedRecoveryIndexCheck (image : RustDescriptorImage) : Bool :=
  match image.expectedOwnedReceipts?, image.decodeOwnedReceipts? with
  | some expected, some actual =>
    actual == expected.map Prod.snd &&
      (List.range image.rollEntryViewCount).all (image.ownedEntryReceiptsCheck expected actual)
  | _, _ => false

theorem accepted_owned_recovery_catalog_is_exact
    {image : RustDescriptorImage}
    (accepted : image.ownedRecoveryIndexCheck = true) :
    image.decodeOwnedReceipts? =
      image.expectedOwnedReceipts?.map (List.map Prod.snd) := by
  unfold RustDescriptorImage.ownedRecoveryIndexCheck at accepted
  cases expectedAt : image.expectedOwnedReceipts? with
  | none => simp [expectedAt] at accepted
  | some expected =>
    cases actualAt : image.decodeOwnedReceipts? with
    | none => simp [expectedAt, actualAt] at accepted
    | some actual =>
      simp only [expectedAt, actualAt, Bool.and_eq_true] at accepted
      have exactRows : actual = expected.map Prod.snd := by
        simpa using accepted.1
      simp [exactRows]

theorem accepted_owned_recovery_entries_are_complete
    {image : RustDescriptorImage} {expected : List (Nat × Nat)} {actual : List Nat}
    (expectedAt : image.expectedOwnedReceipts? = some expected)
    (actualAt : image.decodeOwnedReceipts? = some actual)
    (accepted : image.ownedRecoveryIndexCheck = true) :
    ∀ entry < image.rollEntryViewCount,
      image.ownedEntryReceiptsCheck expected actual entry = true := by
  simp only [RustDescriptorImage.ownedRecoveryIndexCheck, expectedAt, actualAt,
    Bool.and_eq_true] at accepted
  intro entry bound
  exact List.all_eq_true.mp accepted.2 entry (List.mem_range.mpr bound)

end Hibana
