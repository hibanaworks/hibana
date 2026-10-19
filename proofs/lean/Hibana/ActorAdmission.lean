import Hibana.ActorCommit

namespace Hibana

/-- Copied results of the production cursor's read-only `event_enabled` query.
These bits never authorize operations and are absent from the runtime. -/
structure ActorAdmissionObservation where
  snapshot : ActorProgressObservation
  pathIndex : Nat
  admitted : List Bool
  /-- Copied completed-roll scopes, used by the current/fresh candidate check.
  The selected-visit checker below deliberately concerns current event bits. -/
  retainedRolls : List Nat := []

def sourceRowReady (data : DecodedActorOrder)
    (path : EndpointDelegation.NormalizedPath)
    (snapshot : ActorProgressObservation) (row : DecodedActorRow) : Bool :=
  !snapshot.doneState.done row.eventId &&
    (data.sourcePredecessors path row.eventId).all snapshot.doneState.done

/-- Header issuance is observed separately from cursor readiness. An owned
send also needs the payload's issued logical view. This does not turn a header
into an application value or rule out a private future's temporary borrow. -/
def sourceRowViewsIssued (path : EndpointDelegation.NormalizedPath)
    (snapshot : ActorProgressObservation) (row : DecodedActorRow) : Bool :=
  snapshot.viewIssued row.view &&
    match path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
    | none => false
    | some (_, binding) =>
        match row.actionId.side, binding.message.payload with
        | .send, .endpoint key => key.component == 0 && snapshot.viewIssued key.role
        | _, _ => true

/-- Translation validation for an existing selected visit, with no roll reset.
The result is reconstructed from source dependencies, not from another copy of
the Rust admission algorithm. Header ownership and transport readiness are
separate checks; this artifact does not assume that they follow from done bits. -/
def ActorAdmissionObservation.check
    (sample : ActorAdmissionObservation) (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorOrderCheck choreo && sample.snapshot.valid &&
    match image.decodeActorOrder? choreo with
    | none => false
    | some data =>
        match data.normalized.paths[sample.pathIndex]? with
        | none => false
        | some path =>
            sample.snapshot.metadataMatches image data.rows &&
            observedChoicesAgree (canonicalProgramSource choreo) path sample.snapshot &&
            sample.admitted.length == data.rows.length &&
            (data.activeRows path).all fun row =>
              observedSourcePrefixCheck data path sample.snapshot row.eventId &&
                (sample.admitted[row.eventId]? ==
                  some (sourceRowReady data path sample.snapshot row) &&
                (!sourceRowReady data path sample.snapshot row ||
                  sourceRowViewsIssued path sample.snapshot row))

/-- Every accepted observation equates the actual cursor result to source
readiness for every active row, including blocked and completed rows. This is
an observation checker theorem, not whole-program Rust refinement. -/
theorem accepted_actor_admission_has_exact_source_readiness
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    ∃ data path, image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[sample.pathIndex]? = some path ∧
      ∀ row ∈ data.activeRows path,
        sample.snapshot.doneState.PrefixClosed (data.sourcePredecessors path)
          (data.sourcePredecessors path row.eventId) ∧
        sample.admitted[row.eventId]? = some (sourceRowReady data path sample.snapshot row) ∧
        (sourceRowReady data path sample.snapshot row = true →
          sourceRowViewsIssued path sample.snapshot row = true) := by
  unfold ActorAdmissionObservation.check at checked
  have checked := (Bool.and_eq_true_iff.mp checked).2
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [dataAt] at checked
  | some data =>
      cases pathAt : data.normalized.paths[sample.pathIndex]? with
      | none => simp [dataAt, pathAt] at checked
      | some path =>
          have rows : (data.activeRows path).all (fun row =>
              observedSourcePrefixCheck data path sample.snapshot row.eventId &&
                (sample.admitted[row.eventId]? ==
                  some (sourceRowReady data path sample.snapshot row) &&
                (!sourceRowReady data path sample.snapshot row ||
                  sourceRowViewsIssued path sample.snapshot row))) = true := by
            have body := checked
            simp only [dataAt, pathAt] at body
            exact (Bool.and_eq_true_iff.mp body).2
          refine ⟨data, path, rfl, pathAt, ?_⟩
          intro row member
          have fields := Bool.and_eq_true_iff.mp (List.all_eq_true.mp rows row member)
          have result := Bool.and_eq_true_iff.mp fields.2
          refine ⟨observed_source_prefix_check_is_sound fields.1, beq_iff_eq.mp result.1, ?_⟩
          intro ready
          simpa only [ready, Bool.not_true, Bool.false_or] using result.2

theorem source_ready_row_is_admitted_by_a_checked_cursor_observation
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {data : DecodedActorOrder} {path : EndpointDelegation.NormalizedPath}
    {row : DecodedActorRow}
    (checked : sample.check image choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (ready : sourceRowReady data path sample.snapshot row = true) :
    sample.admitted[row.eventId]? = some true ∧
      sourceRowViewsIssued path sample.snapshot row = true := by
  obtain ⟨actual, chosen, actualAt, chosenAt, rows⟩ :=
    accepted_actor_admission_has_exact_source_readiness checked
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  have samePath := Option.some.inj (chosenAt.symm.trans selected)
  subst chosen
  exact ⟨(rows row member).2.1.trans (congrArg some ready), (rows row member).2.2 ready⟩

end Hibana
