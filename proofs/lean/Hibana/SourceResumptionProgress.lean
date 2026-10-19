import Hibana.SourceOccurrenceProjection

namespace Hibana
open EndpointDelegation

/-- Erased intervals in a finite source expansion. The resumed copy precedes
the continuation. These are source positions, not runtime epochs,
an owner table, or bounds imposed on execution. -/
structure SourceResumption where
  bodyStart : Nat
  bodyEnd : Nat
  tailStart : Nat
  tailEnd : Nat
  deriving Repr, DecidableEq

private def SourceUnfolding.copyRanges : {body expanded : Source} →
    SourceUnfolding (.roll body) expanded → Nat → List (Nat × Nat)
  | _, expanded, .rollOnce _, start => [(start, start + expanded.eventCount)]
  | _, .seq earlierExpanded nextExpanded, .rollMore earlier _, start =>
      earlier.copyRanges start ++
        [(start + earlierExpanded.eventCount,
          start + earlierExpanded.eventCount + nextExpanded.eventCount)]

/-- Extract sequential resumption boundaries from the constructors that
actually introduce them. A plain source par adds none. Recursion offsets use
expanded positions, retaining distinct copies and branch sites. -/
def SourceUnfolding.resumptions : {source expanded : Source} →
    SourceUnfolding source expanded → Nat → List SourceResumption
  | _, _, .send _, _ => []
  | _, .seq first _, .seq left right, start
  | _, .par first _, .par left right, start
  | _, .route _ first _, .route _ left right, start =>
      left.resumptions start ++ right.resumptions (start + first.eventCount)
  | _, _, .rollOnce body, start => body.resumptions start
  | _, .seq first _, .rollMore earlier next, start =>
      earlier.resumptions start ++ next.resumptions (start + first.eventCount)
  | _, .seq first _, .resumeAfterContinuation earlier next, start =>
      earlier.resumptions start ++ next.resumptions (start + first.eventCount)
  | _, .seq first (.par second tailSource), .resumeAlongsideContinuation earlier next tail, start =>
      let nextStart := start + first.eventCount
      let tailStart := nextStart + second.eventCount
      let scopes := next.copyRanges nextStart |>.map fun (bodyStart, bodyEnd) =>
        { bodyStart, bodyEnd, tailStart, tailEnd := tailStart + tailSource.eventCount }
      scopes ++ earlier.resumptions start ++ next.resumptions nextStart ++ tail.resumptions tailStart

private def pointEvent : SourcePoint → Nat
  | .accepted event => event
  | .committed action => action.event

private def SourceResumption.inBody (scope : SourceResumption) (event : Nat) : Bool :=
  scope.bodyStart ≤ event && event < scope.bodyEnd && event < scope.tailStart

private def SourceResumption.inTail (scope : SourceResumption) (event : Nat) : Bool :=
  scope.tailStart ≤ event && event < scope.tailEnd

/-- Outgoing continuations use this actor's current body after it enters that
body. Incoming old continuations do not acquire a new prerequisite. Reads are
restricted to source points owned by the target actor. The occurrence word is
proof interpretation, not an additional application progress state. -/
def SourceOccurrenceCut.resumptionDepends (plan : SourceOccurrenceCut source)
    (observed : SourcePoint → Bool) : SourcePoint → SourcePoint → Bool
  | .committed before, .accepted after =>
      let target := SourcePoint.accepted after
      match occurrenceActor? plan target with
      | none => false
      | some actor =>
          occurrenceActor? plan (.committed before) == some actor &&
            (plan.unfolding.resumptions 0).any fun scope =>
              scope.inBody before.event && scope.inTail after &&
                (occurrenceLocalDomain plan actor).any fun point =>
                  scope.inBody (pointEvent point) && observed point
  | _, _ => false

def SourceOccurrenceCut.resumptionReady (plan : SourceOccurrenceCut source)
    (point : SourcePoint) : Bool :=
  (sourceVisitPoints plan.path.bindings).all fun before =>
    !plan.resumptionDepends plan.observed before point || plan.observed before

/-- Every added prerequisite is a forward dependency in this same expansion.
It cannot turn an earlier body or an independent par sibling into a waiter
for the later continuation. -/
theorem a_resumption_dependency_increases_source_rank
    {plan : SourceOccurrenceCut source} {observed : SourcePoint → Bool}
    {before after : SourcePoint}
    (depends : plan.resumptionDepends observed before after = true) :
    before.rank < after.rank := by
  cases before with
  | accepted => contradiction
  | committed before =>
      cases after with
      | committed => contradiction
      | accepted after =>
          unfold SourceOccurrenceCut.resumptionDepends at depends
          cases actorAt : occurrenceActor? plan (.accepted after) with
          | none => simp only [actorAt, Bool.false_eq_true] at depends
          | some actor =>
              have scopes := (Bool.and_eq_true_iff.mp
                (by simpa only [actorAt] using depends)).2
              obtain ⟨scope, _, enabled⟩ := List.any_eq_true.mp scopes
              have fields := Bool.and_eq_true_iff.mp enabled
              have regions := Bool.and_eq_true_iff.mp fields.1
              have inside := Bool.and_eq_true_iff.mp regions.1
              have outside := Bool.and_eq_true_iff.mp regions.2
              have earlier : before.event < after := by
                have upper : before.event < scope.tailStart := of_decide_eq_true inside.2
                have lower : scope.tailStart ≤ after := of_decide_eq_true outside.1
                omega
              cases sideAt : before.side <;> simp only [SourcePoint.rank, sideAt] <;> omega

theorem a_resumption_dependency_stays_with_its_actor
    {plan : SourceOccurrenceCut source} {observed : SourcePoint → Bool}
    {before after : SourcePoint}
    (depends : plan.resumptionDepends observed before after = true) :
    occurrenceActor? plan before = occurrenceActor? plan after := by
  cases before with
  | accepted => contradiction
  | committed before =>
      cases after with
      | committed => contradiction
      | accepted after =>
          unfold SourceOccurrenceCut.resumptionDepends at depends
          cases actorAt : occurrenceActor? plan (.accepted after) with
          | none => simp only [actorAt, Bool.false_eq_true] at depends
          | some actor =>
              exact (beq_iff_eq.mp (Bool.and_eq_true_iff.mp
                (by simpa only [actorAt] using depends)).1)

private theorem any_same_on_domain {items : List α} {left right : α → Bool}
    (same : ∀ item ∈ items, left item = right item) : items.any left = items.any right := by
  induction items with
  | nil => rfl
  | cons head tail ih =>
      simp only [List.any_cons, same head (by simp only [List.mem_cons, true_or])]
      rw [ih (fun item member => same item (List.mem_cons_of_mem _ member))]

private theorem all_same_on_domain {items : List α} {left right : α → Bool}
    (same : ∀ item ∈ items, left item = right item) : items.all left = items.all right := by
  induction items with
  | nil => rfl
  | cons head tail ih =>
      simp only [List.all_cons, same head (by simp only [List.mem_cons, true_or])]
      rw [ih (fun item member => same item (List.mem_cons_of_mem _ member))]

private theorem resumption_depends_uses_local_history
    (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (before after : SourcePoint) (here : occurrenceActor? plan after = some actor) :
    plan.resumptionDepends (fun point => points.contains point) before after =
      plan.resumptionDepends
        (fun point => (occurrenceLocalHistory plan actor points).contains point) before after := by
  cases before with
  | accepted => rfl
  | committed before =>
      cases after with
      | committed => rfl
      | accepted after =>
          simp only [SourceOccurrenceCut.resumptionDepends, here]
          apply congrArg (fun value => occurrenceActor? plan (.committed before) == some actor && value)
          apply any_same_on_domain
          intro scope _
          apply congrArg (fun value => scope.inBody before.event && scope.inTail after && value)
          apply any_same_on_domain
          intro point member
          have owned : occurrenceActor? plan point = some actor :=
            of_decide_eq_true (List.mem_filter.mp member).2
          rw [occurrence_local_history_contains plan points point owned]

/-- The extra query has exactly the same answer on this actor's projected
word. Other actors' observations need not be available, equal, or shared. -/
theorem a_resumption_query_uses_only_its_actor_history
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint)
    (here : occurrenceActor? plan point = some actor) :
    (occurrenceCut plan points).resumptionReady point =
      (occurrenceCut plan (occurrenceLocalHistory plan actor points)).resumptionReady point := by
  change (sourceVisitPoints plan.path.bindings).all
      (fun before => !plan.resumptionDepends (fun step => points.contains step) before point ||
        points.contains before) =
    (sourceVisitPoints plan.path.bindings).all
      (fun before => !plan.resumptionDepends
        (fun step => (occurrenceLocalHistory plan actor points).contains step) before point ||
        (occurrenceLocalHistory plan actor points).contains before)
  apply all_same_on_domain
  intro before _
  rw [resumption_depends_uses_local_history plan points before point here]
  by_cases depends : plan.resumptionDepends
      (fun step => (occurrenceLocalHistory plan actor points).contains step) before point = true
  · have owned := (a_resumption_dependency_stays_with_its_actor depends).trans here
    rw [occurrence_local_history_contains plan points before owned]
  · simp only [Bool.eq_false_iff.mpr depends, Bool.not_false, Bool.true_or]

/-- Choose a minimal unfinished source point for the proof, not for scheduling.
Both the original causal edges and the local resumption edges point forward,
so their union cannot strand an unfinished finite cut. No global-ready or
desired Rust call is a premise. -/
theorem an_unfinished_resumption_cut_has_a_projected_operation
    (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (unfinished : (occurrenceCut plan points).Unfinished) :
    ∃ actor point next,
      occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true ∧
      occurrenceWireReady points point = true ∧
      (occurrenceCut plan (occurrenceLocalHistory plan actor points)).resumptionReady point = true ∧
      SourceOccurrenceStep (occurrenceCut plan points) point next := by
  let cut := occurrenceCut plan points
  let domain := sourceVisitPoints plan.path.bindings
  let remaining := domain.filter fun point => !cut.observed point
  have nonempty : remaining ≠ [] := by
    obtain ⟨point, member, pending⟩ := unfinished
    have present : point ∈ remaining := by simp only [remaining, domain,
      List.mem_filter, Bool.not_eq_true']; exact ⟨member, pending⟩
    intro empty
    simp only [empty, List.not_mem_nil] at present
  let point := remaining.minOn SourcePoint.rank nonempty
  have member : point ∈ remaining := List.minOn_mem
  obtain ⟨present, pending⟩ : point ∈ domain ∧ cut.observed point = false := by
    simpa only [remaining, List.mem_filter, Bool.not_eq_true'] using member
  have lowerDone : ∀ before ∈ domain, before.rank < point.rank → cut.observed before = true := by
    intro before beforePresent earlier
    cases atBefore : cut.observed before with
    | true => rfl
    | false =>
        have belongs : before ∈ remaining := by
          simp only [remaining, List.mem_filter, Bool.not_eq_true']; exact ⟨beforePresent, atBefore⟩
        have minimal : point.rank ≤ before.rank := List.apply_minOn_le_of_mem belongs
        exact False.elim (Nat.not_le_of_gt earlier minimal)
  have checked : cut.ready point = true := by
    apply Bool.and_eq_true_iff.mpr
    refine ⟨Bool.and_eq_true_iff.mpr ⟨by simpa only [domain, List.contains_iff_mem] using present,
      by simp only [pending, Bool.not_false]⟩, List.all_eq_true.mpr ?_⟩
    intro before beforePresent
    change (!sourcePointDepends plan.expanded plan.path.bindings before point ||
      cut.observed before) = true
    by_cases edgeAt : sourcePointDepends plan.expanded plan.path.bindings before point = true
    · have done := lowerDone before beforePresent (source_point_dependency_increases_rank
        (source_point_dependency_check_is_exact.mp edgeAt))
      simp only [done, Bool.or_true]
    · simp only [Bool.eq_false_iff.mpr edgeAt, Bool.not_false, Bool.true_or]
  have resumed : cut.resumptionReady point = true := by
    apply List.all_eq_true.mpr
    intro before beforePresent
    by_cases edgeAt : cut.resumptionDepends cut.observed before point = true
    · have done := lowerDone before beforePresent (a_resumption_dependency_increases_source_rank edgeAt)
      simp only [edgeAt, Bool.not_true, Bool.false_or, done]
    · simp only [Bool.eq_false_iff.mpr edgeAt, Bool.not_false, Bool.true_or]
  obtain ⟨actor, localChecked, wire⟩ := (projected_occurrence_readiness_is_exact plan points point).mp checked
  have belongs : point ∈ occurrenceLocalDomain plan actor :=
    List.contains_iff_mem.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp localChecked).1).1
  have here : occurrenceActor? plan point = some actor :=
    of_decide_eq_true (List.mem_filter.mp belongs).2
  have localResumed :
      (occurrenceCut plan (occurrenceLocalHistory plan actor points)).resumptionReady point = true := by
    rw [← a_resumption_query_uses_only_its_actor_history plan points point here]
    exact resumed
  obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition cut point checked
  exact ⟨actor, point, next, localChecked, wire, localResumed, step⟩

end Hibana
