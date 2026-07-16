import Hibana.Syntax

namespace Hibana

/-- One globally unique communication occurrence. Its index in
`Choreo.globalEvents` is the protocol event identity. -/
structure GlobalEvent where
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  lane : Nat
  deriving Repr, DecidableEq

/-- One global route-membership fact. Conflict ordinals are assigned by the
canonical choreography preorder and are shared by every role projection. -/
structure ConflictArm where
  conflict : Nat
  arm : RouteArm
  deriving Repr, DecidableEq, BEq

/-- One canonical parallel-arm membership. Unlike route conflicts, both arms
execute, but local order never crosses directly between them. -/
structure ParallelArm where
  parallel : Nat
  arm : RouteArm
  deriving Repr, DecidableEq, BEq

def GlobalEvent.action? (event : GlobalEvent) (role : Nat) : Option LocalAction :=
  Choreo.localAction? role event.sender event.receiver event.label event.schema

structure CompiledOccurrence where
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  lane : Nat
  frameLabel : Nat
  deriving Repr, DecidableEq

structure CompiledOccurrenceSource where
  occurrences : List CompiledOccurrence
  laneSpan : Nat
  deriving Repr, DecidableEq

def CompiledOccurrence.globalEventFrom
    (occurrence : CompiledOccurrence) (laneBase : Nat) : GlobalEvent := {
  sender := occurrence.sender
  receiver := occurrence.receiver
  label := occurrence.label
  schema := occurrence.schema
  lane := laneBase + occurrence.lane
}

def CompiledOccurrence.action?
    (occurrence : CompiledOccurrence) (role : Nat) : Option LocalAction :=
  Choreo.localAction? role occurrence.sender occurrence.receiver
    occurrence.label occurrence.schema

def compiledLaneSpan : List CompiledOccurrence -> Nat
  | [] => 0
  | occurrence :: rest => max (occurrence.lane + 1) (compiledLaneSpan rest)

def compiledSourceFrom
    (occurrences : List CompiledOccurrence) : CompiledOccurrenceSource := {
  occurrences
  laneSpan := compiledLaneSpan occurrences
}

def occurrencesShareEndpoint
    (left right : CompiledOccurrence) : Bool :=
  left.sender == right.sender || left.sender == right.receiver ||
    left.receiver == right.sender || left.receiver == right.receiver

def laneClassesShareEndpoint
    (left right : List CompiledOccurrence)
    (leftLane rightLane : Nat) : Bool :=
  left.any fun leftOccurrence =>
    leftOccurrence.lane == leftLane && right.any fun rightOccurrence =>
      rightOccurrence.lane == rightLane &&
        occurrencesShareEndpoint leftOccurrence rightOccurrence

structure LaneRemap where
  oldLane : Nat
  newLane : Nat
  deriving Repr, DecidableEq

def LaneRemap.forRight? (remap : List LaneRemap) (rightLane : Nat) : Option Nat :=
  (remap.find? fun entry => entry.oldLane == rightLane).map LaneRemap.newLane

def LaneRemap.forLeft? (remap : List LaneRemap) (leftLane : Nat) : Option Nat :=
  (remap.find? fun entry => entry.newLane == leftLane).map LaneRemap.oldLane

def LaneRemap.replace
    (remap : List LaneRemap) (rightLane leftLane : Nat) : List LaneRemap :=
  (remap.filter fun entry =>
    entry.oldLane != rightLane && entry.newLane != leftLane) ++
    [{ oldLane := rightLane, newLane := leftLane }]

structure LaneAugmentState where
  remap? : Option (List LaneRemap)
  seenLeft : List Nat

/-- Deterministic augmenting-path search used by the production lane allocator.
`fuel` bounds the alternating path by the finite left-lane domain. Failed
branches retain their visited set exactly as the fixed Rust scratch bitmap does. -/
def augmentParallelLane
    (left right : List CompiledOccurrence) (leftSpan rightLane : Nat)
    (remap : List LaneRemap) (seenLeft : List Nat) : Nat -> LaneAugmentState
  | 0 => { remap? := none, seenLeft }
  | fuel + 1 =>
      (List.range leftSpan).foldl (fun state leftLane =>
        match state.remap? with
        | some _ => state
        | none =>
            if state.seenLeft.contains leftLane ||
                laneClassesShareEndpoint left right leftLane rightLane then
              state
            else
              let nextSeen := leftLane :: state.seenLeft
              match LaneRemap.forLeft? remap leftLane with
              | none => {
                  remap? := some (LaneRemap.replace remap rightLane leftLane)
                  seenLeft := nextSeen
                }
              | some occupant =>
                  let recursive := augmentParallelLane left right leftSpan occupant remap
                    nextSeen fuel
                  match recursive.remap? with
                  | none => recursive
                  | some reassigned => {
                      remap? := some (LaneRemap.replace reassigned rightLane leftLane)
                      seenLeft := recursive.seenLeft
                    }) { remap? := none, seenLeft }

def maximumParallelLaneReuse
    (left right : List CompiledOccurrence)
    (leftSpan rightSpan : Nat) : List LaneRemap :=
  (List.range rightSpan).foldl (fun remap rightLane =>
    (augmentParallelLane left right leftSpan rightLane remap []
      (leftSpan + 1)).remap?.getD remap) []

def parallelLaneRemap
    (left right : List CompiledOccurrence)
    (leftSpan rightSpan : Nat) : List LaneRemap :=
  let reused := maximumParallelLaneReuse left right leftSpan rightSpan
  ((List.range rightSpan).foldl (fun state rightLane =>
    match LaneRemap.forRight? reused rightLane with
    | some _ => state
    | none =>
        (state.1 ++ [({ oldLane := rightLane, newLane := state.2 } : LaneRemap)], state.2 + 1))
    (reused, leftSpan)).1

private def constrainedLaneLeft : List CompiledOccurrence := [
  { sender := 0, receiver := 1, label := 0, schema := 0, lane := 0, frameLabel := 0 },
  { sender := 2, receiver := 3, label := 0, schema := 0, lane := 1, frameLabel := 0 }
]

private def constrainedLaneRight : List CompiledOccurrence := [
  { sender := 4, receiver := 5, label := 0, schema := 0, lane := 0, frameLabel := 0 },
  { sender := 2, receiver := 6, label := 0, schema := 0, lane := 1, frameLabel := 0 }
]

/-- The augmenting path moves the unconstrained class instead of allocating an
unnecessary third lane. This is the minimal counterexample to first-fit. -/
theorem constrained_parallel_lane_matching_uses_minimum_span :
    parallelLaneRemap constrainedLaneLeft constrainedLaneRight 2 2 =
      [{ oldLane := 0, newLane := 1 }, { oldLane := 1, newLane := 0 }] := by
  decide

def remapParallelLane (remap : List LaneRemap) (lane : Nat) : Nat :=
  match remap.find? fun entry => entry.oldLane == lane with
  | some entry => entry.newLane
  | none => 256

def mergeParallelOccurrences
    (left right : CompiledOccurrenceSource) : CompiledOccurrenceSource :=
  let remap := parallelLaneRemap
    left.occurrences right.occurrences left.laneSpan right.laneSpan
  let recoloredRight := right.occurrences.map fun occurrence =>
    { occurrence with lane := remapParallelLane remap occurrence.lane }
  compiledSourceFrom (left.occurrences ++ recoloredRight)

structure FrameLabelRemap where
  sender : Nat
  receiver : Nat
  lane : Nat
  oldLabel : Nat
  newLabel : Nat
  deriving Repr, DecidableEq

def firstAvailableFrameLabel (used : List Nat) : Option Nat :=
  (List.range 256).find? fun candidate => !used.contains candidate

def FrameLabelRemap.matches
    (entry : FrameLabelRemap) (occurrence : CompiledOccurrence) : Bool :=
  entry.sender == occurrence.sender && entry.receiver == occurrence.receiver &&
    entry.lane == occurrence.lane && entry.oldLabel == occurrence.frameLabel

def routeFrameLabelRemap
    (left right : List CompiledOccurrence) : List FrameLabelRemap :=
  right.foldl (fun remap occurrence =>
    if occurrence.sender == occurrence.receiver then
      remap
    else if remap.any (·.matches occurrence) then
      remap
    else
      let usedLeft := (left.filter fun candidate =>
        candidate.sender == occurrence.sender &&
          candidate.receiver == occurrence.receiver &&
          candidate.lane == occurrence.lane).map CompiledOccurrence.frameLabel
      let usedRight := (remap.filter fun entry =>
        entry.sender == occurrence.sender &&
          entry.receiver == occurrence.receiver &&
          entry.lane == occurrence.lane).map FrameLabelRemap.newLabel
      let selected := (firstAvailableFrameLabel (usedLeft ++ usedRight)).getD 256
      remap ++ [{
        sender := occurrence.sender
        receiver := occurrence.receiver
        lane := occurrence.lane
        oldLabel := occurrence.frameLabel
        newLabel := selected
      }]) []

def remapRouteFrameLabel
    (remap : List FrameLabelRemap) (occurrence : CompiledOccurrence) : Nat :=
  if occurrence.sender == occurrence.receiver then
    occurrence.frameLabel
  else
    match remap.find? (·.matches occurrence) with
    | some entry => entry.newLabel
    | none => 256

@[simp]
theorem remap_route_frame_label_preserves_local_effect
    (remap : List FrameLabelRemap) (occurrence : CompiledOccurrence)
    (localEffect : occurrence.sender = occurrence.receiver) :
    remapRouteFrameLabel remap occurrence = occurrence.frameLabel := by
  have sameRole : (occurrence.sender == occurrence.receiver) = true :=
    beq_iff_eq.mpr localEffect
  unfold remapRouteFrameLabel
  rw [sameRole]
  rfl

def mergeRouteOccurrences
    (left right : CompiledOccurrenceSource) : CompiledOccurrenceSource :=
  let remap := routeFrameLabelRemap left.occurrences right.occurrences
  let recoloredRight := right.occurrences.map fun occurrence =>
    { occurrence with frameLabel := remapRouteFrameLabel remap occurrence }
  compiledSourceFrom (left.occurrences ++ recoloredRight)

structure RollFrameLabelClass where
  sender : Nat
  receiver : Nat
  lane : Nat
  conflicts : List ConflictArm
  frameLabel : Nat
  deriving Repr, DecidableEq

def RollFrameLabelClass.matches
    (entry : RollFrameLabelClass) (occurrence : CompiledOccurrence)
    (conflicts : List ConflictArm) : Bool :=
  entry.sender == occurrence.sender && entry.receiver == occurrence.receiver &&
    entry.lane == occurrence.lane && entry.conflicts == conflicts

def RollFrameLabelClass.sameInboundKey
    (entry : RollFrameLabelClass) (occurrence : CompiledOccurrence) : Bool :=
  entry.sender == occurrence.sender && entry.receiver == occurrence.receiver &&
    entry.lane == occurrence.lane

/-- Roll compilation retains one frame color for each exact route path of an
inbound operation key. Equal paths preserve ordered reuse; distinct paths are
the elastic reentry frontier and therefore receive distinct colors. -/
def recolorRollOccurrencesFrom
    (conflictRows : List (List ConflictArm)) :
    List RollFrameLabelClass -> Nat -> List CompiledOccurrence ->
      List CompiledOccurrence
  | _, _, [] => []
  | classes, index, occurrence :: rest =>
      let conflicts := (conflictRows[index]?).getD []
      if occurrence.sender == occurrence.receiver then
        occurrence :: recolorRollOccurrencesFrom conflictRows classes (index + 1) rest
      else
        match classes.find? (fun entry => entry.matches occurrence conflicts) with
        | some entry =>
            { occurrence with frameLabel := entry.frameLabel } ::
              recolorRollOccurrencesFrom conflictRows classes (index + 1) rest
        | none =>
            let used := (classes.filter fun entry =>
              entry.sameInboundKey occurrence).map RollFrameLabelClass.frameLabel
            let selected := (firstAvailableFrameLabel used).getD 256
            let nextClass : RollFrameLabelClass := {
              sender := occurrence.sender
              receiver := occurrence.receiver
              lane := occurrence.lane
              conflicts
              frameLabel := selected
            }
            { occurrence with frameLabel := selected } ::
              recolorRollOccurrencesFrom conflictRows
                (classes ++ [nextClass]) (index + 1) rest

def recolorRollOccurrences
    (occurrences : List CompiledOccurrence)
    (conflictRows : List (List ConflictArm)) : List CompiledOccurrence :=
  recolorRollOccurrencesFrom conflictRows [] 0 occurrences

@[simp]
theorem recolor_roll_local_effect_head_is_unchanged
    (conflictRows : List (List ConflictArm))
    (classes : List RollFrameLabelClass) (index : Nat)
    (occurrence : CompiledOccurrence) (rest : List CompiledOccurrence)
    (localEffect : occurrence.sender = occurrence.receiver) :
    recolorRollOccurrencesFrom conflictRows classes index (occurrence :: rest) =
      occurrence :: recolorRollOccurrencesFrom conflictRows classes (index + 1) rest := by
  have sameRole : (occurrence.sender == occurrence.receiver) = true :=
    beq_iff_eq.mpr localEffect
  cases rest <;> simp only [recolorRollOccurrencesFrom, sameRole, if_true]

def recolorRollSource
    (source : CompiledOccurrenceSource)
    (conflictRows : List (List ConflictArm)) : CompiledOccurrenceSource :=
  compiledSourceFrom (recolorRollOccurrences source.occurrences conflictRows)

theorem filter_map_recolored_lanes
    (role : Nat) (occurrences : List CompiledOccurrence) (recolor : Nat -> Nat) :
    (occurrences.map fun occurrence =>
        { occurrence with lane := recolor occurrence.lane }).filterMap (·.action? role) =
      occurrences.filterMap (·.action? role) := by
  induction occurrences with
  | nil => rfl
  | cons head tail tailIH =>
      have headAction :
          ({ head with lane := recolor head.lane } : CompiledOccurrence).action? role =
            head.action? role := rfl
      simp only [List.map_cons, List.filterMap_cons, headAction, tailIH]

theorem filter_map_recolored_frame_labels
    (role : Nat) (occurrences : List CompiledOccurrence)
    (recolor : CompiledOccurrence -> Nat) :
    (occurrences.map fun occurrence =>
        { occurrence with frameLabel := recolor occurrence }).filterMap (·.action? role) =
      occurrences.filterMap (·.action? role) := by
  induction occurrences with
  | nil => rfl
  | cons head tail tailIH =>
      have headAction :
          ({ head with frameLabel := recolor head } : CompiledOccurrence).action? role =
            head.action? role := rfl
      simp only [List.map_cons, List.filterMap_cons, headAction, tailIH]

@[simp]
theorem merge_parallel_occurrence_actions
    (role : Nat) (left right : CompiledOccurrenceSource) :
    (mergeParallelOccurrences left right).occurrences.filterMap (·.action? role) =
      left.occurrences.filterMap (·.action? role) ++
        right.occurrences.filterMap (·.action? role) := by
  simp only [mergeParallelOccurrences, compiledSourceFrom, List.filterMap_append]
  rw [filter_map_recolored_lanes]

@[simp]
theorem merge_route_occurrence_actions
    (role : Nat) (left right : CompiledOccurrenceSource) :
    (mergeRouteOccurrences left right).occurrences.filterMap (·.action? role) =
      left.occurrences.filterMap (·.action? role) ++
        right.occurrences.filterMap (·.action? role) := by
  simp only [mergeRouteOccurrences, compiledSourceFrom, List.filterMap_append]
  rw [filter_map_recolored_frame_labels]

@[simp]
theorem recolor_roll_occurrences_from_length
    (conflictRows : List (List ConflictArm))
    (classes : List RollFrameLabelClass) (index : Nat)
    (occurrences : List CompiledOccurrence) :
    (recolorRollOccurrencesFrom conflictRows classes index occurrences).length =
      occurrences.length := by
  induction occurrences generalizing classes index with
  | nil => rfl
  | cons head tail tailIH =>
      simp only [recolorRollOccurrencesFrom]
      split
      · simp [tailIH]
      · split <;> simp [tailIH]

@[simp]
theorem compiled_occurrence_action_update_frame_label
    (role frameLabel : Nat) (occurrence : CompiledOccurrence) :
    ({ occurrence with frameLabel } : CompiledOccurrence).action? role =
      occurrence.action? role := rfl

@[simp]
theorem recolor_roll_occurrences_from_actions
    (role : Nat) (conflictRows : List (List ConflictArm))
    (classes : List RollFrameLabelClass) (index : Nat)
    (occurrences : List CompiledOccurrence) :
    (recolorRollOccurrencesFrom conflictRows classes index occurrences).filterMap
        (·.action? role) = occurrences.filterMap (·.action? role) := by
  induction occurrences generalizing classes index with
  | nil => rfl
  | cons head tail tailIH =>
      simp only [recolorRollOccurrencesFrom]
      split
      · simp only [List.filterMap_cons]
        rw [tailIH]
      · split <;>
          simp only [List.filterMap_cons, compiled_occurrence_action_update_frame_label] <;>
          rw [tailIH]

@[simp]
theorem recolor_roll_source_actions
    (role : Nat) (source : CompiledOccurrenceSource)
    (conflictRows : List (List ConflictArm)) :
    (recolorRollSource source conflictRows).occurrences.filterMap (·.action? role) =
      source.occurrences.filterMap (·.action? role) := by
  simp [recolorRollSource, recolorRollOccurrences, compiledSourceFrom]

def Choreo.routeCount : Choreo -> Nat
  | .send _ _ _ _ => 0
  | .seq left right | .par left right => left.routeCount + right.routeCount
  | .route _ left right => 1 + left.routeCount + right.routeCount
  | .roll body => body.routeCount

/-- Route memberships aligned one-for-one with the canonical event preorder. -/
def Choreo.globalEventConflictsFrom (conflictBase : Nat) : Choreo -> List (List ConflictArm)
  | .send _ _ _ _ => [[]]
  | .seq left right | .par left right =>
      left.globalEventConflictsFrom conflictBase ++
        right.globalEventConflictsFrom (conflictBase + left.routeCount)
  | .route _ left right =>
      (left.globalEventConflictsFrom (conflictBase + 1)).map
          ({ conflict := conflictBase, arm := .left } :: ·) ++
        (right.globalEventConflictsFrom
          (conflictBase + 1 + left.routeCount)).map
          ({ conflict := conflictBase, arm := .right } :: ·)
  | .roll body => body.globalEventConflictsFrom conflictBase

def Choreo.globalEventConflicts (choreo : Choreo) : List (List ConflictArm) :=
  choreo.globalEventConflictsFrom 0

/-- Host-only production allocation. Event order is the choreography preorder;
lane and frame-label colors are erased into the compact descriptor rows. -/
def Choreo.compiledOccurrences : Choreo -> CompiledOccurrenceSource
  | .send sender receiver label schema => compiledSourceFrom [{
      sender
      receiver
      label
      schema
      lane := 0
      frameLabel := 0
    }]
  | .seq left right => compiledSourceFrom
      (left.compiledOccurrences.occurrences ++ right.compiledOccurrences.occurrences)
  | .par left right => mergeParallelOccurrences
      left.compiledOccurrences right.compiledOccurrences
  | .route _ left right => mergeRouteOccurrences
      left.compiledOccurrences right.compiledOccurrences
  | .roll body => recolorRollSource body.compiledOccurrences body.globalEventConflicts

def Choreo.laneSpan (choreo : Choreo) : Nat :=
  choreo.compiledOccurrences.laneSpan

def Choreo.globalEventsFrom
    (laneBase : Nat) (choreo : Choreo) : List GlobalEvent :=
  choreo.compiledOccurrences.occurrences.map (·.globalEventFrom laneBase)

/-- Canonical preorder of global communication occurrences. Route arms and
parallel branches retain distinct event identities even when contracts match. -/
def Choreo.globalEvents (choreo : Choreo) : List GlobalEvent :=
  choreo.globalEventsFrom 0

def Choreo.eventCount : Choreo -> Nat
  | .send _ _ _ _ => 1
  | .seq left right | .par left right | .route _ left right =>
      left.eventCount + right.eventCount
  | .roll body => body.eventCount

@[simp]
theorem compiled_occurrences_length (choreo : Choreo) :
    choreo.compiledOccurrences.occurrences.length = choreo.eventCount := by
  induction choreo with
  | send => simp [Choreo.compiledOccurrences, compiledSourceFrom, Choreo.eventCount]
  | seq left right leftIH rightIH =>
      simp [Choreo.compiledOccurrences, compiledSourceFrom, Choreo.eventCount, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.compiledOccurrences, mergeParallelOccurrences, compiledSourceFrom,
        Choreo.eventCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.compiledOccurrences, mergeRouteOccurrences, compiledSourceFrom,
        Choreo.eventCount, leftIH, rightIH]
  | roll body bodyIH =>
      simp [Choreo.compiledOccurrences, recolorRollSource, recolorRollOccurrences,
        compiledSourceFrom, Choreo.eventCount, bodyIH]

@[simp]
theorem global_events_length (choreo : Choreo) :
    choreo.globalEvents.length = choreo.eventCount := by
  simp [Choreo.globalEvents, Choreo.globalEventsFrom]

theorem compiled_occurrence_lane_bound
    {occurrence : CompiledOccurrence} {occurrences : List CompiledOccurrence}
    (member : occurrence ∈ occurrences) :
    occurrence.lane < compiledLaneSpan occurrences := by
  induction occurrences with
  | nil => simp at member
  | cons head tail tailIH =>
      rcases List.mem_cons.mp member with rfl | tailMember
      · simp only [compiledLaneSpan]
        omega
      · have bound := tailIH tailMember
        simp [compiledLaneSpan]
        omega

theorem compiled_occurrences_lane_span_exact (choreo : Choreo) :
    choreo.compiledOccurrences.laneSpan =
      compiledLaneSpan choreo.compiledOccurrences.occurrences := by
  induction choreo with
  | send => rfl
  | seq => rfl
  | par => rfl
  | route => rfl
  | roll => rfl

theorem global_events_from_lane_bounds
    (choreo : Choreo)
    (laneBase : Nat)
    {event : GlobalEvent}
    (member : event ∈ choreo.globalEventsFrom laneBase) :
    laneBase ≤ event.lane /\ event.lane < laneBase + choreo.laneSpan := by
  have compiledMember : ∃ occurrence,
      occurrence ∈ choreo.compiledOccurrences.occurrences /\
        occurrence.globalEventFrom laneBase = event := by
    simpa [Choreo.globalEventsFrom] using List.mem_map.mp member
  obtain ⟨occurrence, occurrenceMember, rfl⟩ := compiledMember
  have occurrenceBound := compiled_occurrence_lane_bound occurrenceMember
  simp only [CompiledOccurrence.globalEventFrom, Choreo.laneSpan]
  exact ⟨by omega, by
    change laneBase + occurrence.lane < laneBase + choreo.compiledOccurrences.laneSpan
    rw [compiled_occurrences_lane_span_exact]
    exact Nat.add_lt_add_left occurrenceBound laneBase⟩

@[simp]
theorem global_events_from_length_eq
    (choreo : Choreo) (leftBase rightBase : Nat) :
    (choreo.globalEventsFrom leftBase).length =
      (choreo.globalEventsFrom rightBase).length := by
  simp [Choreo.globalEventsFrom]

theorem global_events_from_projected_actions_eq
    (role : Nat) (choreo : Choreo) (leftBase rightBase : Nat) :
    (choreo.globalEventsFrom leftBase).filterMap (·.action? role) =
      (choreo.globalEventsFrom rightBase).filterMap (·.action? role) := by
  unfold Choreo.globalEventsFrom
  simp only [List.filterMap_map]
  change choreo.compiledOccurrences.occurrences.filterMap (fun occurrence =>
      Choreo.localAction? role occurrence.sender occurrence.receiver
        occurrence.label occurrence.schema) =
    choreo.compiledOccurrences.occurrences.filterMap (fun occurrence =>
      Choreo.localAction? role occurrence.sender occurrence.receiver
        occurrence.label occurrence.schema)
  rfl

def Choreo.projectedActions (role : Nat) (choreo : Choreo) : List LocalAction :=
  choreo.compiledOccurrences.occurrences.filterMap (·.action? role)

@[simp]
theorem projected_actions_match_global_events (role : Nat) (choreo : Choreo) :
    choreo.projectedActions role = choreo.globalEvents.filterMap (·.action? role) := by
  unfold Choreo.projectedActions Choreo.globalEvents Choreo.globalEventsFrom
  simp only [List.filterMap_map]
  rfl

/-- The local descriptor event ID of one global occurrence is the number of
earlier global occurrences visible to that role. -/
def Choreo.localEventId (role globalId : Nat) (choreo : Choreo) : Nat :=
  (choreo.globalEvents.take globalId).filterMap (·.action? role) |>.length

/-- One syntactic roll occurrence over globally unique event identities. Nested
rolls precede their enclosing roll, matching `projectGraph`'s roll-row order. -/
structure GlobalRoll where
  events : List Nat
  conflicts : List Nat
  deriving Repr, DecidableEq

/-- Roles that can perform the first externally visible send in a branch. A
dynamic or intrinsic route has one controller exactly when both arms expose
the same unique first sender. -/
def Choreo.firstVisibleSenders : Choreo -> List Nat
  | .send sender _ _ _ => [sender]
  | .seq left right =>
      let visible := left.firstVisibleSenders
      if visible.isEmpty then right.firstVisibleSenders else visible
  | .par left right | .route _ left right =>
      (left.firstVisibleSenders ++ right.firstVisibleSenders).eraseDups
  | .roll body => body.firstVisibleSenders

def Choreo.routeController? (left right : Choreo) : Option Nat :=
  match (left.firstVisibleSenders ++ right.firstVisibleSenders).eraseDups with
  | [controller] => some controller
  | _ => none

/-- Controller rows follow the same preorder as route conflict identifiers. -/
def Choreo.routeControllers : Choreo -> List (Option Nat)
  | .send _ _ _ _ => []
  | .seq left right | .par left right =>
      left.routeControllers ++ right.routeControllers
  | .route _ left right =>
      routeController? left right ::
        (left.routeControllers ++ right.routeControllers)
  | .roll body => body.routeControllers

def Choreo.controllerForConflict?
    (choreo : Choreo) (conflict : Nat) : Option Nat :=
  (choreo.routeControllers[conflict]?).join

/-- Route authorities follow the global conflict preorder directly. Deployment
reasoning must not nominate one role projection as a surrogate global registry. -/
def Choreo.routeAuthorities : Choreo -> List RouteAuthority
  | .send _ _ _ _ => []
  | .seq left right | .par left right =>
      left.routeAuthorities ++ right.routeAuthorities
  | .route authority left right =>
      authority :: (left.routeAuthorities ++ right.routeAuthorities)
  | .roll body => body.routeAuthorities

def Choreo.authorityForConflict?
    (choreo : Choreo) (conflict : Nat) : Option RouteAuthority :=
  choreo.routeAuthorities[conflict]?

theorem Choreo.routeAuthorities_length (choreo : Choreo) :
    choreo.routeAuthorities.length = choreo.routeCount := by
  induction choreo with
  | send sender receiver label schema => rfl
  | seq left right leftIH rightIH | par left right leftIH rightIH =>
      simp [Choreo.routeAuthorities, Choreo.routeCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.routeAuthorities, Choreo.routeCount, leftIH, rightIH,
        Nat.add_assoc, Nat.add_comm]
  | roll body bodyIH =>
      simpa [Choreo.routeAuthorities, Choreo.routeCount] using bodyIH

theorem global_event_conflicts_from_length
    (choreo : Choreo) (conflictBase : Nat) :
    (choreo.globalEventConflictsFrom conflictBase).length =
      choreo.globalEvents.length := by
  induction choreo generalizing conflictBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | roll body bodyIH =>
      simpa only [Choreo.globalEventConflictsFrom, global_events_length,
        Choreo.eventCount] using bodyIH conflictBase

theorem global_event_conflicts_length (choreo : Choreo) :
    choreo.globalEventConflicts.length = choreo.globalEvents.length :=
  global_event_conflicts_from_length choreo 0

def Choreo.parallelCount : Choreo -> Nat
  | .send _ _ _ _ => 0
  | .seq left right | .route _ left right =>
      left.parallelCount + right.parallelCount
  | .par left right => 1 + left.parallelCount + right.parallelCount
  | .roll body => body.parallelCount

/-- Parallel memberships aligned one-for-one with `globalEvents`. These rows
are proof metadata only; production already encodes the same distinction in
canonical disjoint physical lanes. -/
def Choreo.globalEventParallelArmsFrom
    (parallelBase : Nat) : Choreo -> List (List ParallelArm)
  | .send _ _ _ _ => [[]]
  | .seq left right | .route _ left right =>
      left.globalEventParallelArmsFrom parallelBase ++
        right.globalEventParallelArmsFrom (parallelBase + left.parallelCount)
  | .par left right =>
      (left.globalEventParallelArmsFrom (parallelBase + 1)).map
          ({ parallel := parallelBase, arm := .left } :: ·) ++
        (right.globalEventParallelArmsFrom
          (parallelBase + 1 + left.parallelCount)).map
          ({ parallel := parallelBase, arm := .right } :: ·)
  | .roll body => body.globalEventParallelArmsFrom parallelBase

def Choreo.globalEventParallelArms (choreo : Choreo) : List (List ParallelArm) :=
  choreo.globalEventParallelArmsFrom 0

theorem global_event_parallel_arms_from_length
    (choreo : Choreo) (parallelBase : Nat) :
    (choreo.globalEventParallelArmsFrom parallelBase).length =
      choreo.globalEvents.length := by
  induction choreo generalizing parallelBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, Choreo.eventCount, leftIH, rightIH]
  | roll body bodyIH =>
      simpa only [Choreo.globalEventParallelArmsFrom, global_events_length,
        Choreo.eventCount] using bodyIH parallelBase

theorem global_event_parallel_arms_length (choreo : Choreo) :
    choreo.globalEventParallelArms.length = choreo.globalEvents.length :=
  global_event_parallel_arms_from_length choreo 0

def Choreo.globalRollsFrom
    (eventBase conflictBase : Nat) : Choreo -> List GlobalRoll
  | .send _ _ _ _ => []
  | .seq left right | .par left right =>
      left.globalRollsFrom eventBase conflictBase ++
        right.globalRollsFrom
          (eventBase + left.globalEvents.length)
          (conflictBase + left.routeCount)
  | .route _ left right =>
      left.globalRollsFrom eventBase (conflictBase + 1) ++
        right.globalRollsFrom
          (eventBase + left.globalEvents.length)
          (conflictBase + 1 + left.routeCount)
  | .roll body =>
      body.globalRollsFrom eventBase conflictBase ++ [{
        events := (List.range body.globalEvents.length).map (eventBase + ·)
        conflicts := (List.range body.routeCount).map (conflictBase + ·)
      }]

def Choreo.globalRolls (choreo : Choreo) : List GlobalRoll :=
  choreo.globalRollsFrom 0 0

def Choreo.rollCount : Choreo -> Nat
  | .send _ _ _ _ => 0
  | .seq left right | .par left right | .route _ left right =>
      left.rollCount + right.rollCount
  | .roll body => body.rollCount + 1

theorem global_rolls_from_length
    (choreo : Choreo) (eventBase conflictBase : Nat) :
    (choreo.globalRollsFrom eventBase conflictBase).length = choreo.rollCount := by
  induction choreo generalizing eventBase conflictBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp [Choreo.globalRollsFrom, Choreo.rollCount, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.globalRollsFrom, Choreo.rollCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalRollsFrom, Choreo.rollCount, leftIH, rightIH]
  | roll body bodyIH =>
      simp [Choreo.globalRollsFrom, Choreo.rollCount, bodyIH]

theorem global_rolls_length (choreo : Choreo) :
    choreo.globalRolls.length = choreo.rollCount :=
  global_rolls_from_length choreo 0 0

def Choreo.globalEventRollMembershipsFrom
    (rollBase : Nat) : Choreo -> List (List Nat)
  | .send _ _ _ _ => [[]]
  | .seq left right | .par left right | .route _ left right =>
      left.globalEventRollMembershipsFrom rollBase ++
        right.globalEventRollMembershipsFrom (rollBase + left.rollCount)
  | .roll body =>
      (body.globalEventRollMembershipsFrom rollBase).map
        (· ++ [rollBase + body.rollCount])

def Choreo.globalEventRollMemberships (choreo : Choreo) : List (List Nat) :=
  choreo.globalEventRollMembershipsFrom 0

private theorem global_event_roll_memberships_from_length
    (choreo : Choreo) (rollBase : Nat) :
    (choreo.globalEventRollMembershipsFrom rollBase).length = choreo.eventCount := by
  induction choreo generalizing rollBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp [Choreo.globalEventRollMembershipsFrom, Choreo.eventCount, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.globalEventRollMembershipsFrom, Choreo.eventCount, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventRollMembershipsFrom, Choreo.eventCount, leftIH, rightIH]
  | roll body bodyIH =>
      simp [Choreo.globalEventRollMembershipsFrom, Choreo.eventCount, bodyIH]

@[simp]
theorem global_event_roll_memberships_length (choreo : Choreo) :
    choreo.globalEventRollMemberships.length = choreo.globalEvents.length := by
  rw [global_events_length]
  exact global_event_roll_memberships_from_length choreo 0

theorem global_roll_conflicts_ignore_event_base
    (choreo : Choreo) (leftBase rightBase conflictBase : Nat) :
    (choreo.globalRollsFrom leftBase conflictBase).map GlobalRoll.conflicts =
      (choreo.globalRollsFrom rightBase conflictBase).map GlobalRoll.conflicts := by
  induction choreo generalizing leftBase rightBase conflictBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp only [Choreo.globalRollsFrom, List.map_append]
      rw [leftIH leftBase rightBase conflictBase]
      rw [rightIH
        (leftBase + left.globalEvents.length)
        (rightBase + left.globalEvents.length)
        (conflictBase + left.routeCount)]
  | par left right leftIH rightIH =>
      simp only [Choreo.globalRollsFrom, List.map_append]
      rw [leftIH leftBase rightBase conflictBase]
      rw [rightIH
        (leftBase + left.globalEvents.length)
        (rightBase + left.globalEvents.length)
        (conflictBase + left.routeCount)]
  | route authority left right leftIH rightIH =>
      simp only [Choreo.globalRollsFrom, List.map_append]
      rw [leftIH leftBase rightBase (conflictBase + 1)]
      rw [rightIH
        (leftBase + left.globalEvents.length)
        (rightBase + left.globalEvents.length)
        (conflictBase + 1 + left.routeCount)]
  | roll body bodyIH =>
      simp only [Choreo.globalRollsFrom, List.map_append, List.map_cons,
        List.map_nil]
      rw [bodyIH leftBase rightBase conflictBase]

private theorem filter_map_get_at_prefix_count
    (map? : α → Option β)
    {values : List α} {index : Nat} {value : α} {mapped : β}
    (source : values[index]? = some value)
    (maps : map? value = some mapped) :
    (values.filterMap map?)[(values.take index).filterMap map? |>.length]? =
      some mapped := by
  induction values generalizing index with
  | nil => simp at source
  | cons head tail inductionHypothesis =>
      cases index with
      | zero =>
          have headEq : head = value := Option.some.inj (by simpa using source)
          subst value
          simp [maps]
      | succ index =>
          have tailSource : tail[index]? = some value := by
            simpa using source
          cases headMap : map? head with
          | none =>
              simpa [headMap] using inductionHypothesis tailSource
          | some result =>
              simpa [headMap] using inductionHypothesis tailSource

/-- A global occurrence visible to one role appears at exactly the local event
ID obtained by counting the role-visible prefix. -/
theorem projected_action_at_local_event_id
    {choreo : Choreo} {role globalId : Nat}
    {event : GlobalEvent} {action : LocalAction}
    (eventAt : choreo.globalEvents[globalId]? = some event)
    (projected : event.action? role = some action) :
    (choreo.projectedActions role)[choreo.localEventId role globalId]? =
      some action := by
  rw [projected_actions_match_global_events]
  exact filter_map_get_at_prefix_count (map? := (·.action? role)) eventAt projected

theorem global_event_send_recv_duality
    (event : GlobalEvent)
    (distinct : Nat.beq event.sender event.receiver = false) :
    event.action? event.sender =
        some (.send event.receiver event.label event.schema) /\
      event.action? event.receiver =
        some (.recv event.sender event.label event.schema) := by
  exact local_action_send_recv_duality distinct

theorem global_event_receive_contract_unique
    {event : GlobalEvent} {peer label schema : Nat}
    (distinct : Nat.beq event.sender event.receiver = false)
    (projected : event.action? event.receiver = some (.recv peer label schema)) :
    peer = event.sender /\ label = event.label /\ schema = event.schema := by
  have expected := (global_event_send_recv_duality event distinct).2
  rw [expected] at projected
  have actionEq := Option.some.inj projected
  cases actionEq
  exact ⟨rfl, rfl, rfl⟩

end Hibana
