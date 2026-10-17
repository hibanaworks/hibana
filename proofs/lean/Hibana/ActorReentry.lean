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

/-- The source scope determines which choices belong to this visit. An outer
choice is not reset just because some of its actions project into the body. -/
def sourceRollResetsRoute?
    (source : CanonicalProgramSource) (rollScope routeScope : Nat) : Option Bool := do
  let index ← (List.range source.markers.length).find? fun index =>
    match source.markers[index]? with
    | some marker => marker.scope == 8192 + rollScope && marker.tag % 4 == 0
    | none => false
  let marker ← source.markers[index]?
  let stop := scopeSegmentEnd source.markers index source.atoms.length marker
  let (routeStart, routeStop) ← routeBounds? source.markers routeScope
  pure (marker.offset ≤ routeStart && routeStop ≤ stop)

/-- Decode the interval comparison used by the production cursor. Empty
local route slots have no interval and cannot be cleared by that predicate. -/
def RustDescriptorImage.rollResetsRoute?
    (image : RustDescriptorImage) (roll : DecodedRollRow) (slot : Nat) : Option Bool := do
  let left ← image.decodeRouteArmEvents? slot 0
  let right ← image.decodeRouteArmEvents? slot 1
  match left ++ right with
  | [] => pure false
  | first :: rest =>
      let bounds := rest.foldl (fun (start, stop) event =>
        (min start event, max stop (event + 1))) (first, first + 1)
      pure (roll.eventStart ≤ bounds.1 && bounds.2 ≤ roll.eventStart + roll.eventLength)

def RustDescriptorImage.rollChoicesMatchSource
    (image : RustDescriptorImage) (source : CanonicalProgramSource)
    (rows : List DecodedRollRow) : Bool :=
  rows.all fun roll => (List.range image.routeScopeCount).all fun slot =>
    match image.decodeRouteScope? slot, image.rollResetsRoute? roll slot with
    | some scope, some actual => sourceRollResetsRoute? source roll.scope scope == some actual
    | _, _ => false

def RustDescriptorImage.actorRollCheck (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorRowsCheck choreo && image.rollColumnsCoherent &&
    match choreo.canonicalActorRollRows? image.role, image.decodeRollRows? with
    | some expected, some actual => expected == actual &&
        image.rollChoicesMatchSource (canonicalProgramSource choreo) actual
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
          have fields : (expected == actual) = true ∧
              image.rollChoicesMatchSource (canonicalProgramSource choreo) actual = true := by
            simpa only [expectedAt, actualAt, Bool.and_eq_true] using accepted.2
          have same : expected = actual := beq_iff_eq.mp
            fields.1
          exact ⟨expected, rfl, by simp only [same]⟩

theorem accepted_actor_roll_reset_binds_source_choices
    {image : RustDescriptorImage} {choreo : Choreo} {rows : List DecodedRollRow}
    {roll : DecodedRollRow} {slot : Nat}
    (accepted : image.actorRollCheck choreo = true)
    (decoded : image.decodeRollRows? = some rows) (member : roll ∈ rows)
    (slotBound : slot ∈ List.range image.routeScopeCount) :
    ∃ scope actual, image.decodeRouteScope? slot = some scope ∧
      image.rollResetsRoute? roll slot = some actual ∧
      sourceRollResetsRoute? (canonicalProgramSource choreo) roll.scope scope = some actual := by
  unfold RustDescriptorImage.actorRollCheck at accepted
  simp only [Bool.and_eq_true] at accepted
  cases sourceAt : choreo.canonicalActorRollRows? image.role with
  | none => simp [sourceAt] at accepted
  | some expected =>
      have fields : (expected == rows) = true ∧
          image.rollChoicesMatchSource (canonicalProgramSource choreo) rows = true := by
        simpa only [sourceAt, decoded, Bool.and_eq_true] using accepted.2
      have checked := List.all_eq_true.mp
        (List.all_eq_true.mp fields.2 roll member) slot slotBound
      cases scopeAt : image.decodeRouteScope? slot with
      | none => simp [scopeAt] at checked
      | some scope =>
          cases resetAt : image.rollResetsRoute? roll slot with
          | none => simp [scopeAt, resetAt] at checked
          | some actual =>
              exact ⟨scope, actual, rfl, rfl, by simpa [scopeAt, resetAt] using checked⟩

/-- A new visit may change every choice inside the reset body. Only choices
outside that body constrain its source path. This relates the existing choice
function to source paths; it introduces no epoch, holder map or progress state. -/
theorem reset_roll_choices_match_next_source_path
    {markers : List DecodedScopeMarker} {previous next : EndpointDelegation.NormalizedPath}
    {state : CommitState} {roll : RollInfo}
    (previousAgrees : ∀ scope arm, state.selected scope = some arm →
      sourcePathArm? markers previous scope = some arm)
    (outsideAgrees : ∀ scope, scope ∉ roll.conflicts →
      sourcePathArm? markers previous scope = sourcePathArm? markers next scope) :
    ∀ scope arm, (resetRoll state roll).selected scope = some arm →
      sourcePathArm? markers next scope = some arm := by
  intro scope arm known
  by_cases inside : scope ∈ roll.conflicts
  · simp [resetRoll, inside] at known
  · have old : state.selected scope = some arm := by
      simpa [resetRoll, inside] using known
    rw [← outsideAgrees scope inside]
    exact previousAgrees scope arm old

theorem reentered_source_path_binds_partially_known_actors
    {columns : DecodedProgramColumns} {markers : List DecodedScopeMarker}
    {previous next : EndpointDelegation.NormalizedPath} {state : CommitState} {roll : RollInfo}
    {event : Nat} {binding : EndpointDelegation.Binding}
    (acceptedPath : columns.sourcePathMatches markers next = true)
    (previousAgrees : ∀ scope arm, state.selected scope = some arm →
      sourcePathArm? markers previous scope = some arm)
    (outsideAgrees : ∀ scope, scope ∉ roll.conflicts →
      sourcePathArm? markers previous scope = sourcePathArm? markers next scope)
    (member : (event, binding) ∈ next.bindings) :
    ∃ sender receiver, columns.actorBindings[event]? = some (sender, receiver) ∧
      (∀ actor, columns.selectedActor? (resetRoll state roll).selected sender = some actor →
        actor = binding.senderActor) ∧
      (∀ actor, columns.selectedActor? (resetRoll state roll).selected receiver = some actor →
        actor = binding.receiverActor) := by
  exact matching_source_path_binds_partially_known_actors acceptedPath
    (reset_roll_choices_match_next_source_path previousAgrees outsideAgrees) member

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
