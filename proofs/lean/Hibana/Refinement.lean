import Hibana.EventGraph

namespace Hibana

/-- The projection topology retained across virtual-lane packing in Rust. -/
structure ProjectionEvent where
  action : LocalAction
  deriving DecidableEq

structure ProjectionRoute where
  conflict : Nat
  authority : RouteAuthority
  leftEvents : List Nat
  rightEvents : List Nat
  reentry : ReentryMode
  deriving DecidableEq

structure ProjectionRoll where
  events : List Nat
  deriving DecidableEq

private def insertProjectionRoute
    (route : ProjectionRoute) : List ProjectionRoute -> List ProjectionRoute
  | [] => [route]
  | head :: tail =>
      if route.conflict <= head.conflict then route :: head :: tail
      else head :: insertProjectionRoute route tail

private def sortProjectionRoutes (routes : List ProjectionRoute) : List ProjectionRoute :=
  routes.foldr insertProjectionRoute []

private def projectionRollBefore (left right : ProjectionRoll) : Bool :=
  match left.events.head?, right.events.head? with
  | none, none => left.events.length <= right.events.length
  | none, some _ => false
  | some _, none => true
  | some leftStart, some rightStart =>
      leftStart < rightStart ||
        (leftStart == rightStart && left.events.length <= right.events.length)

private def insertProjectionRoll
    (roll : ProjectionRoll) : List ProjectionRoll -> List ProjectionRoll
  | [] => [roll]
  | head :: tail =>
      if projectionRollBefore roll head then roll :: head :: tail
      else head :: insertProjectionRoll roll tail

private def sortProjectionRolls (rolls : List ProjectionRoll) : List ProjectionRoll :=
  rolls.foldr insertProjectionRoll []

structure ProjectionShape where
  events : List ProjectionEvent
  rolls : List ProjectionRoll
  routes : List ProjectionRoute
  deriving DecidableEq

def EventGraph.projectionShape (graph : EventGraph) : ProjectionShape := {
  events := graph.events.map fun event => {
    action := event.action
  }
  rolls := sortProjectionRolls <| graph.rolls.filterMap fun roll =>
    if roll.events.isEmpty then none else some { events := roll.events }
  routes := sortProjectionRoutes <| graph.routes.filterMap fun route =>
    if route.leftEvents.isEmpty && route.rightEvents.isEmpty then
      none
    else
      some {
        conflict := route.conflict
        authority := route.authority
        leftEvents := route.leftEvents
        rightEvents := route.rightEvents
        reentry := route.reentry
      }
}

/-- A host-generated descriptor shape paired with its normalized source term. -/
structure ProjectionCertificate where
  role : Nat
  choreo : Choreo
  topology : ProjectionShape

def ProjectionCertificate.check (certificate : ProjectionCertificate) : Bool :=
  let graph := projectGraph certificate.role certificate.choreo
  graph.check && decide (certificate.topology = graph.projectionShape)

def ProjectionCertificate.RefinesTopology (certificate : ProjectionCertificate) : Prop :=
  let graph := projectGraph certificate.role certificate.choreo
  graph.WellFormed /\ certificate.topology = graph.projectionShape

/-- An accepted production descriptor certificate agrees with the normalized projection. -/
theorem projection_certificate_sound
    {certificate : ProjectionCertificate}
    (accepted : certificate.check = true) : certificate.RefinesTopology := by
  simp [ProjectionCertificate.check] at accepted
  exact ⟨artifact_checker_sound accepted.1, accepted.2⟩

end Hibana
