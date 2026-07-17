import Hibana.EventGraph

namespace Hibana

structure CommitState where
  done : Nat -> Bool
  selected : Nat -> Option RouteArm

def CommitState.initial : CommitState := {
  done := fun _ => false
  selected := fun _ => none
}

private def conflictAllows
    (graph : EventGraph) (state : CommitState) (membership : ConflictArm) : Bool :=
  match state.selected membership.conflict with
  | none =>
      match graph.routeForConflict? membership.conflict with
      | none => false
      | some route =>
          match route.authority with
          | .intrinsic => true
          | .dynamic _ => true
  | some selected => selected == membership.arm

private def commitAuthorityAllows
    (graph : EventGraph) (state : CommitState) (membership : ConflictArm) : Bool :=
  match graph.routeForConflict? membership.conflict with
  | none => false
  | some route =>
      match route.authority with
      | .intrinsic => true
      | .dynamic _ => state.selected membership.conflict == some membership.arm

private def eventExcluded (graph : EventGraph) (state : CommitState) (id : Nat) : Bool :=
  match graph.events[id]? with
  | none => false
  | some event => event.conflicts.any fun membership =>
      match state.selected membership.conflict with
      | none => false
      | some selected => selected != membership.arm

def resetRoll (state : CommitState) (roll : RollInfo) : CommitState := {
  done := fun event => if roll.events.contains event then false else state.done event
  selected := fun conflict =>
    if roll.conflicts.contains conflict then none else state.selected conflict
}

def resetRouteArm (state : CommitState) (events : List Nat) : CommitState := {
  done := fun event => if events.contains event then false else state.done event
  selected := state.selected
}

private def eventReady (graph : EventGraph) (state : CommitState) (event : Event) : Bool :=
  event.conflicts.all (conflictAllows graph state) &&
  event.deps.all fun dep => state.done dep || eventExcluded graph state dep

private def eventCommitReady
    (graph : EventGraph) (state : CommitState) (event : Event) : Bool :=
  eventReady graph state event &&
  event.conflicts.all (commitAuthorityAllows graph state)

private def rollComplete (graph : EventGraph) (state : CommitState) (roll : RollInfo) : Bool :=
  roll.events.all fun event => state.done event || eventExcluded graph state event

private def selectedRouteEvents?
    (state : CommitState) (route : RouteInfo) : Option (List Nat) :=
  match state.selected route.conflict with
  | none => none
  | some .left => some route.leftEvents
  | some .right => some route.rightEvents

private def routeComplete
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) : Bool :=
  match selectedRouteEvents? state route with
  | none => false
  | some events => events.all fun event => state.done event || eventExcluded graph state event

private def hasLaterSequentialDone
    (graph : EventGraph)
    (state : CommitState)
    (inner outer : RollInfo)
    (event : Event) : Bool :=
  outer.events.any fun candidateId =>
    candidateId > event.id && !inner.events.contains candidateId &&
      match graph.events[candidateId]? with
      | none => false
      | some candidate =>
          candidate.lane == event.lane && state.done candidateId &&
            !eventExcluded graph state candidateId

private def completedRollForEvent?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option RollInfo :=
  let completed := graph.rolls.filter fun roll =>
    roll.entries.contains event.id && rollComplete graph state roll
  match completed with
  | [] => none
  | inner :: outers =>
      some <| outers.foldl (fun selected candidate =>
        if candidate.events.length > selected.events.length &&
            hasLaterSequentialDone graph state selected candidate event
        then candidate
        else selected) inner

private def rollReentryState?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option CommitState :=
  match completedRollForEvent? graph state event with
  | none => none
  | some roll =>
      if eventReady graph state event then
        let reset := resetRoll state roll
        if eventReady graph reset event then some reset else none
      else
        none

/-- Conflict identifiers follow choreography preorder, so the greatest
candidate conflict is the innermost route containing one event. Maximal-key
duplicates are malformed and fail closed through `resolveUnique?`. -/
def maxRouteConflict (routes : List RouteInfo) : Nat :=
  routes.foldl (fun current route => max current route.conflict) 0

def selectInnermostReentryRoute? (routes : List RouteInfo) : Option RouteInfo :=
  let innermostConflict := maxRouteConflict routes
  resolveUnique? (routes.filter fun route => Nat.beq route.conflict innermostConflict)

theorem max_route_conflict_is_permutation_invariant
    {left right : List RouteInfo}
    (permutation : left.Perm right) :
    maxRouteConflict left = maxRouteConflict right := by
  unfold maxRouteConflict
  apply permutation.foldl_eq'
  intro first _ second _ current
  omega

theorem select_innermost_reentry_route_eq_some_iff
    {routes : List RouteInfo} {route : RouteInfo} :
    selectInnermostReentryRoute? routes = some route ↔
      routes.filter (fun candidate =>
        Nat.beq candidate.conflict (maxRouteConflict routes)) = [route] := by
  exact resolveUnique?_eq_some_iff

/-- Route reentry authority depends on the canonical conflict key and singleton
evidence, never on candidate traversal order. -/
theorem select_innermost_reentry_route_is_permutation_invariant
    {left right : List RouteInfo} {route : RouteInfo}
    (permutation : left.Perm right)
    (selected : selectInnermostReentryRoute? left = some route) :
    selectInnermostReentryRoute? right = some route := by
  have maxEq := max_route_conflict_is_permutation_invariant permutation
  have exactLeft := select_innermost_reentry_route_eq_some_iff.mp selected
  apply select_innermost_reentry_route_eq_some_iff.mpr
  have filtered := permutation.filter fun candidate =>
    Nat.beq candidate.conflict (maxRouteConflict left)
  rw [maxEq] at filtered exactLeft
  exact List.Perm.eq_singleton (exactLeft ▸ filtered).symm

theorem select_innermost_reentry_route_pair_is_order_independent
    (outer inner : RouteInfo)
    (nested : outer.conflict < inner.conflict) :
    selectInnermostReentryRoute? [outer, inner] = some inner ∧
      selectInnermostReentryRoute? [inner, outer] = some inner := by
  have outerNeInner : outer.conflict ≠ inner.conflict := Nat.ne_of_lt nested
  simp [selectInnermostReentryRoute?, maxRouteConflict, resolveUnique?,
    Nat.max_eq_right (Nat.le_of_lt nested), Nat.max_eq_left (Nat.le_of_lt nested),
    outerNeInner, Nat.beq_refl]

private def routeReentryState?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option CommitState :=
  let candidates := graph.routes.filter fun route =>
    route.reentry == .rolled && routeComplete graph state route &&
      match selectedRouteEvents? state route with
      | none => false
      | some events => events.contains event.id
  match selectInnermostReentryRoute? candidates with
  | none => none
  | some route =>
      match selectedRouteEvents? state route with
      | none => none
      | some events =>
          let reset := resetRouteArm state events
          if eventReady graph reset event then some reset else none

private def reentryState?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option CommitState :=
  match rollReentryState? graph state event with
  | some reset => some reset
  | none => routeReentryState? graph state event

private def eventCandidateBaseState?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option CommitState :=
  if !state.done event.id && eventReady graph state event then
    some state
  else
    reentryState? graph state event

private def eventBaseState?
    (graph : EventGraph) (state : CommitState) (event : Event) : Option CommitState :=
  match eventCandidateBaseState? graph state event with
  | none => none
  | some base => if eventCommitReady graph base event then some base else none

def eventEnabled (graph : EventGraph) (state : CommitState) (event : Event) : Bool :=
  (eventCandidateBaseState? graph state event).isSome

private def eventCommitEnabled
    (graph : EventGraph) (state : CommitState) (event : Event) : Bool :=
  (eventBaseState? graph state event).isSome

def applyConflictArms
    (memberships : List ConflictArm)
    (selected : Nat -> Option RouteArm) : Nat -> Option RouteArm :=
  memberships.foldl
    (fun current membership conflict =>
      if conflict == membership.conflict then some membership.arm else current conflict)
    selected

def commitEvent
    (graph : EventGraph)
    (state : CommitState)
    (id : Nat) : Option CommitState :=
  match graph.events[id]? with
  | none => none
  | some event =>
      match eventBaseState? graph state event with
      | none => none
      | some base =>
        some {
          done := fun candidate => if candidate == id then true else base.done candidate
          selected := applyConflictArms event.conflicts base.selected
        }

private def selectRouteArm
    (state : CommitState) (conflict : Nat) (arm : RouteArm) : CommitState := {
  done := state.done
  selected := fun candidate =>
    if candidate == conflict then some arm else state.selected candidate
}

private def routeArmEvents (route : RouteInfo) : RouteArm -> List Nat
  | .left => route.leftEvents
  | .right => route.rightEvents

private def routeArmReady
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) (arm : RouteArm) : Bool :=
  let selected := selectRouteArm state route.conflict arm
  (routeArmEvents route arm).any fun id =>
    match graph.events[id]? with
    | none => false
    | some event => eventReady graph selected event

private def routeResolvable
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) : Bool :=
  state.selected route.conflict == none &&
  match route.authority with
  | .intrinsic => false
  | .dynamic _ =>
      routeArmReady graph state route .left || routeArmReady graph state route .right

private def resolverReentryStateFrom?
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) :
    List RollInfo -> Option CommitState
  | [] => none
  | roll :: rest =>
      if roll.conflicts.contains route.conflict && rollComplete graph state roll then
        let reset := resetRoll state roll
        if routeResolvable graph reset route then
          some reset
        else
          resolverReentryStateFrom? graph state route rest
      else
        resolverReentryStateFrom? graph state route rest

private def resolverBaseState?
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) : Option CommitState :=
  if routeResolvable graph state route then
    some state
  else
    resolverReentryStateFrom? graph state route graph.rolls

/-- One explicit resolver evaluation either grants one arm or rejects progress. -/
inductive ResolverOutcome where
  | select (arm : RouteArm)
  | reject
  deriving Repr, DecidableEq, BEq

/-- A rejection has no choreography successor state. -/
inductive ResolverResult where
  | selected (state : CommitState)
  | rejected

def applyResolver
    (graph : EventGraph)
    (state : CommitState)
    (conflict resolver : Nat)
    (outcome : ResolverOutcome) : Option ResolverResult :=
  match graph.routeForConflict? conflict with
  | none => none
  | some route =>
      match route.authority with
      | .intrinsic => none
      | .dynamic expected =>
          if expected = resolver then
            match resolverBaseState? graph state route with
            | none => none
            | some base =>
                match outcome with
                | .select arm => some (.selected (selectRouteArm base conflict arm))
                | .reject => some .rejected
          else
            none

def enabledOperations (graph : EventGraph) (state : CommitState) : List OperationRequest :=
  graph.events.filterMap fun event =>
    if eventEnabled graph state event then some event.operationRequest else none

def matchingOperationEvent?
    (graph : EventGraph) (request : OperationRequest) : Option Event :=
  match graph.events[request.eventId]? with
  | none => none
  | some event =>
      if event.operationRequest = request then some event else none

def matchingEvent?
    (graph : EventGraph)
    (state : CommitState)
    (request : OperationRequest) : Option Event :=
  match matchingOperationEvent? graph request with
  | none => none
  | some event => if eventCommitEnabled graph state event then some event else none

def commitOperation
    (graph : EventGraph)
    (state : CommitState)
    (request : OperationRequest) : Option CommitState :=
  match matchingEvent? graph state request with
  | none => none
  | some event => commitEvent graph state event.id

private def equalContractDistinctPeerGraph : EventGraph :=
  projectGraph 0 (.par (.send 0 1 7 8) (.send 0 2 7 8))

private def equalContractFirstOperation : OperationRequest :=
  { eventId := 0, action := .send 1 7 8 }

private def equalContractSecondOperation : OperationRequest :=
  { eventId := 1, action := .send 2 7 8 }

/-- Equal logical contracts on different peers remain distinct enabled
operations. Event identity and the complete local action are both retained. -/
theorem equal_contract_operations_retain_distinct_identity :
    enabledOperations equalContractDistinctPeerGraph .initial =
      [equalContractFirstOperation, equalContractSecondOperation] := by
  decide

/-- A request cannot borrow an enabled event ID while substituting another
peer, even when label and schema are unchanged. -/
theorem equal_contract_wrong_peer_is_rejected :
    matchingEvent? equalContractDistinctPeerGraph .initial
      { eventId := 0, action := .send 2 7 8 } = none := by
  decide

inductive TraceAction where
  | commit (request : OperationRequest)
  | resolve (conflict resolver : Nat) (arm : RouteArm)
  | reject (conflict resolver : Nat)
  | stop
  deriving Repr, DecidableEq

structure TraceFrame where
  enabled : List OperationRequest
  action : TraceAction
  deriving Repr, DecidableEq

def checkTrace (graph : EventGraph) : CommitState -> List TraceFrame -> Bool
  | _, [] => true
  | state, frame :: rest =>
      if decide (enabledOperations graph state ≠ frame.enabled) then
        false
      else
        match frame.action with
        | .stop => rest.isEmpty
        | .commit key =>
            if decide (key ∉ frame.enabled) then
              false
            else
              match commitOperation graph state key with
              | none => false
              | some next => checkTrace graph next rest
        | .resolve conflict resolver arm =>
            match applyResolver graph state conflict resolver (.select arm) with
            | none => false
            | some .rejected => false
            | some (.selected next) => checkTrace graph next rest
        | .reject conflict resolver =>
            match applyResolver graph state conflict resolver .reject with
            | none => false
            | some (.selected _) => false
            | some .rejected => rest.isEmpty

def ValidTrace (graph : EventGraph) : CommitState -> List TraceFrame -> Prop
  | _, [] => True
  | state, frame :: rest =>
      enabledOperations graph state = frame.enabled /\
      match frame.action with
      | .stop => rest = []
      | .commit key =>
          key ∈ frame.enabled /\
          ∃ next, commitOperation graph state key = some next /\ ValidTrace graph next rest
      | .resolve conflict resolver arm =>
          ∃ next, applyResolver graph state conflict resolver (.select arm) =
            some (.selected next) /\ ValidTrace graph next rest
      | .reject conflict resolver =>
          applyResolver graph state conflict resolver .reject = some .rejected /\ rest = []

/-- A successful executable check witnesses every frontier and successor transition. -/
theorem trace_checker_sound
    (graph : EventGraph) (state : CommitState) (trace : List TraceFrame)
    (accepted : checkTrace graph state trace = true) :
    ValidTrace graph state trace := by
  induction trace generalizing state with
  | nil => trivial
  | cons frame rest ih =>
      simp only [checkTrace] at accepted
      by_cases frontier : enabledOperations graph state = frame.enabled
      · simp [frontier] at accepted
        cases actionCase : frame.action with
        | stop =>
            simp [actionCase] at accepted
            subst rest
            simp [ValidTrace, actionCase, frontier]
        | commit key =>
            by_cases enabled : key ∈ frame.enabled
            · simp [actionCase, enabled] at accepted
              cases nextCase : commitOperation graph state key with
              | none => simp [nextCase] at accepted
              | some next =>
                  simp [nextCase] at accepted
                  simp only [ValidTrace, actionCase]
                  exact ⟨frontier, enabled, next, nextCase, ih next accepted⟩
            · simp [actionCase, enabled] at accepted
        | resolve conflict resolver arm =>
            cases resolverCase : applyResolver graph state conflict resolver (.select arm) with
            | none => simp [actionCase, resolverCase] at accepted
            | some result =>
                cases result with
                | rejected => simp [actionCase, resolverCase] at accepted
                | selected next =>
                    simp [actionCase, resolverCase] at accepted
                    simp only [ValidTrace, actionCase]
                    exact ⟨frontier, next, resolverCase, ih next accepted⟩
        | reject conflict resolver =>
            cases resolverCase : applyResolver graph state conflict resolver .reject with
            | none => simp [actionCase, resolverCase] at accepted
            | some result =>
                cases result with
                | selected next => simp [actionCase, resolverCase] at accepted
                | rejected =>
                    simp [actionCase, resolverCase] at accepted
                    subst rest
                    simp [ValidTrace, actionCase, frontier, resolverCase]
      · simp [frontier] at accepted

/-- A committed event is marked done in the single returned successor state. -/
theorem commit_marks_event_done
    {graph : EventGraph} {state next : CommitState} {id : Nat}
    (committed : commitEvent graph state id = some next) :
    next.done id = true := by
  cases eventCase : graph.events[id]? with
  | none => simp [commitEvent, eventCase] at committed
  | some event =>
      cases baseCase : eventBaseState? graph state event with
      | none => simp [commitEvent, eventCase, baseCase] at committed
      | some base =>
        simp [commitEvent, eventCase, baseCase] at committed
        subst next
        simp

/-- A successful commit has one prepared base and one exact publish delta. -/
theorem commit_exact_successor
    {graph : EventGraph} {state next : CommitState} {id : Nat}
    (committed : commitEvent graph state id = some next) :
    ∃ event base,
      graph.events[id]? = some event /\
      eventBaseState? graph state event = some base /\
      next.done id = true /\
      (∀ candidate, candidate ≠ id -> next.done candidate = base.done candidate) /\
      next.selected = applyConflictArms event.conflicts base.selected := by
  cases eventCase : graph.events[id]? with
  | none => simp [commitEvent, eventCase] at committed
  | some event =>
      cases baseCase : eventBaseState? graph state event with
      | none => simp [commitEvent, eventCase, baseCase] at committed
      | some base =>
          simp [commitEvent, eventCase, baseCase] at committed
          subst next
          refine ⟨event, base, by simp, baseCase, ?_, ?_, rfl⟩
          · simp
          · intro candidate distinct
            simp [distinct]

/-- Resolver rejection is terminal and cannot manufacture a selected successor. -/
theorem resolver_reject_never_selects
    (graph : EventGraph) (state : CommitState) (conflict resolver : Nat)
    (next : CommitState) :
    applyResolver graph state conflict resolver .reject ≠ some (.selected next) := by
  unfold applyResolver
  cases graph.routeForConflict? conflict with
  | none => simp
  | some route =>
      cases authorityCase : route.authority with
      | intrinsic => simp [authorityCase]
      | dynamic expected =>
          by_cases sameResolver : expected = resolver
          · cases baseCase : resolverBaseState? graph state route <;>
              simp [authorityCase, sameResolver, baseCase]
          · simp [authorityCase, sameResolver]

/-- Resolver selection cannot be reinterpreted as semantic rejection. -/
theorem resolver_select_never_rejects
    (graph : EventGraph) (state : CommitState) (conflict resolver : Nat)
    (arm : RouteArm) :
    applyResolver graph state conflict resolver (.select arm) ≠ some .rejected := by
  unfold applyResolver
  cases graph.routeForConflict? conflict with
  | none => simp
  | some route =>
      cases authorityCase : route.authority with
      | intrinsic => simp [authorityCase]
      | dynamic expected =>
          by_cases sameResolver : expected = resolver
          · cases baseCase : resolverBaseState? graph state route <;>
              simp [authorityCase, sameResolver, baseCase]
          · simp [authorityCase, sameResolver]

/-- A selected resolver successor changes no completion bit from its prepared base. -/
theorem resolver_selection_exact_authority
    {graph : EventGraph} {state next : CommitState}
    {conflict resolver : Nat} {arm : RouteArm}
    (resolved : applyResolver graph state conflict resolver (.select arm) =
      some (.selected next)) :
    ∃ route base,
      graph.routeForConflict? conflict = some route /\
      route.authority = .dynamic resolver /\
      resolverBaseState? graph state route = some base /\
      next.done = base.done /\
      next.selected conflict = some arm /\
      ∀ other, other ≠ conflict -> next.selected other = base.selected other := by
  unfold applyResolver at resolved
  cases routeCase : graph.routeForConflict? conflict with
  | none => simp [routeCase] at resolved
  | some route =>
      cases authorityCase : route.authority with
      | intrinsic => simp [routeCase, authorityCase] at resolved
      | dynamic expected =>
          by_cases sameResolver : expected = resolver
          · cases baseCase : resolverBaseState? graph state route with
            | none => simp [routeCase, authorityCase, sameResolver, baseCase] at resolved
            | some base =>
                simp [routeCase, authorityCase, sameResolver, baseCase] at resolved
                subst next
                subst expected
                refine ⟨route, base, by simp, authorityCase,
                  baseCase, rfl, ?_, ?_⟩
                · simp [selectRouteArm]
                · intro other distinct
                  simp [selectRouteArm, distinct]
          · simp [routeCase, authorityCase, sameResolver] at resolved

/-- Once an arm is attached, that route cannot request another decision without reset. -/
theorem selected_route_is_not_resolvable
    (graph : EventGraph) (state : CommitState) (route : RouteInfo) (arm : RouteArm) :
    routeResolvable graph (selectRouteArm state route.conflict arm) route = false := by
  simp [routeResolvable, selectRouteArm]

/-- A dynamic route candidate is visible, but cannot commit before resolver authority exists. -/
theorem unresolved_dynamic_route_cannot_commit
    {graph : EventGraph} {state : CommitState} {event : Event}
    {membership : ConflictArm} {route : RouteInfo} {resolver : Nat}
    (inside : membership ∈ event.conflicts)
    (routeFound : graph.routeForConflict? membership.conflict = some route)
    (dynamic : route.authority = .dynamic resolver)
    (unselected : state.selected membership.conflict = none) :
    eventCommitReady graph state event = false := by
  cases allCase : event.conflicts.all (commitAuthorityAllows graph state) with
  | false => simp [eventCommitReady, allCase]
  | true =>
      have allowed := (List.all_eq_true.mp allCase) membership inside
      simp [commitAuthorityAllows, routeFound, dynamic, unselected] at allowed

/-- One conflict has at most one selected arm in a commit state. -/
theorem selected_arm_unique
    (state : CommitState) (conflict : Nat) (chosenA chosenB : RouteArm)
    (selectedA : state.selected conflict = some chosenA)
    (selectedB : state.selected conflict = some chosenB) : chosenA = chosenB := by
  rw [selectedA] at selectedB
  exact Option.some.inj selectedB

/-- Reentry reset cannot change completion outside its roll scope. -/
theorem reset_roll_preserves_outside_done
    (state : CommitState) (roll : RollInfo) (event : Nat)
    (outside : event ∉ roll.events) :
    (resetRoll state roll).done event = state.done event := by
  simp [resetRoll, outside]

/-- Roll reentry clears completion for every event owned by the roll. -/
theorem reset_roll_clears_done
    (state : CommitState) (roll : RollInfo) (event : Nat)
    (inside : event ∈ roll.events) :
    (resetRoll state roll).done event = false := by
  simp [resetRoll, inside]

/-- Reentry reset cannot change route authority outside its roll scope. -/
theorem reset_roll_preserves_outside_selection
    (state : CommitState) (roll : RollInfo) (conflict : Nat)
    (outside : conflict ∉ roll.conflicts) :
    (resetRoll state roll).selected conflict = state.selected conflict := by
  simp [resetRoll, outside]

/-- Roll reentry clears every route decision owned by the roll. -/
theorem reset_roll_clears_selection
    (state : CommitState) (roll : RollInfo) (conflict : Nat)
    (inside : conflict ∈ roll.conflicts) :
    (resetRoll state roll).selected conflict = none := by
  simp [resetRoll, inside]

/-- Route-arm reentry cannot change any route authority. -/
theorem reset_route_arm_preserves_selection
    (state : CommitState) (events : List Nat) (conflict : Nat) :
    (resetRouteArm state events).selected conflict = state.selected conflict := by
  rfl

/-- Route-arm reentry cannot change completion outside the selected arm. -/
theorem reset_route_arm_preserves_outside_done
    (state : CommitState) (events : List Nat) (event : Nat)
    (outside : event ∉ events) :
    (resetRouteArm state events).done event = state.done event := by
  simp [resetRouteArm, outside]

/-- Route-arm reentry clears completion for every event in that arm. -/
theorem reset_route_arm_clears_done
    (state : CommitState) (events : List Nat) (event : Nat)
    (inside : event ∈ events) :
    (resetRouteArm state events).done event = false := by
  simp [resetRouteArm, inside]

def checkProgramTrace (choreo : Choreo) (role : Nat) (trace : List TraceFrame) : Bool :=
  let graph := projectGraph role choreo
  graph.check && checkTrace graph .initial trace

theorem program_trace_checker_sound
    (choreo : Choreo) (role : Nat) (trace : List TraceFrame)
    (accepted : checkProgramTrace choreo role trace = true) :
    (projectGraph role choreo).WellFormed /\
      ValidTrace (projectGraph role choreo) .initial trace := by
  simp [checkProgramTrace] at accepted
  exact ⟨artifact_checker_sound accepted.1,
    trace_checker_sound _ _ _ accepted.2⟩

end Hibana
