import Hibana.SourceResourceAvailability
import Hibana.ActorReentryCommit

namespace Hibana
open EndpointDelegation

/-- Read the existing header bytes, without interpreting an operation lease
as ownership. A tag names a candidate value; the header/transfer-history theorem is
needed to establish whether that value is in application code or a future. -/
def observedResourceHeader? (snapshot : ActorProgressObservation) (role : Nat) :
    Option (Nat × Nat × Nat) :=
  resolveUnique? (snapshot.viewHeaders.filter fun header => header.1 == role)

def observedResourceTag? (snapshot : ActorProgressObservation) (role : Nat) :
    Option (Nat × Nat × Nat × Nat) := do
  let header ← observedResourceHeader? snapshot role
  if header.2.1 == snapshot.run && header.2.2 % 2 == 1 then
    some (snapshot.actor, header)
  else none

private theorem unique_map_commutes {α β : Type} (f : α → β) (entries : List α) :
    resolveUnique? (entries.map f) = (resolveUnique? entries).map f := by
  cases entries with
  | nil => rfl
  | cons head tail => cases tail <;> rfl

private theorem header_filter_is_the_view_filter
    (views : List ActorViewObservation) (role : Nat) :
    (views.map ActorViewObservation.header).filter (fun header => header.1 == role) =
      (views.filter (fun view => view.role == role)).map ActorViewObservation.header := by
  induction views with
  | nil => rfl
  | cons head tail ih =>
      cases guard : head.role == role <;>
        simp only [List.map_cons, List.filter_cons, ActorViewObservation.header,
          guard, Bool.false_eq_true, ite_false, ite_true, ih, List.map_cons]

private theorem observed_header_is_the_view_header
    (snapshot : ActorProgressObservation) (role : Nat) :
    observedResourceHeader? snapshot role =
      (observedActorView? snapshot role).map ActorViewObservation.header := by
  rw [observedResourceHeader?, ActorProgressObservation.viewHeaders,
    header_filter_is_the_view_filter, unique_map_commutes]
  rfl

theorem an_issued_resource_header_has_its_exact_observed_tag
    {snapshot : ActorProgressObservation} {role : Nat}
    (issued : snapshot.viewIssued role = true) :
    ∃ view, observedActorView? snapshot role = some view ∧
      view.run = snapshot.run ∧ view.nonce % 2 = 1 ∧
      observedResourceTag? snapshot role = some (snapshot.actor, view.header) := by
  obtain ⟨view, found, runAt, live⟩ := observed_issued_view_has_current_header issued
  have runBool : (view.run == snapshot.run) = true := decide_eq_true runAt
  have liveBool : (view.nonce % 2 == 1) = true := decide_eq_true live
  refine ⟨view, found, runAt, live, ?_⟩
  simp only [observedResourceTag?, observed_header_is_the_view_header, found,
    Option.map_some, Option.bind_eq_bind, Option.bind_some,
    ActorViewObservation.header, runBool, liveBool, Bool.true_and, ite_true]

private def updateResourceHeader (role run nonce : Nat) (header : Nat × Nat × Nat) :
    Nat × Nat × Nat :=
  if header.1 == role then (header.1, run, nonce) else header

private theorem updated_header_preserves_role (role run nonce : Nat)
    (header : Nat × Nat × Nat) : (updateResourceHeader role run nonce header).1 = header.1 := by
  unfold updateResourceHeader
  split <;> rfl

private theorem replacement_headers_are_updated_headers
    (snapshot : ActorProgressObservation) (role run nonce : Nat) :
    (replaceObservedActorView snapshot role run nonce).map ActorViewObservation.header =
      snapshot.viewHeaders.map (updateResourceHeader role run nonce) := by
  simp only [replaceObservedActorView, ActorProgressObservation.viewHeaders, List.map_map]
  apply List.map_congr_left
  intro view _
  cases guard : view.role == role <;>
    simp only [Function.comp_apply, guard, Bool.false_eq_true, ite_false, ite_true,
      ActorViewObservation.header, updateResourceHeader]

private theorem a_role_preserving_map_commutes_with_header_filter
    (headers : List (Nat × Nat × Nat)) (f : (Nat × Nat × Nat) → (Nat × Nat × Nat))
    (preserves : ∀ header, (f header).1 = header.1) (query : Nat) :
    (headers.map f).filter (fun header => header.1 == query) =
      (headers.filter (fun header => header.1 == query)).map f := by
  induction headers with
  | nil => rfl
  | cons head tail ih =>
      cases guard : head.1 == query <;>
        simp only [List.map_cons, List.filter_cons, preserves, guard,
          Bool.false_eq_true, ite_false, ite_true, ih, List.map_cons]

private theorem updated_header_lookup
    {before after : ActorProgressObservation} {role run nonce : Nat}
    (updated : after.viewHeaders =
      (replaceObservedActorView before role run nonce).map ActorViewObservation.header)
    (query : Nat) :
    observedResourceHeader? after query =
      (observedResourceHeader? before query).map (updateResourceHeader role run nonce) := by
  unfold observedResourceHeader?
  rw [updated, replacement_headers_are_updated_headers,
    a_role_preserving_map_commutes_with_header_filter _ _
      (updated_header_preserves_role role run nonce), unique_map_commutes]

private theorem observed_header_lookup_retains_its_role
    {snapshot : ActorProgressObservation} {query : Nat} {header : Nat × Nat × Nat}
    (found : observedResourceHeader? snapshot query = some header) : header.1 = query := by
  have singleton := resolveUnique?_eq_some_iff.mp found
  have member : header ∈ snapshot.viewHeaders.filter (fun header => header.1 == query) := by
    rw [singleton]
    exact List.mem_singleton_self _
  exact beq_iff_eq.mp (List.mem_filter.mp member).2

private theorem a_changed_header_preserves_other_resource_tags
    {before after : ActorProgressObservation} {role run nonce query : Nat}
    (actorAt : after.actor = before.actor) (runAt : after.run = before.run)
    (updated : after.viewHeaders =
      (replaceObservedActorView before role run nonce).map ActorViewObservation.header)
    (different : query ≠ role) :
    observedResourceTag? after query = observedResourceTag? before query := by
  have lookup := updated_header_lookup updated query
  cases found : observedResourceHeader? before query with
  | none => simp only [observedResourceTag?, lookup, found, Option.map_none,
      Option.bind_eq_bind, Option.bind_none]
  | some header =>
      have headerRole := observed_header_lookup_retains_its_role found
      have guard : (header.1 == role) = false := decide_eq_false (headerRole ▸ different)
      simp only [observedResourceTag?, lookup, found, Option.map_some,
        updateResourceHeader, guard, Bool.false_eq_true, ite_false, actorAt, runAt]

/-- Source and wire validation authorize precisely this resource's header
change. The decoded row, payload kind and after-view are the same facts used
by the actual owned operation checker. -/
theorem checked_owned_commit_has_its_exact_resource_header_change
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) :
    sample.after.actor = sample.before.actor ∧ sample.after.run = sample.before.run ∧
      observedViewCommitMatches sample.before sample.after (.endpoint facts.resource) facts.side = true := by
  obtain ⟨_, data, path, row, entry, dataAt, pathAt, rowAt, _, _, _, entryAt,
    _, _, _, _, headerChange, _⟩ := (accepted_owned_commit_binds_source_view_and_wire checked).1
  obtain ⟨actualData, actualPath, actualRow, actualEntry, actualDataAt, actualPathAt,
    actualRowAt, actualEntryAt, payload, side, _, _⟩ :=
    decoded_owned_facts_retain_the_source_row_and_view decoded
  have dataSame := Option.some.inj (actualDataAt.symm.trans dataAt)
  subst actualData
  have pathSame := Option.some.inj (actualPathAt.symm.trans pathAt)
  subst actualPath
  have rowSame := Option.some.inj (actualRowAt.symm.trans rowAt)
  subst actualRow
  have entrySame := Option.some.inj (actualEntryAt.symm.trans entryAt)
  subst actualEntry
  have stable := checked_actor_commit_preserves_actor_and_run (Bool.and_eq_true_iff.mp checked).1
  exact ⟨stable.1, stable.2, by simpa only [payload, side] using headerChange⟩

private theorem a_replaced_header_is_exact_at_its_role
    {before after : ActorProgressObservation} {role run nonce : Nat}
    {header : Nat × Nat × Nat}
    (updated : after.viewHeaders =
      (replaceObservedActorView before role run nonce).map ActorViewObservation.header)
    (found : observedResourceHeader? before role = some header) :
    observedResourceHeader? after role = some (role, run, nonce) := by
  have identity := observed_header_lookup_retains_its_role found
  have guard : (header.1 == role) = true := decide_eq_true identity
  rw [updated_header_lookup updated, found]
  simp only [Option.map_some, updateResourceHeader, guard, ite_true]
  rw [identity]

private theorem a_live_header_has_its_exact_tag
    {snapshot : ActorProgressObservation} {role : Nat} {header : Nat × Nat × Nat}
    (found : observedResourceHeader? snapshot role = some header)
    (runAt : header.2.1 = snapshot.run) (odd : header.2.2 % 2 = 1) :
    observedResourceTag? snapshot role = some (snapshot.actor, header) := by
  have runBool : (header.2.1 == snapshot.run) = true := decide_eq_true runAt
  have oddBool : (header.2.2 % 2 == 1) = true := decide_eq_true odd
  simp only [observedResourceTag?, found, Option.bind_eq_bind, Option.bind_some,
    runBool, oddBool, Bool.true_and, ite_true]

private theorem an_even_header_has_no_live_tag
    {snapshot : ActorProgressObservation} {role : Nat} {header : Nat × Nat × Nat}
    (found : observedResourceHeader? snapshot role = some header)
    (even : header.2.2 % 2 = 0) : observedResourceTag? snapshot role = none := by
  simp only [observedResourceTag?, found, Option.bind_eq_bind, Option.bind_some,
    even, Nat.reduceBEq, Bool.and_false, Bool.false_eq_true, ite_false]

/-- The sender's local commit invalidates this exact tag, after transport
acceptance. A receiver can progress while the old header is still live inside
the suspended send future. That accepted witness is not operation authority. -/
theorem checked_owned_send_retires_its_exact_observed_tag
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (sending : facts.side = .send) :
    ∃ view, observedActorView? sample.before facts.resource.role = some view ∧
      observedResourceTag? sample.before facts.resource.role = some (sample.before.actor, view.header) ∧
      observedResourceTag? sample.after facts.resource.role = none := by
  have delta := (checked_owned_commit_has_its_exact_resource_header_change checked decoded).2.2
  cases found : observedActorView? sample.before facts.resource.role with
  | none => simp [observedViewCommitMatches, found] at delta
  | some view =>
      have fields : facts.resource.component = 0 ∧ view.run = sample.before.run ∧
          view.nonce % 2 = 1 ∧
          sample.after.viewHeaders =
            (replaceObservedActorView sample.before facts.resource.role 0
              (retireViewGeneration view.nonce)).map ActorViewObservation.header ∧
          sample.after.counter = sample.before.counter ∧
          sample.after.liveViews + 1 = sample.before.liveViews := by
        simpa only [observedViewCommitMatches, sending, found, Bool.and_eq_true,
          beq_iff_eq, decide_eq_true_eq, and_assoc] using delta
      have beforeHeader : observedResourceHeader? sample.before facts.resource.role = some view.header := by
        rw [observed_header_is_the_view_header, found]
        rfl
      have afterHeader := a_replaced_header_is_exact_at_its_role fields.2.2.2.1 beforeHeader
      have retiredEven : retireViewGeneration view.nonce % 2 = 0 := by
        rw [retireViewGeneration, if_pos fields.2.2.1]
        omega
      exact ⟨view, rfl, a_live_header_has_its_exact_tag beforeHeader fields.2.1 fields.2.2.1,
        an_even_header_has_no_live_tag afterHeader retiredEven⟩

/-- Receipt publishes the receiver's actual checked header. It does not reuse
the sender tag, or require the sender to drop its completed future first. -/
theorem checked_owned_receive_publishes_its_exact_observed_tag
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (receiving : facts.side = .receive) :
    observedResourceTag? sample.before facts.resource.role = none ∧
      observedResourceTag? sample.after facts.resource.role =
        some (sample.after.actor, facts.afterView.header) := by
  have delta := checked_owned_commit_has_its_exact_resource_header_change checked decoded
  obtain ⟨_, _, _, _, _, _, _, _, _, _, _, afterViewAt⟩ :=
    decoded_owned_facts_retain_the_source_row_and_view decoded
  cases found : observedActorView? sample.before facts.resource.role with
  | none => simp [observedViewCommitMatches, found] at delta
  | some view =>
      have parts : facts.resource.component = 0 ∧ view.run = 0 ∧ view.nonce % 2 = 0 ∧
          (match nextViewGeneration? sample.before.counter with
          | none => false
          | some nonce =>
              decide (sample.after.viewHeaders =
                (replaceObservedActorView sample.before facts.resource.role sample.before.run nonce).map
                  ActorViewObservation.header) && sample.after.counter == nonce &&
                sample.after.liveViews == sample.before.liveViews + 1) = true := by
        simpa only [observedViewCommitMatches, receiving, found, Bool.and_eq_true,
          beq_iff_eq, and_assoc] using delta.2.2
      cases freshAt : nextViewGeneration? sample.before.counter with
      | none => simp [freshAt] at parts
      | some nonce =>
          have updated : sample.after.viewHeaders =
              (replaceObservedActorView sample.before facts.resource.role sample.before.run nonce).map
                ActorViewObservation.header := by
            have checked' := parts.2.2.2
            simp only [freshAt, Bool.and_eq_true, decide_eq_true_eq, beq_iff_eq] at checked'
            exact checked'.1.1
          have beforeHeader : observedResourceHeader? sample.before facts.resource.role = some view.header := by
            rw [observed_header_is_the_view_header, found]
            rfl
          have afterHeader := a_replaced_header_is_exact_at_its_role updated beforeHeader
          have actualHeader : observedResourceHeader? sample.after facts.resource.role =
              some facts.afterView.header := by
            rw [observed_header_is_the_view_header, afterViewAt]
            rfl
          have same := Option.some.inj (actualHeader.symm.trans afterHeader)
          refine ⟨an_even_header_has_no_live_tag beforeHeader parts.2.2.1, ?_⟩
          apply a_live_header_has_its_exact_tag actualHeader
          · simpa only [same] using delta.2.1.symm
          · simpa only [same] using fresh_view_generation_is_live freshAt

private theorem an_owned_commit_updates_only_its_resource_header
    {before after : ActorProgressObservation} {key : EndpointKey} {side : Side}
    (changed : observedViewCommitMatches before after (.endpoint key) side = true) :
    ∃ run nonce, after.viewHeaders =
      (replaceObservedActorView before key.role run nonce).map ActorViewObservation.header := by
  cases found : observedActorView? before key.role with
  | none => simp [observedViewCommitMatches, found] at changed
  | some view =>
      cases side with
      | «local» => simp [observedViewCommitMatches, found] at changed
      | send =>
          simp only [observedViewCommitMatches, found, Bool.and_eq_true] at changed
          exact ⟨0, retireViewGeneration view.nonce, of_decide_eq_true changed.2.1.1.2⟩
      | receive =>
          cases fresh : nextViewGeneration? before.counter with
          | none => simp [observedViewCommitMatches, found, fresh] at changed
          | some nonce =>
              simp only [observedViewCommitMatches, found, fresh, Bool.and_eq_true] at changed
              exact ⟨before.run, nonce, of_decide_eq_true changed.2.2.1.1⟩

theorem checked_owned_commit_preserves_other_resource_tags
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session query : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (different : query ≠ facts.resource.role) :
    observedResourceTag? sample.after query = observedResourceTag? sample.before query := by
  have delta := checked_owned_commit_has_its_exact_resource_header_change checked decoded
  obtain ⟨run, nonce, changed⟩ := an_owned_commit_updates_only_its_resource_header delta.2.2
  exact a_changed_header_preserves_other_resource_tags delta.1 delta.2.1 changed different

theorem unchanged_headers_preserve_resource_tags
    {before after : ActorProgressObservation}
    (actorAt : after.actor = before.actor) (runAt : after.run = before.run)
    (headers : after.viewHeaders = before.viewHeaders) (role : Nat) :
    observedResourceTag? after role = observedResourceTag? before role := by
  simp only [observedResourceTag?, observedResourceHeader?, headers, actorAt, runAt]

theorem checked_retirement_invalidates_its_exact_observed_tag
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    ∃ view, observedActorView? sample.before sample.view = some view ∧
      observedResourceTag? sample.before sample.view = some (sample.before.actor, view.header) ∧
      observedResourceTag? sample.after sample.view = none ∧
      ∀ query, query ≠ sample.view →
        observedResourceTag? sample.after query = observedResourceTag? sample.before query := by
  obtain ⟨complete, view, _, found, progress, headers, _⟩ :=
    accepted_retirement_has_checked_source_and_headers checked
  have parts : sample.before.viewIssued sample.view = true ∧ sample.after.valid = true ∧
      sample.after.liveViews + 1 = sample.before.liveViews ∧
      sample.after.viewHeaders =
        (replaceObservedActorView sample.before sample.view 0 (view.nonce + 1)).map
          ActorViewObservation.header := by
    simpa only [ViewRetirementObservation.headersMatch, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq, and_assoc] using headers
  have live : view.run = sample.before.run ∧ view.nonce % 2 = 1 := by
    simpa only [ActorProgressObservation.viewIssued, found, Bool.and_eq_true, beq_iff_eq] using parts.1
  have stable : sample.after.actor = sample.before.actor ∧ sample.after.run = sample.before.run := by
    simp only [ViewRetirementObservation.progressMatches, Bool.and_eq_true, beq_iff_eq] at progress
    exact ⟨progress.1.1.1.1, progress.1.1.1.2⟩
  have beforeHeader : observedResourceHeader? sample.before sample.view = some view.header := by
    rw [observed_header_is_the_view_header, found]
    rfl
  have afterHeader := a_replaced_header_is_exact_at_its_role parts.2.2.2 beforeHeader
  have even : (view.nonce + 1) % 2 = 0 := by omega
  refine ⟨view, found, a_live_header_has_its_exact_tag beforeHeader live.1 live.2,
    an_even_header_has_no_live_tag afterHeader even, ?_⟩
  intro query different
  exact a_changed_header_preserves_other_resource_tags stable.1 stable.2 parts.2.2.2 different

/-- Reentry's actual prepared base may clear completed source rows and choices,
but cannot issue or resurrect a resource value. This uses the same prepared
base that the existing owned reentry checker passes to wire validation. -/
theorem prepared_reentry_preserves_all_observed_resource_tags
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    ∀ role, observedResourceTag? step.before role = observedResourceTag? sample.before role := by
  obtain ⟨actorAt, runAt, viewsAt⟩ := prepared_body_reentry_preserves_value_identity prepared
  exact unchanged_headers_preserve_resource_tags actorAt runAt
    (congrArg (List.map ActorViewObservation.header) viewsAt)

end Hibana
