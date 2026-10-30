import Hibana.OwnershipDescriptor
import Hibana.DescriptorTopology
import Hibana.OwnedEntry

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

/-- One source/local edge obligation, as checked at the typed Rust projection
boundary. A missing direct edge can be covered by an actual predecessor whose
source prefix already contains the required action. -/
def predecessorPairCheck
    (expected : Nat → List Nat) (actual : List Nat) (event before : Nat) : Bool :=
  if actual.contains before then (expected event).contains before
  else if (expected event).contains before then
    actual.any fun middle => (expected middle).contains before
  else true

theorem pair_validation_is_exact_predecessor_coverage
    (expected : Nat → List Nat) (actual : List Nat) (event : Nat) :
    (actual ++ expected event).all (predecessorPairCheck expected actual event) = true ↔
      predecessorCoverage expected actual event = true := by
  constructor
  · intro checked
    have each := List.all_eq_true.mp checked
    apply Bool.and_eq_true_iff.mpr
    constructor
    · apply List.all_eq_true.mpr
      intro before member
      have present : actual.contains before = true := by simpa using member
      simpa only [predecessorPairCheck, present, Bool.true_eq, ite_true] using
        each before (List.mem_append.mpr (.inl member))
    · apply List.all_eq_true.mpr
      intro before member
      by_cases direct : before ∈ actual
      · have same : (before == before) = true := decide_eq_true rfl
        exact List.any_eq_true.mpr ⟨before, direct, Bool.or_eq_true_iff.mpr (.inl same)⟩
      · have absent : actual.contains before = false := by simpa using direct
        have needed : (expected event).contains before = true := by simpa using member
        have ancestors : actual.any (fun middle => (expected middle).contains before) = true := by
          simpa only [predecessorPairCheck, absent, needed,
            Bool.false_eq_true, Bool.true_eq, ite_false, ite_true] using
            each before (List.mem_append.mpr (.inr member))
        obtain ⟨middle, belongs, ancestor⟩ := List.any_eq_true.mp ancestors
        exact List.any_eq_true.mpr ⟨middle, belongs, Bool.or_eq_true_iff.mpr (.inr ancestor)⟩
  · intro checked
    have fields := Bool.and_eq_true_iff.mp checked
    apply List.all_eq_true.mpr
    intro before _
    cases direct : actual.contains before with
    | true =>
        have belongs : before ∈ actual := by simpa using direct
        have needed := List.all_eq_true.mp fields.1 before belongs
        simpa only [predecessorPairCheck, direct, Bool.true_eq, ite_true] using needed
    | false =>
        cases needed : (expected event).contains before with
        | false => simp only [predecessorPairCheck, direct, needed,
            Bool.false_eq_true, ite_false]
        | true =>
            have belongs : before ∈ expected event := by simpa using needed
            obtain ⟨middle, member, covers⟩ :=
              List.any_eq_true.mp (List.all_eq_true.mp fields.2 before belongs)
            have ancestor : (expected middle).contains before = true := by
              rcases Bool.or_eq_true_iff.mp covers with same | previous
              · have equal : middle = before := beq_iff_eq.mp same
                have present : actual.contains before = true := by simpa [equal] using member
                rw [direct] at present
                contradiction
              · exact previous
            have covered : actual.any (fun middle => (expected middle).contains before) = true :=
              List.any_eq_true.mpr ⟨middle, member, ancestor⟩
            simpa only [predecessorPairCheck, direct, needed,
              Bool.false_eq_true, Bool.true_eq, ite_false, ite_true] using covered

/-- A maximal active source predecessor of one operation. -/
def sourceCover (expected : Nat → List Nat) (event before : Nat) : Prop :=
  before ∈ expected event ∧
    ∀ middle ∈ expected event, before ∉ expected middle

private theorem source_predecessor_has_cover_above
    {expected : Nat → List Nat} {event : Nat}
    (backward : ∀ {a b : Nat}, a ∈ expected b → a < b)
    (transitive : ∀ {a b c : Nat},
      a ∈ expected b → b ∈ expected c → a ∈ expected c) :
    ∀ before, before ∈ expected event →
      ∃ cover, sourceCover expected event cover ∧
        (cover = before ∨ before ∈ expected cover) := by
  have bounded :
      ∀ fuel before, event - before = fuel →
        before ∈ expected event →
        ∃ cover, sourceCover expected event cover ∧
          (cover = before ∨ before ∈ expected cover) := by
    intro fuel
    induction fuel using Nat.strongRecOn with
    | ind fuel ih =>
        intro before distance member
        by_cases later :
            (expected event).any (fun middle => (expected middle).contains before) = true
        · obtain ⟨middle, middleMember, present⟩ := List.any_eq_true.mp later
          have preceding : before ∈ expected middle := by simpa using present
          have increases := backward preceding
          have boundedMiddle := backward middleMember
          have smaller : event - middle < fuel := by omega
          obtain ⟨cover, maximal, above⟩ :=
            ih (event - middle) smaller middle rfl middleMember
          refine ⟨cover, maximal, Or.inr ?_⟩
          rcases above with same | previous
          · simpa only [same] using preceding
          · exact transitive preceding previous
        · refine ⟨before, ⟨member, ?_⟩, Or.inl rfl⟩
          intro middle middleMember preceding
          exact later (List.any_eq_true.mpr ⟨middle, middleMember, by simpa using preceding⟩)
  intro before member
  exact bounded (event - before) before rfl member

/-- Positive obligations need only a superset of source covers. Every extra
actual predecessor must still be rejected. All lists refer to the same full
choice and holder assignment, within one source visit. -/
theorem cover_candidates_validate_predecessor_coverage
    {expected : Nat → List Nat}
    {actual candidates : List Nat} {event : Nat}
    (backward : ∀ {a b : Nat}, a ∈ expected b → a < b)
    (transitive : ∀ {a b c : Nat},
      a ∈ expected b → b ∈ expected c → a ∈ expected c)
    (noExtra : ∀ before ∈ actual, before ∈ expected event)
    (complete : ∀ before,
      sourceCover expected event before → before ∈ candidates)
    (checked : candidates.all
      (predecessorPairCheck expected actual event) = true) :
    predecessorCoverage expected actual event = true := by
  have coverActual :
      ∀ before, sourceCover expected event before → before ∈ actual := by
    intro before maximal
    by_cases member : before ∈ actual
    · exact member
    have absent := member
    have missing : actual.contains before = false := by simpa using absent
    have needed : (expected event).contains before = true := by simpa using maximal.1
    have ancestors :
        actual.any (fun middle => (expected middle).contains before) = true := by
      simpa only [predecessorPairCheck, missing, needed,
        Bool.false_eq_true, Bool.true_eq, ite_false, ite_true] using
        List.all_eq_true.mp checked before (complete before maximal)
    obtain ⟨middle, member, preceding⟩ := List.any_eq_true.mp ancestors
    exact False.elim (maximal.2 middle (noExtra middle member) (by simpa using preceding))
  apply Bool.and_eq_true_iff.mpr
  constructor
  · apply List.all_eq_true.mpr
    intro before member
    simpa using noExtra before member
  · apply List.all_eq_true.mpr
    intro before member
    obtain ⟨cover, maximal, above⟩ :=
      source_predecessor_has_cover_above backward transitive before member
    exact List.any_eq_true.mpr
      ⟨cover, coverActual cover maximal, by simpa using above⟩

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

/-- Cover candidates and all negative obligations preserve the enabled set on
the existing source-closed progress. -/
theorem checked_cover_candidates_have_equal_source_readiness
    {expected : Nat → List Nat} {actual candidates : List Nat}
    {event : Nat} {state : CommitState}
    (backward : ∀ {a b : Nat}, a ∈ expected b → a < b)
    (transitive : ∀ {a b c : Nat},
      a ∈ expected b → b ∈ expected c → a ∈ expected c)
    (noExtra : ∀ before ∈ actual, before ∈ expected event)
    (complete : ∀ before, sourceCover expected event before → before ∈ candidates)
    (checked : candidates.all
      (predecessorPairCheck expected actual event) = true)
    (closed : state.PrefixClosed expected (expected event)) :
    actual.all state.done = true ↔ (expected event).all state.done = true :=
  covered_predecessors_have_equal_readiness
    (cover_candidates_validate_predecessor_coverage
      backward transitive noExtra complete checked) closed

/-- Accepted pair obligations preserve exactly the source-enabled operations
on the existing source-closed progress. This does not require another cursor
or serialize independent parallel operations. -/
theorem checked_pairs_have_equal_source_readiness
    {expected : Nat → List Nat} {actual : List Nat} {event : Nat} {state : CommitState}
    (checked : (actual ++ expected event).all
      (predecessorPairCheck expected actual event) = true)
    (closed : state.PrefixClosed expected (expected event)) :
    actual.all state.done = true ↔ (expected event).all state.done = true :=
  covered_predecessors_have_equal_readiness
    ((pair_validation_is_exact_predecessor_coverage expected actual event).mp checked) closed

/-- A pending source ancestor still blocks a compact guard that covers it
through another join. A future holding that ancestor's view cannot classify
the wait as work that an independent view can finish. -/
theorem a_pending_source_ancestor_blocks_covered_local_guards
    {expected : Nat → List Nat} {actual : List Nat} {event before : Nat} {state : CommitState}
    (coverage : predecessorCoverage expected actual event = true)
    (closed : state.PrefixClosed expected (expected event))
    (ancestor : before ∈ expected event)
    (pending : state.done before = false) :
    actual.all state.done = false := by
  cases ready : actual.all state.done with
  | false => rfl
  | true =>
      have sourceReady := (covered_predecessors_have_equal_readiness coverage closed).mp ready
      have completed := List.all_eq_true.mp sourceReady before ancestor
      rw [pending] at completed
      contradiction

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
  if image.ownedRecoveryIndexCheck then pure () else none
  if image.ownedEntryResourcesCheck then pure () else none
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
