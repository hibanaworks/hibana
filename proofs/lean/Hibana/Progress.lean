import Hibana.Commit

namespace Hibana

/-- Finite representation of the event and route state read by the Lean model. -/
structure CompactCommitState where
  done : List Bool
  selected : List (Option RouteArm)
  deriving DecidableEq

def CompactCommitState.initial (graph : EventGraph) : CompactCommitState := {
  done := List.replicate graph.events.length false
  selected := List.replicate graph.conflictCount none
}

/-- Accepted certificates make the total extension outside graph bounds unobservable. -/
def CompactCommitState.toCommitState (state : CompactCommitState) : CommitState := {
  done := fun event =>
    match state.done[event]? with
    | some done => done
    | none => false
  selected := fun conflict =>
    match state.selected[conflict]? with
    | some selected => selected
    | none => none
}

def CompactCommitState.fromCommitState
    (graph : EventGraph) (state : CommitState) : CompactCommitState := {
  done := (List.range graph.events.length).map state.done
  selected := (List.range graph.conflictCount).map state.selected
}

def CompactCommitState.canonical (graph : EventGraph) (state : CompactCommitState) : Bool :=
  state.done.length == graph.events.length &&
  state.selected.length == graph.conflictCount

private def progressEventExcluded
    (state : CommitState) (event : Event) : Bool :=
  event.conflicts.any fun membership =>
    match state.selected membership.conflict with
    | none => false
    | some selected => selected != membership.arm

def logicalComplete (graph : EventGraph) (state : CompactCommitState) : Bool :=
  let decoded := state.toCommitState
  graph.events.all fun event =>
    decoded.done event.id || progressEventExcluded decoded event

private def resolverSuccessorsForRoute
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) : List CompactCommitState :=
  match route.authority with
  | .intrinsic => []
  | .dynamic resolver =>
      [.left, .right].filterMap fun arm =>
        match applyResolver graph state route.conflict resolver (.select arm) with
        | some (.selected next) => some (CompactCommitState.fromCommitState graph next)
        | none | some .rejected => none

private def resolverRejectAvailableForRoute
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) : Bool :=
  match route.authority with
  | .intrinsic => false
  | .dynamic resolver =>
      match applyResolver graph state route.conflict resolver .reject with
      | some .rejected => true
      | none | some (.selected _) => false

def compactSuccessors
    (graph : EventGraph) (state : CompactCommitState) : List CompactCommitState :=
  let decoded := state.toCommitState
  let commits := (enabledLabels graph decoded).eraseDups.filterMap fun label =>
    (commitLabel graph decoded label).map (CompactCommitState.fromCommitState graph)
  let resolutions := graph.routes.flatMap (resolverSuccessorsForRoute graph decoded)
  commits ++ resolutions

def resolverRejectAvailable (graph : EventGraph) (state : CompactCommitState) : Bool :=
  let decoded := state.toCommitState
  graph.routes.any (resolverRejectAvailableForRoute graph decoded)

def LogicalProgress (graph : EventGraph) (state : CompactCommitState) : Prop :=
  logicalComplete graph state = true \/
  compactSuccessors graph state ≠ [] \/
  resolverRejectAvailable graph state = true

def checkLogicalProgress (graph : EventGraph) (state : CompactCommitState) : Bool :=
  logicalComplete graph state ||
  !(compactSuccessors graph state).isEmpty ||
  resolverRejectAvailable graph state

theorem logical_progress_checker_sound
    {graph : EventGraph} {state : CompactCommitState}
    (accepted : checkLogicalProgress graph state = true) :
    LogicalProgress graph state := by
  simp [checkLogicalProgress] at accepted
  rcases accepted with (complete | successor) | reject
  · exact Or.inl complete
  · exact Or.inr (Or.inl successor)
  · exact Or.inr (Or.inr reject)

structure ProgressCertificate where
  states : List CompactCommitState

private def compactMember (target : CompactCommitState) : List CompactCommitState -> Bool
  | [] => false
  | state :: rest => decide (state = target) || compactMember target rest

private theorem compact_member_sound
    {target : CompactCommitState} {states : List CompactCommitState}
    (accepted : compactMember target states = true) : target ∈ states := by
  induction states with
  | nil => simp [compactMember] at accepted
  | cons state rest ih =>
      simp [compactMember] at accepted
      rcases accepted with same | tail
      · simpa using Or.inl same.symm
      · simpa using Or.inr (ih tail)

def ProgressCertificate.Closed
    (graph : EventGraph) (certificate : ProgressCertificate) : Prop :=
  graph.WellFormed /\
  CompactCommitState.initial graph ∈ certificate.states /\
  certificate.states.Nodup /\
  forall state, state ∈ certificate.states ->
    state.canonical graph = true /\
    LogicalProgress graph state /\
    forall next, next ∈ compactSuccessors graph state -> next ∈ certificate.states

def ProgressCertificate.check
    (graph : EventGraph) (certificate : ProgressCertificate) : Bool :=
  graph.check &&
  compactMember (CompactCommitState.initial graph) certificate.states &&
  decide certificate.states.Nodup &&
  certificate.states.all fun state =>
    state.canonical graph &&
    checkLogicalProgress graph state &&
    (compactSuccessors graph state).all fun next => compactMember next certificate.states

theorem progress_certificate_sound
    {graph : EventGraph} {certificate : ProgressCertificate}
    (accepted : certificate.check graph = true) : certificate.Closed graph := by
  have parsed :
      ((graph.check = true /\
        compactMember (CompactCommitState.initial graph) certificate.states = true) /\
        certificate.states.Nodup) /\
      (forall state, state ∈ certificate.states ->
        (state.canonical graph = true /\
          checkLogicalProgress graph state = true) /\
        forall next, next ∈ compactSuccessors graph state ->
          compactMember next certificate.states = true) := by
    simpa [ProgressCertificate.check] using accepted
  rcases parsed with ⟨⟨⟨graphAccepted, initial⟩, unique⟩, states⟩
  refine ⟨artifact_checker_sound graphAccepted, compact_member_sound initial, unique, ?_⟩
  intro state member
  have checked := states state member
  refine ⟨checked.1.1, logical_progress_checker_sound checked.1.2, ?_⟩
  intro next successor
  exact compact_member_sound (checked.2 next successor)

inductive CompactReachable (graph : EventGraph) : CompactCommitState -> Prop where
  | initial : CompactReachable graph (CompactCommitState.initial graph)
  | step {state next} :
      CompactReachable graph state ->
      next ∈ compactSuccessors graph state ->
      CompactReachable graph next

theorem progress_certificate_covers_reachable
    {graph : EventGraph} {certificate : ProgressCertificate}
    (closed : certificate.Closed graph)
    {state : CompactCommitState}
    (reachable : CompactReachable graph state) : state ∈ certificate.states := by
  induction reachable with
  | initial => exact closed.2.1
  | step prior successor ih => exact (closed.2.2.2 _ ih).2.2 _ successor

/-- Every model-reachable state covered by an accepted closure can complete, step, or reject. -/
theorem reachable_state_has_logical_progress
    {graph : EventGraph} {certificate : ProgressCertificate}
    (accepted : certificate.check graph = true)
    {state : CompactCommitState}
    (reachable : CompactReachable graph state) : LogicalProgress graph state := by
  have closed := progress_certificate_sound accepted
  have member := progress_certificate_covers_reachable closed reachable
  exact (closed.2.2.2 state member).2.1

private def enqueueFresh
    (candidates pending visited : List CompactCommitState) : List CompactCommitState :=
  candidates.foldl (fun queued candidate =>
    if compactMember candidate visited || compactMember candidate queued then
      queued
    else
      queued ++ [candidate]) pending

/-- Each event has two completion states and each route has three authority states. -/
def progressStateBound (graph : EventGraph) : Nat :=
  2 ^ graph.events.length * 3 ^ graph.conflictCount

def discoverProgressStates
    (graph : EventGraph) : Nat -> List CompactCommitState -> List CompactCommitState ->
      List CompactCommitState
  | 0, _, visited => visited
  | _ + 1, [], visited => visited
  | fuel + 1, state :: pending, visited =>
      if compactMember state visited then
        discoverProgressStates graph fuel pending visited
      else
        let nextVisited := visited ++ [state]
        let nextPending := enqueueFresh (compactSuccessors graph state) pending nextVisited
        discoverProgressStates graph fuel nextPending nextVisited

def buildProgressCertificate (graph : EventGraph) : ProgressCertificate := {
  states := discoverProgressStates graph (progressStateBound graph)
    [CompactCommitState.initial graph] []
}

end Hibana
