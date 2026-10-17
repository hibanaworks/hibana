import Hibana.OwnershipDescriptor
import Hibana.DescriptorTopology

namespace Hibana

/-- A property of the existing shared progress on a source dependency cut.
Unrelated parallel work and old continuations are not evidence for this cut.
There is no second completion store. -/
def CommitState.PrefixClosed
    (state : CommitState) (predecessors : Nat → List Nat) (cut : List Nat) : Prop :=
  ∀ event ∈ cut, state.done event = true →
    ∀ before ∈ predecessors event, state.done before = true

def predecessorCoverage (expected : Nat → List Nat) (actual : List Nat) (event : Nat) : Bool :=
  actual.all (fun before => (expected event).contains before) &&
    (expected event).all fun before =>
      actual.any fun middle => middle == before || (expected middle).contains before

/-- A compact dependency may require an intermediate operation instead of all
its ancestors. On a source-closed prefix this preserves exactly the same next
operations, without serializing independent source operations. -/
theorem covered_predecessors_have_equal_readiness
    {expected : Nat → List Nat} {actual : List Nat} {event : Nat} {state : CommitState}
    (coverage : predecessorCoverage expected actual event = true)
    (closed : state.PrefixClosed expected (expected event)) :
    actual.all state.done = true ↔ (expected event).all state.done = true := by
  simp only [predecessorCoverage, Bool.and_eq_true] at coverage
  constructor
  · intro ready
    apply List.all_eq_true.mpr
    intro before member
    obtain ⟨middle, parent, covers⟩ := List.any_eq_true.mp
      (List.all_eq_true.mp coverage.2 before member)
    have done := List.all_eq_true.mp ready middle parent
    have middleExpected : middle ∈ expected event := by
      simpa using List.all_eq_true.mp coverage.1 middle parent
    have either : middle = before ∨ before ∈ expected middle := by simpa using covers
    rcases either with same | preceding
    · exact same ▸ done
    · exact closed middle middleExpected done before preceding
  · intro ready
    apply List.all_eq_true.mpr
    intro before member
    have belongs : before ∈ expected event := by
      simpa using List.all_eq_true.mp coverage.1 before member
    exact List.all_eq_true.mp ready before belongs

theorem initial_progress_is_source_prefix (predecessors : Nat → List Nat) (cut : List Nat) :
    CommitState.initial.PrefixClosed predecessors cut := by
  intro event _ completed
  simp [CommitState.initial] at completed

/-- These are the exact done-bit facts exposed by `commit_exact_successor`.
Reentry must establish the property for its prepared base separately. -/
theorem actor_commit_delta_preserves_source_prefix
    {predecessors : Nat → List Nat} {base next : CommitState} {event : Nat} {cut : List Nat}
    (closed : base.PrefixClosed predecessors cut)
    (ready : (predecessors event).all base.done = true)
    (completed : next.done event = true)
    (unchanged : ∀ other, other ≠ event → next.done other = base.done other) :
    next.PrefixClosed predecessors cut := by
  intro target targetBound targetDone before member
  by_cases current : target = event
  · subst target
    have wasDone := List.all_eq_true.mp ready before member
    by_cases same : before = event
    · simpa [same] using completed
    · simpa [unchanged before same] using wasDone
  · have priorDone : base.done target = true := by
      simpa [unchanged target current] using targetDone
    have wasDone := closed target targetBound priorDone before member
    by_cases same : before = event
    · simpa [same] using completed
    · simpa [unchanged before same] using wasDone

/-- Resetting the exact body range establishes the candidate's fresh dependency
cut without clearing unrelated parallel work or old continuations. Its outside
ancestors precede the body, so none of their own predecessors is reset. -/
theorem prepared_roll_is_source_prefix
    {state : CommitState} {roll : RollInfo} {predecessors : Nat → List Nat}
    {first stop candidate : Nat}
    (closedBefore : state.PrefixClosed predecessors
      ((predecessors candidate).filter (· < first)))
    (candidateBound : candidate < stop)
    (ancestors : ∀ event ∈ predecessors candidate, event < candidate)
    (ordered : ∀ event ∈ predecessors candidate,
      ∀ before ∈ predecessors event, before < event)
    (region : ∀ event, event ∈ roll.events ↔ first ≤ event ∧ event < stop) :
    (resetRoll state roll).PrefixClosed predecessors (predecessors candidate) := by
  intro event eventMember completed before member
  have outside : event ∉ roll.events := by
    intro inside
    have cleared := reset_roll_clears_done state roll event inside
    rw [cleared] at completed
    contradiction
  have eventStop : event < stop := Nat.lt_trans (ancestors event eventMember) candidateBound
  have eventBefore : event < first := Nat.lt_of_not_ge fun beyond =>
    outside ((region event).mpr ⟨beyond, eventStop⟩)
  have priorDone : state.done event = true := by
    simpa only [reset_roll_preserves_outside_done state roll event outside] using completed
  have parentBefore : before < first :=
    Nat.lt_trans (ordered event eventMember before member) eventBefore
  have parentOutside : before ∉ roll.events := by
    intro inside
    exact (Nat.not_le_of_gt parentBefore) ((region before).mp inside).1
  have inCut : event ∈ (predecessors candidate).filter (· < first) := by
    simp only [List.mem_filter, decide_eq_true_eq]
    exact ⟨eventMember, eventBefore⟩
  have parentDone := closedBefore event inCut priorDone before member
  simpa only [reset_roll_preserves_outside_done state roll before parentOutside] using parentDone

/-- Source and decoded immutable columns used by one translation check.
The only execution state consumed by the readiness theorem is CommitState. -/
structure DecodedActorOrder where
  source : EndpointDelegation.Source
  normalized : EndpointDelegation.Normalization
  actor : Nat
  columns : DecodedProgramColumns
  markers : List DecodedScopeMarker
  rows : List DecodedActorRow
  lanes : List Nat
  eventDependencies : List Nat
  dependencies : List DecodedDependencyRow

def RustDescriptorImage.decodeActorOrder? (image : RustDescriptorImage) (choreo : Choreo) :
    Option DecodedActorOrder := do
  let source ← choreo.ownershipSource?
  let normalized ← EndpointDelegation.normalize source
  let columns ← image.decodeProgramColumns?
  let markers ← image.decodeScopeMarkers?
  let rows ← image.decodeActorRows?
  let lanes ← (List.range image.eventCount).mapM image.eventLane?
  let eventDependencies ← image.decodeEventDependencyRows?
  let dependencies ← image.decodeDependencyRows?
  pure {
    source, normalized, actor := image.role, columns, markers, rows
    lanes, eventDependencies, dependencies
  }

def DecodedActorOrder.activeRows
    (data : DecodedActorOrder) (path : EndpointDelegation.NormalizedPath) : List DecodedActorRow :=
  data.columns.actorRowsForPath data.markers path data.actor data.rows

def DecodedActorOrder.sourcePredecessors
    (data : DecodedActorOrder) (path : EndpointDelegation.NormalizedPath) (event : Nat) : List Nat :=
  let active := data.activeRows path
  active.filterMap fun before =>
    if active.any (fun after => after.eventId == event &&
        EndpointDelegation.precedes data.source path.bindings before.actionId after.actionId) then
      some before.eventId
    else none

private def dependencyApplies
    (markers : List DecodedScopeMarker) (path : EndpointDelegation.NormalizedPath)
    (conflict : Nat) : Bool :=
  match conflict % 4 with
  | 0 => true
  | 1 => false
  | 2 => sourcePathArm? markers path (conflict / 4) == some .left
  | _ => sourcePathArm? markers path (conflict / 4) == some .right

/-- Read the same lane-prefix and dependency-row guards as Rust's single-visit
admission. Route membership and conditional actors use one complete source
path. This does not model resetting a completed roll or previewing a new arm. -/
def DecodedActorOrder.runtimePredecessors?
    (data : DecodedActorOrder) (path : EndpointDelegation.NormalizedPath) (event : Nat) :
    Option (List Nat) := do
  let lane ← data.lanes[event]?
  let dependencyIndex ← data.eventDependencies[event]?
  let dependency ← if dependencyIndex = packedU16Absent then some none else do
    let row ← data.dependencies[dependencyIndex]?
    if dependencyApplies data.markers path row.conflict then
      if row.stop ≤ event then some (some row) else none
    else some none
  let located ← (data.activeRows path).mapM fun row => do
    pure (row.eventId, ← data.lanes[row.eventId]?)
  pure (located.filterMap fun (before, beforeLane) =>
    let inDependency := match dependency with
      | none => false
      | some row => row.start ≤ before && before < row.stop
    if (before < event && beforeLane == lane) || inDependency then some before else none)

def DecodedActorOrder.pathOrderCheck
    (data : DecodedActorOrder) (path : EndpointDelegation.NormalizedPath) : Bool :=
  (data.activeRows path).all fun row =>
    match data.runtimePredecessors? path row.eventId with
    | none => false
    | some actual =>
        (data.sourcePredecessors path row.eventId).all (· < row.eventId) &&
          predecessorCoverage (data.sourcePredecessors path) actual row.eventId

/-- The checked source order is backward for every candidate, including
indices absent from this path, whose predecessor list is empty. -/
theorem checked_actor_source_predecessors_are_backward
    {data : DecodedActorOrder} {path : EndpointDelegation.NormalizedPath}
    (checked : data.pathOrderCheck path = true)
    (event before : Nat) (member : before ∈ data.sourcePredecessors path event) :
    before < event := by
  obtain ⟨prior, _, found⟩ := List.mem_filterMap.mp member
  split at found
  next hasAfter =>
    obtain ⟨after, afterMember, relates⟩ := List.any_eq_true.mp hasAfter
    have same : after.eventId = event := by
      simp only [Bool.and_eq_true, beq_iff_eq] at relates
      exact relates.1
    have guards := List.all_eq_true.mp checked after afterMember
    cases actualAt : data.runtimePredecessors? path after.eventId with
    | none => simp [actualAt] at guards
    | some actual =>
        have earlier : (data.sourcePredecessors path after.eventId).all
            (· < after.eventId) = true := by
          have fields :
              (data.sourcePredecessors path after.eventId).all (· < after.eventId) = true ∧
              predecessorCoverage (data.sourcePredecessors path) actual after.eventId = true := by
            simpa only [actualAt, Bool.and_eq_true] using guards
          exact fields.1
        have bounded := of_decide_eq_true (List.all_eq_true.mp earlier before
          (by simpa only [same] using member))
        simpa only [same] using bounded
  next impossible => simp at found

/-- Necessary single-visit source-order refinement for exact owned bytes.
Full route authority, roll reset and ownership-transfer refinement are separate
parts of qualification; successful row ordering alone does not establish them. -/
def RustDescriptorImage.actorOrderCheck (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorRowsCheck choreo && image.laneColumnsCoherent &&
    match image.decodeActorOrder? choreo with
    | none => false
    | some data => data.normalized.paths.all data.pathOrderCheck

theorem accepted_actor_order_binds_source_readiness
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorOrderCheck choreo = true) :
    image.actorRowsCheck choreo = true ∧
    ∃ data, image.decodeActorOrder? choreo = some data ∧
      ∀ path ∈ data.normalized.paths, ∀ row ∈ data.activeRows path,
        ∃ actual, data.runtimePredecessors? path row.eventId = some actual ∧
          (∀ before ∈ data.sourcePredecessors path row.eventId, before < row.eventId) ∧
          ∀ state : CommitState,
            state.PrefixClosed (data.sourcePredecessors path)
              (data.sourcePredecessors path row.eventId) →
            (actual.all state.done = true ↔
              (data.sourcePredecessors path row.eventId).all state.done = true) := by
  unfold RustDescriptorImage.actorOrderCheck at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨accepted.1.1, ?_⟩
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [dataAt] at accepted
  | some data =>
      refine ⟨data, rfl, ?_⟩
      have paths : data.normalized.paths.all data.pathOrderCheck = true := by
        simpa [dataAt] using accepted.2
      intro path pathMember row rowMember
      have checked := List.all_eq_true.mp (List.all_eq_true.mp paths path pathMember) row rowMember
      cases parentsAt : data.runtimePredecessors? path row.eventId with
      | none => simp [parentsAt] at checked
      | some actual =>
          have fields : (data.sourcePredecessors path row.eventId).all (· < row.eventId) = true ∧
              predecessorCoverage (data.sourcePredecessors path) actual row.eventId = true := by
            simpa [parentsAt] using checked
          refine ⟨actual, rfl, ?_, ?_⟩
          · intro before member
            exact of_decide_eq_true (List.all_eq_true.mp fields.1 before member)
          · intro state closed
            exact covered_predecessors_have_equal_readiness fields.2 closed

end Hibana
