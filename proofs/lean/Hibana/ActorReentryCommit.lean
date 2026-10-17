import Hibana.ActorCommit
import Hibana.ActorReentry

namespace Hibana
open EndpointDelegation

/-- An atomic body reentry and its successful operation. These are copied
observations; the prepared base is derived, never stored in the Rust runtime. -/
structure ActorReentryCommitObservation where
  previousPath : Nat
  nextPath : Nat
  scope : Nat
  event : Nat
  view : Nat
  before : ActorProgressObservation
  after : ActorProgressObservation

def observedChoicesAgree
    (source : CanonicalProgramSource) (path : NormalizedPath)
    (snapshot : ActorProgressObservation) : Bool :=
  snapshot.choices.all fun (scope, selected) =>
    match selected with
    | none => true
    | some arm => sourcePathArm? source.markers path scope == some arm

def resetObservedActor
    (snapshot : ActorProgressObservation) (roll : DecodedRollRow)
    (choices : List (Nat × Option RouteArm)) : ActorProgressObservation :=
  { snapshot with
    choices
    steps := snapshot.steps.zipIdx.map fun (step, event) =>
      { step with done := if roll.eventStart ≤ event &&
          event < roll.eventStart + roll.eventLength then false else step.done } }

def resetObservedChoices?
    (source : CanonicalProgramSource) (scope : Nat)
    (choices : List (Nat × Option RouteArm)) : Option (List (Nat × Option RouteArm)) :=
  choices.mapM fun (route, arm) => do
    let reset ← sourceRollResetsRoute? source scope route
    pure (route, if reset then none else arm)

def publishedObservedChoices?
    (source : CanonicalProgramSource) (eff : Nat)
    (choices : List (Nat × Option RouteArm)) : Option (List (Nat × Option RouteArm)) := do
  let memberships ← canonicalEventRoutePath? source eff
  choices.mapM fun (scope, previous) =>
    match memberships.find? (fun member => member.1 == scope) with
    | none => pure (scope, previous)
    | some (_, arm) => do
        let chosen ← match arm with
          | 0 => some RouteArm.left
          | 1 => some RouteArm.right
          | _ => none
        if previous.isSome && previous != some chosen then none
        else pure (scope, some chosen)

def ActorReentryCommitObservation.prepared?
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage)
    (choreo : Choreo) : Option (DecodedRollRow × ActorCommitObservation) := do
  let data ← image.decodeActorOrder? choreo
  let previous ← data.normalized.paths[sample.previousPath]?
  let next ← data.normalized.paths[sample.nextPath]?
  let rolls ← image.decodeRollRows?
  let roll ← resolveUnique? (rolls.filter fun row => row.scope == sample.scope)
  let source := canonicalProgramSource choreo
  let choices ← resetObservedChoices? source sample.scope sample.before.choices
  let base := resetObservedActor sample.before roll choices
  let eff ← image.eventEffIndex? sample.event
  let published ← publishedObservedChoices? source eff choices
  if image.actorOrderCheck choreo && image.actorRollCheck choreo &&
      image.actorRouteCheck choreo && sample.before.metadataMatches image data.rows &&
      sample.before.valid &&
      roll.eventStart ≤ sample.event && sample.event < roll.eventStart + roll.eventLength &&
      observedChoicesAgree source previous sample.before &&
      observedChoicesAgree source next base &&
      (data.activeRows previous).all (fun row =>
        if roll.eventStart ≤ row.eventId && row.eventId < roll.eventStart + roll.eventLength
        then sample.before.doneState.done row.eventId else true) &&
      observedSourcePrefixCheck data next sample.before sample.event &&
      decide (sample.after.choices = published) then
    some (roll, {
      pathIndex := sample.nextPath, event := sample.event, view := sample.view,
      before := base, after := sample.after })
  else none

def ActorReentryCommitObservation.check
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage)
    (choreo : Choreo) : Bool :=
  match sample.prepared? image choreo with
  | none => false
  | some (_, step) => step.check image choreo

private theorem reset_observed_actor_done
    (snapshot : ActorProgressObservation) (roll : DecodedRollRow)
    (choices : List (Nat × Option RouteArm)) (event : Nat) :
    (resetObservedActor snapshot roll choices).doneState.done event =
      if roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength
      then false else snapshot.doneState.done event := by
  simp only [resetObservedActor, ActorProgressObservation.doneState,
    ActorProgressObservation.progress, List.getElem?_map, List.getElem?_zipIdx, Nat.zero_add]
  cases snapshot.steps[event]? <;> simp

private theorem prepared_reentry_facts
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    image.actorRollCheck choreo = true ∧
      ∃ rows choices, image.decodeRollRows? = some rows ∧ roll ∈ rows ∧
        step.before = resetObservedActor sample.before roll choices := by
  unfold ActorReentryCommitObservation.prepared? at prepared
  change (image.decodeActorOrder? choreo).bind _ = some (roll, step) at prepared
  obtain ⟨data, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change data.normalized.paths[sample.previousPath]?.bind _ = some (roll, step) at prepared
  obtain ⟨previous, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change data.normalized.paths[sample.nextPath]?.bind _ = some (roll, step) at prepared
  obtain ⟨nextPath, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change image.decodeRollRows?.bind _ = some (roll, step) at prepared
  obtain ⟨rows, rowsAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change (resolveUnique? (rows.filter fun row => row.scope == sample.scope)).bind _ =
    some (roll, step) at prepared
  obtain ⟨chosen, chosenAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change (resetObservedChoices? (canonicalProgramSource choreo) sample.scope
    sample.before.choices).bind _ = some (roll, step) at prepared
  obtain ⟨choices, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change (image.eventEffIndex? sample.event).bind _ = some (roll, step) at prepared
  obtain ⟨eff, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  change (publishedObservedChoices? (canonicalProgramSource choreo) eff choices).bind _ =
    some (roll, step) at prepared
  obtain ⟨published, _, allowed⟩ := Option.bind_eq_some_iff.mp prepared
  split at allowed
  next guarded =>
    have same := Option.some.inj allowed
    have sameRoll := congrArg Prod.fst same
    have sameStep := congrArg Prod.snd same
    dsimp only at sameRoll sameStep
    subst roll
    subst step
    have member : chosen ∈ rows := by
      have singleton := resolveUnique?_eq_some_iff.mp chosenAt
      have filtered : chosen ∈ rows.filter (fun row => row.scope == sample.scope) := by
        rw [singleton]
        simp
      exact (List.mem_filter.mp filtered).1
    simp only [Bool.and_eq_true] at guarded
    obtain ⟨⟨⟨⟨⟨⟨⟨⟨⟨⟨⟨_, ranges⟩, _⟩, _⟩, _⟩, _⟩, _⟩, _⟩, _⟩, _⟩, _⟩, _⟩ := guarded
    exact ⟨ranges, rows, choices, rowsAt, member, rfl⟩
  next rejected => simp at allowed

def ActorReentryCommitObservation.SourceOrdered
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage) (choreo : Choreo) : Prop :=
    ∃ roll step rows,
      sample.prepared? image choreo = some (roll, step) ∧
      choreo.canonicalActorRollRows? image.role = some rows ∧ roll ∈ rows ∧
      (∀ event, step.before.doneState.done event =
        if roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength
        then false else sample.before.doneState.done event) ∧
      step.SourceOrdered image choreo

/-- A checked atomic body reentry clears exactly a source-validated range,
preserves all outside progress, and commits a source-ordered step with the
matching local view publication. Choice reset/publication is checked by the
same prepared observation. This does not yet establish distributed liveness. -/
theorem accepted_body_reentry_restores_source_progress
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) : sample.SourceOrdered image choreo := by
  unfold ActorReentryCommitObservation.SourceOrdered
  unfold ActorReentryCommitObservation.check at accepted
  cases found : sample.prepared? image choreo with
  | none => simp [found] at accepted
  | some entry =>
      rcases entry with ⟨roll, step⟩
      have checked : step.check image choreo = true := by simpa [found] using accepted
      obtain ⟨ranges, actual, choices, actualAt, member, base⟩ := prepared_reentry_facts found
      obtain ⟨_, _, rows, rowsAt, decoded⟩ := accepted_actor_rolls_bind_source_ranges ranges
      have same : actual = rows := Option.some.inj (actualAt.symm.trans decoded)
      refine ⟨roll, step, rows, rfl, rowsAt, by simpa only [← same] using member, ?_,
        accepted_actor_commit_matches_source_progress checked⟩
      intro event
      rw [base]
      exact reset_observed_actor_done sample.before roll choices event

end Hibana
