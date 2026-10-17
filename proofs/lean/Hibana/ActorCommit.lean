import Hibana.ActorSourceProgress
import Hibana.ActorRouting
import Hibana.ViewGeneration
import Hibana.PublicOperationKernel

namespace Hibana
open EndpointDelegation

/-- Copied from a real cursor under its existing operation lease. This is
qualification data, not another runtime cursor or completion store. -/
structure ActorStepObservation where
  effIndex : Nat
  view : Nat
  side : Side
  done : Bool
  deriving Repr, DecidableEq

structure ActorViewObservation where
  role : Nat
  run : Nat
  nonce : Nat
  operation : PublicOperationPhase
  deriving Repr, DecidableEq

structure ActorProgressObservation where
  actor : Nat
  run : Nat
  counter : Nat
  liveViews : Nat
  views : List ActorViewObservation
  steps : List ActorStepObservation
  choices : List (Nat × Option RouteArm) := []
  deriving Repr, DecidableEq

def ActorProgressObservation.progress (snapshot : ActorProgressObservation) : List Bool :=
  snapshot.steps.map ActorStepObservation.done

def ActorViewObservation.header (view : ActorViewObservation) : Nat × Nat × Nat :=
  (view.role, view.run, view.nonce)

def ActorProgressObservation.viewHeaders (snapshot : ActorProgressObservation) : List (Nat × Nat × Nat) :=
  snapshot.views.map ActorViewObservation.header

/-- Copy the existing done bits and committed choices. Commit checks bind both
choice histories and their publication to the selected source visit; reentry
first derives the fresh base for that same check. -/
def ActorProgressObservation.doneState (snapshot : ActorProgressObservation) : CommitState := {
  done := fun event => snapshot.progress[event]?.getD false
  selected := fun scope =>
    (snapshot.choices.find? fun entry => entry.1 == scope).bind Prod.snd
}

def ActorProgressObservation.choicesMatch
    (snapshot : ActorProgressObservation) (image : RustDescriptorImage) : Bool :=
  snapshot.choices.length == image.routeScopeCount &&
    decide (snapshot.choices.map Prod.fst).Nodup &&
    (List.range image.routeScopeCount).all fun slot =>
      match image.decodeRouteScope? slot with
      | none => false
      | some scope => snapshot.choices.any fun entry => entry.1 == scope

def observedChoicesAgree
    (source : CanonicalProgramSource) (path : NormalizedPath)
    (snapshot : ActorProgressObservation) : Bool :=
  snapshot.choices.all fun (scope, selected) =>
    match selected with
    | none => true
    | some arm => sourcePathArm? source.markers path scope == some arm

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

def ActorProgressObservation.metadataMatches
    (snapshot : ActorProgressObservation) (image : RustDescriptorImage)
    (rows : List DecodedActorRow) : Bool :=
  snapshot.actor == image.role && snapshot.choicesMatch image &&
    snapshot.steps.length == image.eventCount &&
    snapshot.steps.length == rows.length &&
    (snapshot.steps.zip rows).all fun (step, row) =>
      image.eventEffIndex? row.eventId == some step.effIndex &&
      step.view == row.view && decide (step.side = row.actionId.side)

def ActorProgressObservation.valid (snapshot : ActorProgressObservation) : Bool :=
  0 < snapshot.run && snapshot.run < 2 ^ 32 &&
  snapshot.counter % 2 == 1 && snapshot.counter < maxViewGeneration &&
  0 < snapshot.liveViews && snapshot.liveViews ≤ 256 &&
  decide (snapshot.views.map ActorViewObservation.role).Nodup &&
  snapshot.views.all fun view =>
    view.role < 256 && view.nonce < maxViewGeneration && view.nonce ≤ snapshot.counter + 1 &&
      view.run == (if view.nonce % 2 = 1 then snapshot.run else 0)

def observedActorView? (snapshot : ActorProgressObservation) (role : Nat) :
    Option ActorViewObservation :=
  resolveUnique? (snapshot.views.filter fun view => view.role == role)

/-- Existing affine header issuance, not a second owner ledger. Issuance is
necessary but does not by itself exclude a pending owned-payload borrow. -/
def ActorProgressObservation.viewIssued (snapshot : ActorProgressObservation) (role : Nat) : Bool :=
  match observedActorView? snapshot role with
  | none => false
  | some view => view.run == snapshot.run && view.nonce % 2 == 1

theorem observed_issued_view_has_current_header
    {snapshot : ActorProgressObservation} {role : Nat}
    (issued : snapshot.viewIssued role = true) :
    ∃ view, observedActorView? snapshot role = some view ∧
      view.run = snapshot.run ∧ view.nonce % 2 = 1 := by
  unfold ActorProgressObservation.viewIssued at issued
  cases found : observedActorView? snapshot role with
  | none => simp [found] at issued
  | some view =>
      have parts : view.run = snapshot.run ∧ view.nonce % 2 = 1 := by
        simpa only [found, Bool.and_eq_true, beq_iff_eq] using issued
      exact ⟨view, rfl, parts⟩

def replaceObservedActorView
    (snapshot : ActorProgressObservation) (role run nonce : Nat) : List ActorViewObservation :=
  snapshot.views.map fun view =>
    if view.role == role then { view with run, nonce } else view

/-- Check the same retirement/issuance arithmetic as the raw carrier. Header
counts are actor-local observations, not globally usable ownership counts.
Operation leases may change without changing these headers, including previews.
Their payload-borrow condition is checked at reentry admission separately. -/
def observedViewCommitMatches
    (before after : ActorProgressObservation) (payload : EndpointDelegation.Payload)
    (side : Side) : Bool :=
  match payload with
  | .wire _ =>
      decide (after.viewHeaders = before.viewHeaders) && after.counter == before.counter &&
        after.liveViews == before.liveViews
  | .endpoint key =>
      key.component == 0 &&
      match observedActorView? before key.role with
      | none => false
      | some view =>
          match side with
          | .local => false
          | .send =>
              view.run == before.run && view.nonce % 2 == 1 &&
              decide (after.viewHeaders = (replaceObservedActorView before key.role 0
                (retireViewGeneration view.nonce)).map ActorViewObservation.header) &&
              after.counter == before.counter && after.liveViews + 1 == before.liveViews
          | .receive =>
              view.run == 0 && view.nonce % 2 == 0 &&
              match nextViewGeneration? before.counter with
              | none => false
              | some nonce =>
                  decide (after.viewHeaders = (replaceObservedActorView before key.role before.run nonce).map
                    ActorViewObservation.header) &&
                  after.counter == nonce && after.liveViews == before.liveViews + 1

def observedSourcePrefixCheck
    (data : DecodedActorOrder) (path : NormalizedPath)
    (snapshot : ActorProgressObservation) (event : Nat) : Bool :=
  (data.sourcePredecessors path event).all fun middle =>
    if snapshot.doneState.done middle then
      (data.sourcePredecessors path middle).all snapshot.doneState.done
    else true

theorem observed_source_prefix_check_is_sound
    {data : DecodedActorOrder} {path : NormalizedPath}
    {snapshot : ActorProgressObservation} {event : Nat}
    (checked : observedSourcePrefixCheck data path snapshot event = true) :
    snapshot.doneState.PrefixClosed (data.sourcePredecessors path)
      (data.sourcePredecessors path event) := by
  intro middle member completed before beforeMember
  have ready := List.all_eq_true.mp checked middle member
  rw [if_pos completed] at ready
  exact List.all_eq_true.mp ready before beforeMember

/-- One successful ordinary/owned step, observed without overlapping another
commit in that actor. A repeated visit needs its actual prepared base first;
this checker does not silently erase old completion or route observations. -/
structure ActorCommitObservation where
  pathIndex : Nat
  event : Nat
  view : Nat
  before : ActorProgressObservation
  after : ActorProgressObservation

def ActorCommitObservation.check
    (sample : ActorCommitObservation) (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  sample.before.viewIssued sample.view && (image.actorOrderCheck choreo &&
  match image.decodeActorOrder? choreo with
  | none => false
  | some data =>
      match data.normalized.paths[sample.pathIndex]?, data.rows[sample.event]? with
      | some path, some row =>
          ((sample.before.metadataMatches image data.rows &&
            sample.after.metadataMatches image data.rows &&
            sample.before.valid && sample.after.valid &&
            sample.after.actor == sample.before.actor && sample.after.run == sample.before.run) &&
          (observedChoicesAgree (canonicalProgramSource choreo) path sample.before &&
            (observedChoicesAgree (canonicalProgramSource choreo) path sample.after &&
            decide ((image.eventEffIndex? sample.event).bind (fun eff =>
              publishedObservedChoices? (canonicalProgramSource choreo) eff
                sample.before.choices) = some sample.after.choices)))) &&
          ((row.eventId == sample.event &&
              (row.view == sample.view && decide (row ∈ data.activeRows path))) &&
          ((sample.before.progress[sample.event]? == some false &&
            decide (sample.after.progress = sample.before.progress.set sample.event true)) &&
          (observedSourcePrefixCheck data path sample.before sample.event &&
          (match data.runtimePredecessors? path sample.event,
              path.bindings.find? (fun binding => binding.1 == row.actionId.event) with
          | some parents, some (_, binding) =>
              parents.all sample.before.doneState.done &&
              observedViewCommitMatches sample.before sample.after binding.message.payload
                row.actionId.side
          | _, _ => false))))
      | _, _ => false)

theorem accepted_actor_commit_uses_issued_view
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) :
    ∃ view, observedActorView? sample.before sample.view = some view ∧
      view.run = sample.before.run ∧ view.nonce % 2 = 1 :=
  observed_issued_view_has_current_header (Bool.and_eq_true_iff.mp accepted).1

private theorem one_observed_step_is_exact
    {before after : List Bool} {event : Nat}
    (unfinished : before[event]? = some false)
    (updated : after = before.set event true) :
    after[event]?.getD false = true ∧
      ∀ other, other ≠ event → after[other]?.getD false = before[other]?.getD false := by
  subst after
  induction before generalizing event with
  | nil => simp at unfinished
  | cons head tail ih =>
      cases event with
      | zero =>
          refine ⟨rfl, ?_⟩
          intro other different
          cases other with
          | zero => exact False.elim (different rfl)
          | succ other => rfl
      | succ event =>
          have remaining : tail[event]? = some false := unfinished
          obtain ⟨completed, unchanged⟩ := ih remaining
          refine ⟨completed, ?_⟩
          intro other different
          cases other with
          | zero => rfl
          | succ other =>
              exact unchanged other (fun same => different (congrArg Nat.succ same))

def ActorCommitObservation.SourceOrdered
    (sample : ActorCommitObservation) (image : RustDescriptorImage) (choreo : Choreo) : Prop :=
  image.actorRowsCheck choreo = true ∧
  ∃ data path row binding,
    image.decodeActorOrder? choreo = some data ∧
    data.normalized.paths[sample.pathIndex]? = some path ∧
    data.rows[sample.event]? = some row ∧ row.eventId = sample.event ∧
    row.view = sample.view ∧ row ∈ data.activeRows path ∧
    path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some binding ∧
    (data.sourcePredecessors path sample.event).all sample.before.doneState.done = true ∧
    sample.after.doneState.done sample.event = true ∧
    sample.after.doneState.PrefixClosed (data.sourcePredecessors path)
      (sample.event :: data.sourcePredecessors path sample.event) ∧
    (∀ other, other ≠ sample.event →
      sample.after.doneState.done other = sample.before.doneState.done other) ∧
    observedViewCommitMatches sample.before sample.after binding.2.message.payload
      row.actionId.side = true ∧
    observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true ∧
    observedChoicesAgree (canonicalProgramSource choreo) path sample.after = true ∧
    (image.eventEffIndex? sample.event).bind (fun eff =>
      publishedObservedChoices? (canonicalProgramSource choreo) eff sample.before.choices) =
        some sample.after.choices

/-- Accepted observations connect the real actor-local done bits to the
source order of the checked image. View publication and one exact progress
step are checked together. This is translation validation for observations;
it is not source extraction or a distributed ownership/liveness theorem. -/
theorem accepted_actor_commit_matches_source_progress
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) : sample.SourceOrdered image choreo := by
  have accepted := (Bool.and_eq_true_iff.mp accepted).2
  have actorAccepted : image.actorOrderCheck choreo = true :=
    (Bool.and_eq_true_iff.mp accepted).1
  obtain ⟨rowsAccepted, data, decoded, readiness⟩ :=
    accepted_actor_order_binds_source_readiness actorAccepted
  have checked := (Bool.and_eq_true_iff.mp accepted).2
  simp only [decoded] at checked
  cases pathAt : data.normalized.paths[sample.pathIndex]? with
  | none => simp [pathAt] at checked
  | some path =>
      cases rowAt : data.rows[sample.event]? with
      | none => simp [pathAt, rowAt] at checked
      | some row =>
          cases parentsAt : data.runtimePredecessors? path sample.event with
          | none => simp [pathAt, rowAt, parentsAt] at checked
          | some parents =>
              cases bindingAt : path.bindings.find?
                  (fun entry => entry.1 == row.actionId.event) with
              | none => simp [pathAt, rowAt, parentsAt, bindingAt] at checked
              | some binding =>
                  simp only [pathAt, rowAt, parentsAt, bindingAt, Bool.and_eq_true] at checked
                  obtain ⟨⟨_, beforeAgrees, afterAgrees, publication⟩, ⟨⟨eventSame, viewSame, active⟩,
                    ⟨⟨unfinished, exactStep⟩, closed, ready, viewCommit⟩⟩⟩ := checked
                  have same : row.eventId = sample.event := beq_iff_eq.mp eventSame
                  have member : row ∈ data.activeRows path := of_decide_eq_true active
                  have sourceClosed := observed_source_prefix_check_is_sound closed
                  have pathMember := List.mem_of_getElem? pathAt
                  have sourceReady := readiness path pathMember row member
                  rw [same] at sourceReady
                  obtain ⟨actual, actualAt, _, equivalent⟩ := sourceReady
                  have sameParents : actual = parents := Option.some.inj (actualAt.symm.trans parentsAt)
                  subst actual
                  have sourceDone := (equivalent sample.before.doneState sourceClosed).mp ready
                  have step := one_observed_step_is_exact
                    (beq_iff_eq.mp unfinished) (of_decide_eq_true exactStep)
                  have completed : sample.after.doneState.done sample.event = true := step.1
                  have unchanged : ∀ other, other ≠ sample.event →
                      sample.after.doneState.done other = sample.before.doneState.done other := step.2
                  have beforeFalse : sample.before.doneState.done sample.event = false := by
                    change sample.before.progress[sample.event]?.getD false = false
                    rw [beq_iff_eq.mp unfinished]
                    rfl
                  have includingCurrent : sample.before.doneState.PrefixClosed
                      (data.sourcePredecessors path)
                      (sample.event :: data.sourcePredecessors path sample.event) := by
                    intro target inCut targetDone before beforeMember
                    rcases List.mem_cons.mp inCut with current | prior
                    · subst target
                      rw [beforeFalse] at targetDone
                      contradiction
                    · exact sourceClosed target prior targetDone before beforeMember
                  have nextPrefix := actor_commit_delta_preserves_source_prefix
                    includingCurrent sourceDone completed unchanged
                  exact ⟨rowsAccepted, data, path, row, binding, decoded, pathAt, rowAt, same,
                    beq_iff_eq.mp viewSame, member, bindingAt, sourceDone, completed,
                    nextPrefix, unchanged, viewCommit, beforeAgrees, afterAgrees,
                    of_decide_eq_true publication⟩

end Hibana
