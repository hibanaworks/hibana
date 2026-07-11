import Hibana.Syntax

namespace Hibana

/-- One globally unique communication occurrence. Its index in
`Choreo.globalEvents` is the protocol event identity. -/
structure GlobalEvent where
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  deriving Repr, DecidableEq

/-- One global route-membership fact. Conflict ordinals are assigned by the
canonical choreography preorder and are shared by every role projection. -/
structure ConflictArm where
  conflict : Nat
  arm : RouteArm
  deriving Repr, DecidableEq, BEq

def GlobalEvent.action? (event : GlobalEvent) (role : Nat) : Option LocalAction :=
  Choreo.localAction? role event.sender event.receiver event.label event.schema

/-- Canonical preorder of global communication occurrences. Route arms and
parallel branches retain distinct event identities even when contracts match. -/
def Choreo.globalEvents : Choreo -> List GlobalEvent
  | .send sender receiver label schema => [{ sender, receiver, label, schema }]
  | .seq left right | .par left right => left.globalEvents ++ right.globalEvents
  | .route _ left right => left.globalEvents ++ right.globalEvents
  | .roll body => body.globalEvents

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
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents, leftIH, rightIH]
  | par left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents, leftIH, rightIH]
  | route authority left right leftIH rightIH =>
      simp [Choreo.globalEventConflictsFrom, Choreo.globalEvents, leftIH, rightIH]
  | roll body bodyIH =>
      simpa [Choreo.globalEventConflictsFrom, Choreo.globalEvents] using bodyIH conflictBase

theorem global_event_conflicts_length (choreo : Choreo) :
    choreo.globalEventConflicts.length = choreo.globalEvents.length :=
  global_event_conflicts_from_length choreo 0

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
