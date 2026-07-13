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

def Choreo.laneSpan : Choreo -> Nat
  | .send _ _ _ _ => 1
  | .seq left right | .route _ left right => max left.laneSpan right.laneSpan
  | .par left right => left.laneSpan + right.laneSpan
  | .roll body => body.laneSpan

/-- Canonical production lane assignment. Sequential and exclusive route arms
reuse lanes; parallel arms receive disjoint contiguous lane spans. -/
def Choreo.globalEventsFrom (laneBase : Nat) : Choreo -> List GlobalEvent
  | .send sender receiver label schema =>
      [{ sender, receiver, label, schema, lane := laneBase }]
  | .seq left right | .route _ left right =>
      left.globalEventsFrom laneBase ++ right.globalEventsFrom laneBase
  | .par left right =>
      left.globalEventsFrom laneBase ++
        right.globalEventsFrom (laneBase + left.laneSpan)
  | .roll body => body.globalEventsFrom laneBase

/-- Canonical preorder of global communication occurrences. Route arms and
parallel branches retain distinct event identities even when contracts match. -/
def Choreo.globalEvents (choreo : Choreo) : List GlobalEvent :=
  choreo.globalEventsFrom 0

theorem global_events_from_lane_bounds
    (choreo : Choreo)
    (laneBase : Nat)
    {event : GlobalEvent}
    (member : event ∈ choreo.globalEventsFrom laneBase) :
    laneBase ≤ event.lane /\ event.lane < laneBase + choreo.laneSpan := by
  induction choreo generalizing laneBase with
  | send sender receiver label schema =>
      simp only [Choreo.globalEventsFrom, List.mem_singleton] at member
      subst event
      simp [Choreo.laneSpan]
  | seq left right leftIH rightIH =>
      rcases List.mem_append.mp member with leftMember | rightMember
      · have bounds := leftIH laneBase leftMember
        exact ⟨bounds.1, Nat.lt_of_lt_of_le bounds.2 (by
          simp [Choreo.laneSpan]
          omega)⟩
      · have bounds := rightIH laneBase rightMember
        exact ⟨bounds.1, Nat.lt_of_lt_of_le bounds.2 (by
          simp [Choreo.laneSpan]
          omega)⟩
  | par left right leftIH rightIH =>
      rcases List.mem_append.mp member with leftMember | rightMember
      · have bounds := leftIH laneBase leftMember
        exact ⟨bounds.1, by
          simp [Choreo.laneSpan]
          omega⟩
      · have bounds := rightIH (laneBase + left.laneSpan) rightMember
        exact ⟨Nat.le_trans (by omega) bounds.1, by
          simp [Choreo.laneSpan]
          omega⟩
  | route authority left right leftIH rightIH =>
      rcases List.mem_append.mp member with leftMember | rightMember
      · have bounds := leftIH laneBase leftMember
        exact ⟨bounds.1, Nat.lt_of_lt_of_le bounds.2 (by
          simp [Choreo.laneSpan]
          omega)⟩
      · have bounds := rightIH laneBase rightMember
        exact ⟨bounds.1, Nat.lt_of_lt_of_le bounds.2 (by
          simp [Choreo.laneSpan]
          omega)⟩
  | roll body bodyIH =>
      simpa [Choreo.globalEventsFrom, Choreo.laneSpan] using bodyIH laneBase member

/-- Canonical parallel arms occupy disjoint physical lane intervals. This is the
lane-level noninterference fact used by the message-erased endpoint runtime. -/
theorem parallel_global_event_lanes_are_disjoint
    (left right : Choreo)
    (laneBase : Nat)
    {leftEvent rightEvent : GlobalEvent}
    (leftMember : leftEvent ∈ left.globalEventsFrom laneBase)
    (rightMember : rightEvent ∈
      right.globalEventsFrom (laneBase + left.laneSpan)) :
    leftEvent.lane ≠ rightEvent.lane := by
  have leftBounds := global_events_from_lane_bounds left laneBase leftMember
  have rightBounds := global_events_from_lane_bounds right
    (laneBase + left.laneSpan) rightMember
  omega

@[simp]
theorem global_events_from_length_eq
    (choreo : Choreo) (leftBase rightBase : Nat) :
    (choreo.globalEventsFrom leftBase).length =
      (choreo.globalEventsFrom rightBase).length := by
  induction choreo generalizing leftBase rightBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.length_append]
      rw [leftIH leftBase rightBase, rightIH leftBase rightBase]
  | par left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.length_append]
      rw [leftIH leftBase rightBase,
        rightIH (leftBase + left.laneSpan) (rightBase + left.laneSpan)]
  | route authority left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.length_append]
      rw [leftIH leftBase rightBase, rightIH leftBase rightBase]
  | roll body bodyIH =>
      exact bodyIH leftBase rightBase

theorem global_events_from_projected_actions_eq
    (role : Nat) (choreo : Choreo) (leftBase rightBase : Nat) :
    (choreo.globalEventsFrom leftBase).filterMap (·.action? role) =
      (choreo.globalEventsFrom rightBase).filterMap (·.action? role) := by
  induction choreo generalizing leftBase rightBase with
  | send sender receiver label schema =>
      cases actionCase : Choreo.localAction? role sender receiver label schema <;>
        simp [Choreo.globalEventsFrom, GlobalEvent.action?, actionCase]
  | seq left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.filterMap_append]
      rw [leftIH leftBase rightBase, rightIH leftBase rightBase]
  | par left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.filterMap_append]
      rw [leftIH leftBase rightBase,
        rightIH (leftBase + left.laneSpan) (rightBase + left.laneSpan)]
  | route authority left right leftIH rightIH =>
      simp only [Choreo.globalEventsFrom, List.filterMap_append]
      rw [leftIH leftBase rightBase, rightIH leftBase rightBase]
  | roll body bodyIH =>
      exact bodyIH leftBase rightBase

def Choreo.projectedActions (role : Nat) (choreo : Choreo) : List LocalAction :=
  choreo.globalEvents.filterMap (·.action? role)

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

def Choreo.routeCount : Choreo -> Nat
  | .send _ _ _ _ => 0
  | .seq left right | .par left right => left.routeCount + right.routeCount
  | .route _ left right => 1 + left.routeCount + right.routeCount
  | .roll body => body.routeCount

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

/-- Route memberships aligned one-for-one with `globalEvents`. The conflict
base follows the same preorder as role projection, so global exclusion never
depends on choosing one role's local descriptor as a second authority. -/
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

theorem global_event_conflicts_from_length
    (choreo : Choreo) (conflictBase : Nat) :
    (choreo.globalEventConflictsFrom conflictBase).length =
      choreo.globalEvents.length := by
  induction choreo generalizing conflictBase with
  | send => rfl
  | seq left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simpa [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, leftIH, rightIH] using
        global_events_from_length_eq right 0 left.laneSpan
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, leftIH, rightIH]
  | roll body bodyIH =>
      simpa [Choreo.globalEventConflictsFrom, Choreo.globalEvents] using bodyIH conflictBase

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
        Choreo.globalEventsFrom, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simpa [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, leftIH, rightIH] using
        global_events_from_length_eq right 0 left.laneSpan
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents,
        Choreo.globalEventsFrom, leftIH, rightIH]
  | roll body bodyIH =>
      simpa [Choreo.globalEventParallelArmsFrom, Choreo.globalEvents] using
        bodyIH parallelBase

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
