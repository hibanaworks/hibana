import Hibana.ElasticIterationQueue

namespace Hibana

structure ElasticRoutePublication where
  key : ElasticConflictKey
  arm : RouteArm
  deriving Repr, DecidableEq

def routePublicationIterations
    (published : List ElasticRoutePublication)
    (conflict : Nat) : List Nat :=
  published.filterMap fun publication =>
    if publication.key.conflict = conflict then
      some publication.key.iteration
    else
      none

@[simp]
private theorem route_publication_iterations_append
    (published : List ElasticRoutePublication)
    (publication : ElasticRoutePublication)
    (conflict : Nat) :
    routePublicationIterations (published ++ [publication]) conflict =
      routePublicationIterations published conflict ++
        if publication.key.conflict = conflict then
          [publication.key.iteration]
        else
          [] := by
  by_cases same : publication.key.conflict = conflict
  · simp [routePublicationIterations, same]
  · simp [routePublicationIterations, same]

/-- Append-only proof history for route publications. Production stores only
its compact affine route cell and descriptor-admitted branch evidence. -/
structure ElasticRouteHistory where
  published : List ElasticRoutePublication
  nextIteration : Nat -> Nat

def ElasticRouteHistory.initial : ElasticRouteHistory := {
  published := []
  nextIteration := fun _ => 0
}

structure ElasticRouteHistory.WellFormed
    (history : ElasticRouteHistory) : Prop where
  publishedNodup : history.published.Nodup
  exactIterations : forall conflict,
    routePublicationIterations history.published conflict =
      List.range (history.nextIteration conflict)

def ElasticRouteHistory.nextPublication
    (history : ElasticRouteHistory)
    (conflict : Nat)
    (arm : RouteArm) : ElasticRoutePublication := {
  key := {
    conflict
    iteration := history.nextIteration conflict
  }
  arm
}

def ElasticRouteHistory.publish
    (history : ElasticRouteHistory)
    (conflict : Nat)
    (arm : RouteArm) : ElasticRouteHistory :=
  let publication := history.nextPublication conflict arm
  {
    published := history.published ++ [publication]
    nextIteration := fun query =>
      if conflict = query then history.nextIteration query + 1
      else history.nextIteration query
  }

theorem elastic_route_history_initial_well_formed :
    ElasticRouteHistory.initial.WellFormed := by
  refine {
    publishedNodup := by simp [ElasticRouteHistory.initial]
    exactIterations := ?_
  }
  intro conflict
  simp [ElasticRouteHistory.initial, routePublicationIterations]

theorem elastic_route_next_publication_is_fresh
    {history : ElasticRouteHistory}
    (wellFormed : history.WellFormed)
    (conflict : Nat)
    (arm : RouteArm) :
    history.nextPublication conflict arm ∉ history.published := by
  intro member
  have iterationMember :
      history.nextIteration conflict ∈
        routePublicationIterations history.published conflict := by
    apply List.mem_filterMap.mpr
    exact ⟨history.nextPublication conflict arm, member, by
      simp [ElasticRouteHistory.nextPublication]⟩
  rw [wellFormed.exactIterations conflict] at iterationMember
  exact Nat.lt_irrefl _ (List.mem_range.mp iterationMember)

theorem elastic_route_publish_preserves_well_formed
    {history : ElasticRouteHistory}
    (wellFormed : history.WellFormed)
    (conflict : Nat)
    (arm : RouteArm) :
    (history.publish conflict arm).WellFormed := by
  let publication := history.nextPublication conflict arm
  have fresh : publication ∉ history.published :=
    elastic_route_next_publication_is_fresh wellFormed conflict arm
  refine {
    publishedNodup := ?_
    exactIterations := ?_
  }
  · change (history.published ++ [publication]).Nodup
    rw [List.nodup_append]
    refine ⟨wellFormed.publishedNodup, by simp, ?_⟩
    intro prior priorMember appended appendedMember
    have appendedEq : appended = publication := by simpa using appendedMember
    subst appended
    intro same
    apply fresh
    simpa [same] using priorMember
  · intro query
    change routePublicationIterations (history.published ++ [publication]) query =
      List.range (if conflict = query then
        history.nextIteration query + 1
      else
        history.nextIteration query)
    rw [route_publication_iterations_append]
    by_cases same : conflict = query
    · subst query
      simp [publication, ElasticRouteHistory.nextPublication,
        wellFormed.exactIterations, List.range_succ]
    · simp [publication, ElasticRouteHistory.nextPublication, same,
        wellFormed.exactIterations]

theorem elastic_route_publish_appends_exact_publication
    (history : ElasticRouteHistory)
    (conflict : Nat)
    (arm : RouteArm) :
    (history.publish conflict arm).published =
        history.published ++ [history.nextPublication conflict arm] /\
      (history.nextPublication conflict arm).key.conflict = conflict /\
      (history.nextPublication conflict arm).key.iteration =
        history.nextIteration conflict /\
      (history.nextPublication conflict arm).arm = arm := by
  exact ⟨rfl, rfl, rfl, rfl⟩

/-- A selected static route membership fixes both the conflict and arm of the
next publication; the append operation contributes only the affine ordinal. -/
theorem elastic_route_publication_binds_static_membership
    {choreo : Choreo}
    {globalId : Nat}
    {memberships : List ConflictArm}
    {membership : ConflictArm}
    {history : ElasticRouteHistory}
    (wellFormed : history.WellFormed)
    (membershipsAt :
      choreo.globalEventConflicts[globalId]? = some memberships)
    (member : membership ∈ memberships) :
    let publication :=
      history.nextPublication membership.conflict membership.arm
    let next := history.publish membership.conflict membership.arm
    next.WellFormed /\
      next.published = history.published ++ [publication] /\
      publication.key.conflict = membership.conflict /\
      publication.key.iteration = history.nextIteration membership.conflict /\
      publication.arm = membership.arm /\
      choreo.globalEventConflicts[globalId]? = some memberships /\
      membership ∈ memberships := by
  exact ⟨
    elastic_route_publish_preserves_well_formed
      wellFormed membership.conflict membership.arm,
    rfl,
    rfl,
    rfl,
    rfl,
    membershipsAt,
    member
  ⟩

/-- Opposite arms of one conflict may be published before any remote receive;
their proof identities remain ordered and distinct. -/
theorem elastic_route_history_models_alternating_pipelining :
    let first := ElasticRouteHistory.initial.publish 0 .left
    let second := first.publish 0 .right
    second.WellFormed /\
      second.published.map ElasticRoutePublication.arm = [.left, .right] /\
      routePublicationIterations second.published 0 = [0, 1] := by
  let first := ElasticRouteHistory.initial.publish 0 .left
  let second := first.publish 0 .right
  have initialWellFormed := elastic_route_history_initial_well_formed
  have firstWellFormed : first.WellFormed :=
    elastic_route_publish_preserves_well_formed initialWellFormed 0 .left
  have secondWellFormed : second.WellFormed :=
    elastic_route_publish_preserves_well_formed firstWellFormed 0 .right
  refine ⟨secondWellFormed, ?_⟩
  decide

end Hibana
