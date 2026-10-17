import Hibana.ActorIngress

namespace Hibana

theorem event_range_membership (start length event : Nat) :
    event ∈ eventRange start length ↔ start ≤ event ∧ event < start + length := by
  simp only [eventRange, List.mem_map, List.mem_range]
  constructor
  · rintro ⟨offset, bound, rfl⟩
    exact ⟨Nat.le_add_right _ _, Nat.add_lt_add_left bound _⟩
  · rintro ⟨lower, upper⟩
    have bounded : start + (event - start) < start + length := by
      rw [Nat.add_sub_of_le lower]
      exact upper
    exact ⟨event - start, Nat.lt_of_add_lt_add_left bounded, Nat.add_sub_of_le lower⟩

/-- Roll ranges come from the source scope and the actor's exact candidate
actions. Both logical facets of an intra-actor message retain their positions.
This is immutable projection data, not a runtime holder or completion table. -/
def Choreo.canonicalActorRollRows?
    (choreo : Choreo) (actor : Nat) : Option (List DecodedRollRow) := do
  let ownership ← choreo.ownershipSource?
  let normalized ← EndpointDelegation.normalize ownership
  let actions := ownership.actorCandidateActions normalized.paths actor
  let source := canonicalProgramSource choreo
  pure ((List.range source.markers.length).filterMap fun index => do
    let marker ← source.markers[index]?
    if marker.scope / 8192 = 1 && marker.tag % 4 = 0 &&
        !(source.markers.take index).any (fun earlier =>
          earlier.scope == marker.scope && earlier.tag % 4 == 0) then
      let stop := scopeSegmentEnd source.markers index source.atoms.length marker
      let positions := (List.range actions.length).filter fun position =>
        match actions[position]? with
        | some (action, _) => marker.offset ≤ action.event && action.event < stop
        | none => false
      let first ← positions.head?
      pure { scope := marker.scope % 8192, eventStart := first, eventLength := positions.length }
    else none)

def RustDescriptorImage.actorRollCheck (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorRowsCheck choreo && image.rollColumnsCoherent &&
    match choreo.canonicalActorRollRows? image.role, image.decodeRollRows? with
    | some expected, some actual => expected == actual
    | _, _ => false

theorem accepted_actor_rolls_bind_source_ranges
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorRollCheck choreo = true) :
    image.actorRowsCheck choreo = true ∧ image.rollColumnsCoherent = true ∧
    ∃ rows, choreo.canonicalActorRollRows? image.role = some rows ∧
      image.decodeRollRows? = some rows := by
  unfold RustDescriptorImage.actorRollCheck at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨accepted.1.1, accepted.1.2, ?_⟩
  cases expectedAt : choreo.canonicalActorRollRows? image.role with
  | none => simp [expectedAt] at accepted
  | some expected =>
      cases actualAt : image.decodeRollRows? with
      | none => simp [expectedAt, actualAt] at accepted
      | some actual =>
          have same : expected = actual := beq_iff_eq.mp
            (by simpa [expectedAt, actualAt] using accepted.2)
          exact ⟨expected, rfl, by simp only [same]⟩

/-- Exact source ranges and backward source dependencies discharge the range
and ordering premises of fresh-prefix preparation. This concerns done bits;
eligibility, route reset and transfer publication require their own checks. -/
theorem accepted_actor_roll_reset_preserves_source_prefix
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : EndpointDelegation.NormalizedPath} {rows : List DecodedRollRow}
    {row : DecodedRollRow} {state : CommitState} {roll : RollInfo} {candidate : Nat}
    (ordered : image.actorOrderCheck choreo = true)
    (ranges : image.actorRollCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathMember : path ∈ data.normalized.paths)
    (decodedRows : image.decodeRollRows? = some rows) (rowMember : row ∈ rows)
    (resetEvents : roll.events = eventRange row.eventStart row.eventLength)
    (candidateBound : candidate < row.eventStart + row.eventLength)
    (closedBefore : state.PrefixClosed (data.sourcePredecessors path)
      ((data.sourcePredecessors path candidate).filter (· < row.eventStart))) :
    ∃ sourceRows, choreo.canonicalActorRollRows? image.role = some sourceRows ∧
      row ∈ sourceRows ∧ (resetRoll state roll).PrefixClosed
        (data.sourcePredecessors path) (data.sourcePredecessors path candidate) := by
  obtain ⟨_, _, sourceRows, sourceAt, actualAt⟩ :=
    accepted_actor_rolls_bind_source_ranges ranges
  have sameRows : rows = sourceRows := Option.some.inj (decodedRows.symm.trans actualAt)
  refine ⟨sourceRows, sourceAt, by simpa only [← sameRows] using rowMember, ?_⟩
  have checks : data.normalized.paths.all data.pathOrderCheck = true := by
    have fields := ordered
    simp only [RustDescriptorImage.actorOrderCheck, decoded, Bool.and_eq_true] at fields
    exact fields.2
  have checkedPath := List.all_eq_true.mp checks path pathMember
  apply prepared_roll_is_source_prefix closedBefore candidateBound
  · intro event member
    exact checked_actor_source_predecessors_are_backward checkedPath candidate event member
  · intro event _ before member
    exact checked_actor_source_predecessors_are_backward checkedPath event before member
  · intro event
    rw [resetEvents]
    exact event_range_membership _ _ _

end Hibana
