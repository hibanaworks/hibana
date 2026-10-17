import Hibana.OwnedRecovery

namespace Hibana

/-- Exact immutable (scope, resource, entry-owner reference) obligations. -/
abbrev OwnedEntryResources := List (DecodedRollRow × List (Nat × Nat))

private def sourceRollBounds?
    (markers : List DecodedScopeMarker) (scope atomCount : Nat) : Option (Nat × Nat) := do
  let first ← (List.range markers.length).find? fun index =>
    match markers[index]? with
    | some marker => marker.scope == 8192 + scope && marker.tag % 4 == 0
    | none => false
  let enter ← markers[first]?
  let exit ← (markers.drop (first + 1)).find? fun marker =>
    marker.scope == enter.scope && marker.tag % 4 == 2
  if enter.offset < exit.offset && exit.offset ≤ atomCount then
    pure (enter.offset, exit.offset)
  else none

private def expectedEntryResource?
    (atoms : List DecodedProgramAtom) (columns : DecodedProgramColumns)
    (localRows : List (Nat × Nat)) (start stop resource : Nat) : Option (Option (Nat × Nat)) := do
  match (atoms.zipIdx).find? fun (atom, event) =>
      start ≤ event && event < stop &&
        (atom.sender == resource || atom.receiver == resource ||
          atom.schema == 4294967296 + resource) with
  | none => pure none
  | some (atom, event) =>
    let sender := atom.sender == resource || atom.schema == 4294967296 + resource
    let direction := if atom.sender == atom.receiver then 0 else if sender then 1 else 2
    if localRows.contains (atom.effIndex, direction) then
      let binding ← columns.actorBindings[event]?
      pure (some (resource, if sender then binding.1 else binding.2))
    else pure none

/-- Rebuild local roll ranges and their obligations from global source markers
and independently checked local directions. Neither local table's count can
disable a source obligation. -/
def RustDescriptorImage.expectedRollEntryResources?
    (image : RustDescriptorImage) : Option OwnedEntryResources := do
  let columns ← image.decodeProgramColumns?
  let atoms ← image.decodeAtoms?
  let markers ← image.decodeScopeMarkers?
  let localRows ← (List.range image.eventCount).mapM fun row => do
    let event ← image.eventEffIndex? row
    let flags ← image.eventFlags? row
    pure (event, flags / 2)
  let resources := (List.range image.roleCount).filter fun resource =>
    atoms.any fun atom => atom.schema == 4294967296 + resource
  let rolls ← markers.mapM fun marker => do
    if marker.scope / 8192 != 1 || marker.tag % 4 != 0 then pure none else
      let scope := marker.scope % 8192
      let (start, stop) ← sourceRollBounds? markers scope atoms.length
      let positions := (List.range localRows.length).filter fun position =>
        match localRows[position]? with
        | some (event, _) => start ≤ event && event < stop
        | none => false
      match positions.head? with
      | none => pure none
      | some first =>
        let roll : DecodedRollRow := { scope, eventStart := first, eventLength := positions.length }
        let entries ← if columns.actorBindings.isEmpty then pure [] else
          resources.mapM (expectedEntryResource? atoms columns localRows start stop)
        pure (some (roll, entries.filterMap id))
  pure (rolls.filterMap id)

def RustDescriptorImage.decodeRollEntryResources?
    (image : RustDescriptorImage) : Option OwnedEntryResources := do
  let rolls ← image.decodeRollRows?
  (rolls.zipIdx).mapM fun (roll, slot) => do
    if image.rollEntryViewCount = 0 then pure (roll, []) else
      let range ← readU32LE? image.roleBytes
        (image.roleRollEntryRangeOffset + slot * productionRoleLaneRangeStride)
      let first := range / 65536
      let len := range % 65536
      if first + len ≤ image.rollEntryViewCount then pure () else none
      let entries ← (eventRange first len).mapM fun entry => do
        let offset := image.roleRollEntryViewOffset + entry * productionRoleRollEntryStride
        let resource ← readByte? image.roleBytes offset
        let owner ← readU16LE? image.roleBytes (offset + 1)
        pure (resource, owner)
      pure (roll, entries)

def RustDescriptorImage.ownedEntryResourcesCheck (image : RustDescriptorImage) : Bool :=
  match image.expectedRollEntryResources?, image.decodeRollEntryResources? with
  | some expected, some actual =>
    actual == expected &&
      (actual.map (fun row => row.2.length)).sum == image.rollEntryViewCount
  | _, _ => false

theorem accepted_owned_entry_resources_match_source
    {image : RustDescriptorImage}
    (accepted : image.ownedEntryResourcesCheck = true) :
    ∃ rows, image.expectedRollEntryResources? = some rows ∧
      image.decodeRollEntryResources? = some rows := by
  unfold RustDescriptorImage.ownedEntryResourcesCheck at accepted
  cases expectedAt : image.expectedRollEntryResources? with
  | none => simp [expectedAt] at accepted
  | some expected =>
    cases actualAt : image.decodeRollEntryResources? with
    | none => simp [expectedAt, actualAt] at accepted
    | some actual =>
      simp only [expectedAt, actualAt, Bool.and_eq_true] at accepted
      have exactRows : actual = expected := by
        simpa only [beq_iff_eq] using accepted.1
      exact ⟨expected, rfl, by simp [exactRows]⟩

theorem accepted_owned_entry_resource_cannot_be_omitted
    {image : RustDescriptorImage} {rows : OwnedEntryResources}
    {roll : DecodedRollRow} {resource owner : Nat} {entries : List (Nat × Nat)}
    (accepted : image.ownedEntryResourcesCheck = true)
    (expectedAt : image.expectedRollEntryResources? = some rows)
    (scopeAt : (roll, entries) ∈ rows) (required : (resource, owner) ∈ entries) :
    ∃ actual, image.decodeRollEntryResources? = some actual ∧
      ∃ present, (roll, present) ∈ actual ∧ (resource, owner) ∈ present := by
  obtain ⟨exactRows, expected, decoded⟩ := accepted_owned_entry_resources_match_source accepted
  have same : rows = exactRows := by simpa [expectedAt] using expected
  subst exactRows
  exact ⟨rows, decoded, entries, scopeAt, required⟩

end Hibana
