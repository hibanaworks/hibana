import Hibana.SourceOccurrenceProgress

namespace Hibana
open EndpointDelegation

private def pointAction : SourcePoint → ActionId
  | .accepted event => ⟨event, .send⟩
  | .committed action => action

private theorem entry_point_action (id : ActionId) : pointAction id.entryPoint = id := by
  cases id with
  | mk event side => cases side <;> rfl

/-- The holder of a projected operation is derived from the accepted binding,
not from a shared owner table. Acceptance belongs to the sending actor. -/
def occurrenceActor? (plan : SourceOccurrenceCut source) (point : SourcePoint) : Option Nat :=
  (action? plan.path.bindings (pointAction point)).map Action.actor

def occurrenceLocalDomain (plan : SourceOccurrenceCut source) (actor : Nat) : List SourcePoint :=
  (sourceVisitPoints plan.path.bindings).filter fun point => decide (occurrenceActor? plan point = some actor)

/-- Projection of the one chronological word onto an actor's own observations.
This is erased model data, not another runtime progress store. -/
def occurrenceLocalHistory (plan : SourceOccurrenceCut source) (actor : Nat)
    (points : List SourcePoint) : List SourcePoint :=
  points.filter fun point => decide (occurrenceActor? plan point = some actor)

private def localDepends (plan : SourceOccurrenceCut source) : SourcePoint → SourcePoint → Bool
  | .accepted _, _ => false
  | before, after => sourcePointDepends plan.expanded plan.path.bindings before after

/-- A projected query reads only this actor's local domain, local history and
immutable predecessor relation. Message acceptance is checked separately. -/
def occurrenceLocalReady (plan : SourceOccurrenceCut source) (actor : Nat)
    (history : List SourcePoint) (point : SourcePoint) : Bool :=
  (occurrenceLocalDomain plan actor).contains point && !history.contains point &&
    (occurrenceLocalDomain plan actor).all fun before =>
      !localDepends plan before point || history.contains before

def occurrenceWireReady (points : List SourcePoint) : SourcePoint → Bool
  | .committed ⟨event, .send⟩ | .committed ⟨event, .receive⟩ => points.contains (.accepted event)
  | _ => true

private theorem local_history_contains
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint)
    (atActor : occurrenceActor? plan point = some actor) :
    (occurrenceLocalHistory plan actor points).contains point = points.contains point := by
  have same : (occurrenceLocalHistory plan actor points).contains point = true ↔
      points.contains point = true := by
    simp only [List.contains_iff_mem, occurrenceLocalHistory, List.mem_filter,
      atActor, decide_true, and_true]
  cases left : (occurrenceLocalHistory plan actor points).contains point with
  | false =>
    cases right : points.contains point with
    | false => rfl
    | true => cases left.symm.trans (same.mpr right)
  | true =>
    cases right : points.contains point with
    | false => cases right.symm.trans (same.mp left)
    | true => rfl

private theorem ordered_points_have_the_same_actor
    (plan : SourceOccurrenceCut source) {before after : ActionId}
    (ordered : precedes plan.expanded plan.path.bindings before after = true) :
    occurrenceActor? plan (.committed before) = occurrenceActor? plan after.entryPoint := by
  unfold precedes at ordered
  cases leftAt : action? plan.path.bindings before with
  | none => simp [leftAt] at ordered
  | some left =>
    cases rightAt : action? plan.path.bindings after with
    | none => simp [leftAt, rightAt] at ordered
    | some right =>
      have actors : left.actor = right.actor :=
        beq_iff_eq.mp (Bool.and_eq_true_iff.mp (by simpa only [leftAt, rightAt] using ordered)).1
      change (action? plan.path.bindings before).map Action.actor =
        (action? plan.path.bindings (pointAction after.entryPoint)).map Action.actor
      simp only [entry_point_action, leftAt, rightAt, Option.map_some, actors]

private theorem a_local_query_and_wire_evidence_are_source_ready
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint)
    (localChecked : occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true)
    (wire : occurrenceWireReady points point = true) :
    (occurrenceCut plan points).ready point = true := by
  have fields := Bool.and_eq_true_iff.mp localChecked
  have head := Bool.and_eq_true_iff.mp fields.1
  have member : point ∈ occurrenceLocalDomain plan actor := by simpa using head.1
  have present := (List.mem_filter.mp member).1
  have atActor : occurrenceActor? plan point = some actor :=
    of_decide_eq_true (List.mem_filter.mp member).2
  have pending : points.contains point = false := by
    simpa only [local_history_contains plan points point atActor, Bool.not_eq_true'] using head.2
  change (((sourceVisitPoints plan.path.bindings).contains point && !points.contains point) &&
    ((sourceVisitPoints plan.path.bindings).all (fun before =>
      !sourcePointDepends plan.expanded plan.path.bindings before point || points.contains before))) = true
  apply Bool.and_eq_true_iff.mpr
  refine ⟨Bool.and_eq_true_iff.mpr ⟨by simpa using present, by simp only [pending, Bool.not_false]⟩, ?_⟩
  apply List.all_eq_true.mpr
  intro before beforePresent
  by_cases edgeAt : sourcePointDepends plan.expanded plan.path.bindings before point = true
  · have edge := source_point_dependency_check_is_exact.mp edgeAt
    have done : points.contains before = true := by
      cases edge with
      | publish event | deliver event => exact wire
      | @ordered prior after ordered =>
        have priorActor : occurrenceActor? plan (.committed prior) = some actor :=
          (ordered_points_have_the_same_actor plan ordered).trans atActor
        have localMember : .committed prior ∈ occurrenceLocalDomain plan actor :=
          List.mem_filter.mpr ⟨beforePresent, by simpa using priorActor⟩
        have enabled := List.all_eq_true.mp fields.2 _ localMember
        have dependency : localDepends plan (.committed prior) after.entryPoint = true := edgeAt
        simpa only [dependency, Bool.not_true, Bool.false_or,
          local_history_contains plan points _ priorActor] using enabled
    simp only [edgeAt, Bool.not_true, Bool.false_or, done]
  · have noEdge : sourcePointDepends plan.expanded plan.path.bindings before point = false :=
      Bool.eq_false_iff.mpr edgeAt
    simp only [noEdge, Bool.not_false, Bool.true_or]

private theorem a_source_point_has_an_actor
    (plan : SourceOccurrenceCut source) (point : SourcePoint)
    (present : point ∈ sourceVisitPoints plan.path.bindings) :
    ∃ actor, occurrenceActor? plan point = some actor := by
  have member : pointAction point ∈ sourceVisitActions plan.path.bindings := by
    cases point with
    | accepted event => exact canonical_visit_contains_exactly_its_wire_acceptances.mp present
    | committed id => exact canonical_visit_contains_exactly_its_committed_actions.mp present
  have valid := canonical_visit_actions_are_exactly_decodable.mp member
  cases atAction : action? plan.path.bindings (pointAction point) with
  | none => simp only [atAction, Option.isSome_none, Bool.false_eq_true] at valid
  | some action => exact ⟨action.actor, by simp only [occurrenceActor?, atAction, Option.map_some]⟩

/-- Global dependency readiness is exactly the corresponding actor's local
projection plus evidence of transport acceptance. Neither direction assumes
a callable Rust future or consults another actor's mutable state. -/
theorem projected_occurrence_readiness_is_exact
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint) :
    (occurrenceCut plan points).ready point = true ↔
      ∃ actor, occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true ∧
        occurrenceWireReady points point = true := by
  constructor
  · intro checked
    have fields := Bool.and_eq_true_iff.mp checked
    have head := Bool.and_eq_true_iff.mp fields.1
    have present : point ∈ sourceVisitPoints plan.path.bindings := by simpa using head.1
    obtain ⟨actor, atActor⟩ := a_source_point_has_an_actor plan point present
    have pending : points.contains point = false := by
      simpa only [occurrenceCut, Bool.not_eq_true'] using head.2
    have ready : ∀ before, SourcePointEdge plan.expanded plan.path.bindings before point →
        points.contains before = true := by
      intro before edge
      have beforePresent := canonical_source_visit_is_dependency_closed present edge
      have enabled := List.all_eq_true.mp fields.2 before beforePresent
      simpa only [source_point_dependency_check_is_exact.mpr edge, Bool.not_true,
        Bool.false_or, occurrenceCut] using enabled
    refine ⟨actor, ?_, ?_⟩
    · apply Bool.and_eq_true_iff.mpr
      refine ⟨Bool.and_eq_true_iff.mpr ⟨?_, ?_⟩, List.all_eq_true.mpr ?_⟩
      · have member : point ∈ occurrenceLocalDomain plan actor :=
          List.mem_filter.mpr ⟨present, by simpa using atActor⟩
        simpa using member
      · simp only [local_history_contains plan points point atActor, pending, Bool.not_false]
      · intro before member
        have beforeAt : occurrenceActor? plan before = some actor :=
          of_decide_eq_true (List.mem_filter.mp member).2
        cases before with
        | accepted event => simp only [localDepends, Bool.not_false, Bool.true_or]
        | committed id =>
          by_cases depends : sourcePointDepends plan.expanded plan.path.bindings (.committed id) point = true
          · have done := ready _ (source_point_dependency_check_is_exact.mp depends)
            simp only [localDepends, depends, Bool.not_true, Bool.false_or,
              local_history_contains plan points _ beforeAt, done]
          · have noEdge := Bool.eq_false_iff.mpr depends
            simp only [localDepends, noEdge, Bool.not_false, Bool.true_or]
    · cases point with
      | accepted event => rfl
      | committed id =>
        cases id with
        | mk event side =>
          cases side with
          | send => exact ready _ (.publish event)
          | receive => exact ready _ (.deliver event)
          | «local» => rfl
  · rintro ⟨actor, localChecked, wire⟩
    exact a_local_query_and_wire_evidence_are_source_ready plan points point localChecked wire

/-- An unfinished occurrence cut has an operation enabled by one actor's own
projection and its acceptance evidence. This is a communication-model result;
application issuance and executor polling are separate premises. -/
theorem unfinished_source_occurrences_have_a_projected_operation
    (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (unfinished : (occurrenceCut plan points).Unfinished) :
    ∃ actor point next,
      occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true ∧
      occurrenceWireReady points point = true ∧
      SourceOccurrenceStep (occurrenceCut plan points) point next := by
  obtain ⟨point, present, pending, ready⟩ :=
    unfinished_canonical_source_visit_has_ready_point (source := plan.expanded) unfinished
  have checked : (occurrenceCut plan points).ready point = true := by
    apply Bool.and_eq_true_iff.mpr
    refine ⟨Bool.and_eq_true_iff.mpr ⟨by simpa using present, by simpa using pending⟩,
      List.all_eq_true.mpr ?_⟩
    intro before _
    change (!sourcePointDepends plan.expanded plan.path.bindings before point ||
      points.contains before) = true
    by_cases depends : sourcePointDepends plan.expanded plan.path.bindings before point = true
    · have done := ready before (source_point_dependency_check_is_exact.mp depends)
      simpa only [depends, Bool.not_true, Bool.false_or, occurrenceCut] using done
    · have noEdge := Bool.eq_false_iff.mpr depends
      simp only [noEdge, Bool.not_false, Bool.true_or]
  obtain ⟨actor, localChecked, wire⟩ :=
    (projected_occurrence_readiness_is_exact plan points point).mp checked
  obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition _ _ checked
  exact ⟨actor, point, next, localChecked, wire, step⟩

end Hibana
