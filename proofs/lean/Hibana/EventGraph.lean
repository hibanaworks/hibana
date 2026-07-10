import Hibana.Syntax

namespace Hibana

structure ConflictArm where
  conflict : Nat
  arm : RouteArm
  deriving Repr, DecidableEq, BEq

structure Event where
  id : Nat
  lane : Nat
  action : LocalAction
  deps : List Nat
  conflicts : List ConflictArm
  deriving Repr, DecidableEq, BEq

structure RollInfo where
  events : List Nat
  entries : List Nat
  conflicts : List Nat
  deriving Repr, DecidableEq, BEq

structure RouteInfo where
  conflict : Nat
  authority : RouteAuthority
  leftEvents : List Nat
  rightEvents : List Nat
  reentry : ReentryMode
  deriving Repr, DecidableEq, BEq

structure EventGraph where
  events : List Event
  laneCount : Nat
  conflictCount : Nat
  rolls : List RollInfo
  routes : List RouteInfo
  deriving Repr, DecidableEq, BEq

structure ProjectionContext where
  incoming : List Nat
  lane : Nat
  conflicts : List ConflictArm
  reentry : ReentryMode

structure ProjectionBuilder where
  events : List Event
  nextLane : Nat
  nextConflict : Nat
  rolls : List RollInfo
  routes : List RouteInfo

structure ProjectionResult where
  builder : ProjectionBuilder
  exits : List Nat

def ProjectionBuilder.empty : ProjectionBuilder := {
  events := []
  nextLane := 1
  nextConflict := 0
  rolls := []
  routes := []
}

def ProjectionContext.root : ProjectionContext := {
  incoming := []
  lane := 0
  conflicts := []
  reentry := .singlePass
}

private def uniqueNats (values : List Nat) : List Nat := values.eraseDups

private def ProjectionBuilder.addEvent
    (builder : ProjectionBuilder)
    (context : ProjectionContext)
    (action : LocalAction) : ProjectionResult :=
  let id := builder.events.length
  let event : Event := {
    id
    lane := context.lane
    action
    deps := uniqueNats context.incoming
    conflicts := context.conflicts
  }
  {
    builder := { builder with events := builder.events ++ [event] }
    exits := [id]
  }

/-- Project one normalized choreography into a finite role-local event graph. -/
def projectInto
    (role : Nat)
    (choreo : Choreo)
    (builder : ProjectionBuilder)
    (context : ProjectionContext) : ProjectionResult :=
  match choreo with
  | .send sender receiver label =>
      match Choreo.localAction? role sender receiver label with
      | some action => builder.addEvent context action
      | none => { builder, exits := context.incoming }
  | .seq left right =>
      let leftResult := projectInto role left builder context
      projectInto role right leftResult.builder {
        context with incoming := leftResult.exits
      }
  | .par left right =>
      let leftLane := builder.nextLane
      let rightLane := builder.nextLane + 1
      let forked := { builder with nextLane := builder.nextLane + 2 }
      let leftResult := projectInto role left forked {
        context with
        lane := leftLane
      }
      let rightResult := projectInto role right leftResult.builder {
        context with
        lane := rightLane
      }
      {
        builder := rightResult.builder
        exits := uniqueNats (leftResult.exits ++ rightResult.exits)
      }
  | .route authority left right =>
      let conflict := builder.nextConflict
      let leftLane := builder.nextLane
      let rightLane := builder.nextLane + 1
      let forked := {
        builder with
        nextLane := builder.nextLane + 2
        nextConflict := builder.nextConflict + 1
      }
      let leftStart := forked.events.length
      let leftResult := projectInto role left forked {
        context with
        lane := leftLane
        conflicts := context.conflicts ++ [{ conflict, arm := .left }]
      }
      let leftCount := leftResult.builder.events.length - leftStart
      let leftEvents := (List.range leftCount).map (leftStart + ·)
      let rightStart := leftResult.builder.events.length
      let rightResult := projectInto role right leftResult.builder {
        context with
        lane := rightLane
        conflicts := context.conflicts ++ [{ conflict, arm := .right }]
      }
      let rightCount := rightResult.builder.events.length - rightStart
      let rightEvents := (List.range rightCount).map (rightStart + ·)
      let route : RouteInfo := {
        conflict
        authority
        leftEvents
        rightEvents
        reentry := context.reentry
      }
      {
        builder := {
          rightResult.builder with routes := rightResult.builder.routes ++ [route]
        }
        exits := uniqueNats (leftResult.exits ++ rightResult.exits)
      }
  | .roll body =>
      let eventStart := builder.events.length
      let conflictStart := builder.nextConflict
      let bodyResult := projectInto role body builder {
        context with reentry := .rolled
      }
      let eventCount := bodyResult.builder.events.length - eventStart
      let eventIds := (List.range eventCount).map (eventStart + ·)
      let entries := eventIds.filter fun id =>
        match bodyResult.builder.events[id]? with
        | none => false
        | some event => event.deps.all fun dep => !eventIds.contains dep
      let conflictCount := bodyResult.builder.nextConflict - conflictStart
      let conflicts := (List.range conflictCount).map (conflictStart + ·)
      let roll : RollInfo := {
        events := eventIds
        entries
        conflicts
      }
      {
        builder := {
          bodyResult.builder with rolls := bodyResult.builder.rolls ++ [roll]
        }
        exits := bodyResult.exits
      }
def projectGraph (role : Nat) (choreo : Choreo) : EventGraph :=
  let result := projectInto role choreo .empty .root
  {
    events := result.builder.events
    laneCount := result.builder.nextLane
    conflictCount := result.builder.nextConflict
    rolls := result.builder.rolls
    routes := result.builder.routes
  }

private def uniqueKeys (keys : List Nat) : Bool := decide keys.Nodup

private def Event.checkAt (graph : EventGraph) (index : Nat) (event : Event) : Bool :=
  event.id == index &&
  event.lane < graph.laneCount &&
  event.deps.all (fun dep => dep < event.id) &&
  event.conflicts.all (fun membership => membership.conflict < graph.conflictCount) &&
  uniqueKeys (event.conflicts.map ConflictArm.conflict)

private def checkEvents (graph : EventGraph) : Nat -> List Event -> Bool
  | _, [] => true
  | index, event :: rest => event.checkAt graph index && checkEvents graph (index + 1) rest

private def RollInfo.check (graph : EventGraph) (roll : RollInfo) : Bool :=
  roll.events.all (fun event => event < graph.events.length) &&
  roll.entries.all (fun entry => roll.events.contains entry) &&
  roll.conflicts.all (fun conflict => conflict < graph.conflictCount) &&
  uniqueKeys roll.events &&
  uniqueKeys roll.entries &&
  uniqueKeys roll.conflicts

private def checkRolls (graph : EventGraph) : List RollInfo -> Bool
  | [] => true
  | roll :: rest => roll.check graph && checkRolls graph rest

private def Event.hasConflictArm
    (graph : EventGraph) (id conflict : Nat) (arm : RouteArm) : Bool :=
  match graph.events[id]? with
  | none => false
  | some event => event.conflicts.contains { conflict, arm }

private def RouteAuthority.check : RouteAuthority -> Bool
  | .intrinsic => true
  | .dynamic resolver => resolver < 65535

private def RouteInfo.check (graph : EventGraph) (route : RouteInfo) : Bool :=
  route.conflict < graph.conflictCount &&
  route.authority.check &&
  route.leftEvents.all (fun event => event < graph.events.length) &&
  route.rightEvents.all (fun event => event < graph.events.length) &&
  route.leftEvents.all (fun event => Event.hasConflictArm graph event route.conflict .left) &&
  route.rightEvents.all (fun event => Event.hasConflictArm graph event route.conflict .right) &&
  uniqueKeys (route.leftEvents ++ route.rightEvents)

private def checkRoutes (graph : EventGraph) : List RouteInfo -> Bool
  | [] => true
  | route :: rest => route.check graph && checkRoutes graph rest

def Event.WellFormedAt (graph : EventGraph) (index : Nat) (event : Event) : Prop :=
  event.id = index /\
  event.lane < graph.laneCount /\
  (forall dep, dep ∈ event.deps -> dep < event.id) /\
  (forall membership, membership ∈ event.conflicts ->
    membership.conflict < graph.conflictCount) /\
  (event.conflicts.map ConflictArm.conflict).Nodup

def EventsWellFormed (graph : EventGraph) : Nat -> List Event -> Prop
  | _, [] => True
  | index, event :: rest =>
      event.WellFormedAt graph index /\ EventsWellFormed graph (index + 1) rest

def RollInfo.WellFormed (graph : EventGraph) (roll : RollInfo) : Prop :=
  (forall event, event ∈ roll.events -> event < graph.events.length) /\
  (forall entry, entry ∈ roll.entries -> entry ∈ roll.events) /\
  (forall conflict, conflict ∈ roll.conflicts -> conflict < graph.conflictCount) /\
  roll.events.Nodup /\ roll.entries.Nodup /\ roll.conflicts.Nodup

def RollsWellFormed (graph : EventGraph) : List RollInfo -> Prop
  | [] => True
  | roll :: rest => roll.WellFormed graph /\ RollsWellFormed graph rest

def RouteInfo.WellFormed (graph : EventGraph) (route : RouteInfo) : Prop :=
  route.conflict < graph.conflictCount /\
  route.authority.check = true /\
  (forall event, event ∈ route.leftEvents -> event < graph.events.length) /\
  (forall event, event ∈ route.rightEvents -> event < graph.events.length) /\
  (forall event, event ∈ route.leftEvents ->
    Event.hasConflictArm graph event route.conflict .left = true) /\
  (forall event, event ∈ route.rightEvents ->
    Event.hasConflictArm graph event route.conflict .right = true) /\
  (route.leftEvents ++ route.rightEvents).Nodup

def RoutesWellFormed (graph : EventGraph) : List RouteInfo -> Prop
  | [] => True
  | route :: rest => route.WellFormed graph /\ RoutesWellFormed graph rest

def EventGraph.WellFormed (graph : EventGraph) : Prop :=
  0 < graph.laneCount /\
  EventsWellFormed graph 0 graph.events /\
  RollsWellFormed graph graph.rolls /\
  RoutesWellFormed graph graph.routes /\
  graph.routes.length = graph.conflictCount /\
  (graph.routes.map RouteInfo.conflict).Nodup

def EventGraph.check (graph : EventGraph) : Bool :=
  0 < graph.laneCount &&
  checkEvents graph 0 graph.events &&
  checkRolls graph graph.rolls &&
  checkRoutes graph graph.routes &&
  graph.routes.length == graph.conflictCount &&
  uniqueKeys (graph.routes.map RouteInfo.conflict)

private theorem event_check_sound
    {graph : EventGraph} {index : Nat} {event : Event}
    (checked : event.checkAt graph index = true) :
    event.WellFormedAt graph index := by
  simp [Event.checkAt, uniqueKeys] at checked
  rcases checked with ⟨⟨⟨⟨id, lane⟩, deps⟩, conflicts⟩, unique⟩
  exact ⟨id, lane, deps, conflicts, unique⟩

private theorem events_check_sound
    (graph : EventGraph) (index : Nat) (events : List Event)
    (checked : checkEvents graph index events = true) :
    EventsWellFormed graph index events := by
  induction events generalizing index with
  | nil => trivial
  | cons event rest ih =>
      simp [checkEvents] at checked
      exact ⟨event_check_sound checked.1, ih (index := index + 1) checked.2⟩

private theorem roll_check_sound
    {graph : EventGraph} {roll : RollInfo}
    (checked : roll.check graph = true) :
    roll.WellFormed graph := by
  simp [RollInfo.check, uniqueKeys] at checked
  rcases checked with ⟨⟨⟨⟨⟨events, entries⟩, conflicts⟩, eventUnique⟩,
    entryUnique⟩, conflictUnique⟩
  exact ⟨events, entries, conflicts, eventUnique, entryUnique, conflictUnique⟩

private theorem rolls_check_sound
    (graph : EventGraph) (rolls : List RollInfo)
    (checked : checkRolls graph rolls = true) :
    RollsWellFormed graph rolls := by
  induction rolls with
  | nil => trivial
  | cons roll rest ih =>
      simp [checkRolls] at checked
      exact ⟨roll_check_sound checked.1, ih checked.2⟩

private theorem route_check_sound
    {graph : EventGraph} {route : RouteInfo}
    (checked : route.check graph = true) :
    route.WellFormed graph := by
  simp [RouteInfo.check, uniqueKeys] at checked
  rcases checked with
    ⟨⟨⟨⟨⟨⟨conflict, authority⟩, leftBounds⟩, rightBounds⟩, leftMembership⟩,
      rightMembership⟩, unique⟩
  exact ⟨conflict, authority, leftBounds, rightBounds, leftMembership,
    rightMembership, unique⟩

private theorem routes_check_sound
    (graph : EventGraph) (routes : List RouteInfo)
    (checked : checkRoutes graph routes = true) :
    RoutesWellFormed graph routes := by
  induction routes with
  | nil => trivial
  | cons route rest ih =>
      simp [checkRoutes] at checked
      exact ⟨route_check_sound checked.1, ih checked.2⟩

/-- Acceptance by the executable graph checker implies the structural invariant. -/
theorem artifact_checker_sound
    {graph : EventGraph} (accepted : graph.check = true) : graph.WellFormed := by
  simp [EventGraph.check, uniqueKeys] at accepted
  rcases accepted with
    ⟨⟨⟨⟨⟨lanes, events⟩, rolls⟩, routes⟩, routeCount⟩, routeUnique⟩
  exact ⟨lanes, events_check_sound graph 0 graph.events events,
    rolls_check_sound graph graph.rolls rolls,
    routes_check_sound graph graph.routes routes, routeCount, routeUnique⟩

end Hibana
