import Hibana.DescriptorImage
import Hibana.EndpointOwnership

namespace Hibana

private def ownershipPayload? (selector : Nat) : Option EndpointDelegation.Payload :=
  if selector < 4294967296 then some (.wire selector)
  else if selector - 4294967296 < 256 then
    some (.endpoint { component := 0, role := selector - 4294967296 })
  else none

/-- The source witness already carries a disjoint payload selector. This
conversion preserves Endpoint payloads as endpoints in the independent source
semantics; it does not give them a wire codec. -/
def Choreo.ownershipSource? : Choreo → Option EndpointDelegation.Source
  | .send sender receiver label selector => do
      let payload ← ownershipPayload? selector
      pure (.send {
        sender := { component := 0, role := sender }
        receiver := { component := 0, role := receiver }
        label, payload
      })
  | .seq left right => do
      pure (.seq (← left.ownershipSource?) (← right.ownershipSource?))
  | .par left right => do
      pure (.par (← left.ownershipSource?) (← right.ownershipSource?))
  | .route authority left right => do
      let resolver := match authority with
        | .intrinsic => none
        | .dynamic resolver => some resolver
      pure (.route resolver (← left.ownershipSource?) (← right.ownershipSource?))
  | .roll body => do pure (.roll (← body.ownershipSource?))

def Choreo.hasOwnedPayload : Choreo → Bool
  | .send _ _ _ selector => 4294967296 ≤ selector
  | .seq left right | .par left right | .route _ left right =>
      left.hasOwnedPayload || right.hasOwnedPayload
  | .roll body => body.hasOwnedPayload

private def selectActorWithBound
    (rows : List DecodedActorDecision) (selected : Nat → Option RouteArm) :
    Nat → Nat → Option Nat
  | 0, _ => none
  | remaining + 1, reference =>
      if reference < 256 then some reference else do
        let row ← rows[reference - 256]?
        let arm ← selected row.scope
        let next := match arm with
          | .left => row.left
          | .right => row.right
        if next < reference then selectActorWithBound rows selected remaining next else none

/-- Every validated decision edge points to an earlier row. Its row count plus
the final direct actor therefore bounds traversal without a guessed fuel
constant. Unknown choices and malformed references cannot authorize a holder. -/
def DecodedProgramColumns.selectedActor?
    (columns : DecodedProgramColumns) (selected : Nat → Option RouteArm)
    (reference : Nat) : Option Nat :=
  selectActorWithBound columns.actorDecisions selected
    (columns.actorDecisions.length + 1) reference

theorem unknown_actor_choice_does_not_authorize_a_holder
    {columns : DecodedProgramColumns} {selected : Nat → Option RouteArm}
    {reference : Nat} {row : DecodedActorDecision}
    (decision : 256 ≤ reference)
    (rowAt : columns.actorDecisions[reference - 256]? = some row)
    (unknown : selected row.scope = none) :
    columns.selectedActor? selected reference = none := by
  simp [DecodedProgramColumns.selectedActor?, selectActorWithBound,
    Nat.not_lt.mpr decision, rowAt, unknown]

private theorem actor_reference_valid_with_more_rows
    {roleCount before after reference : Nat}
    (valid : validActorReference roleCount before reference = true)
    (covered : before ≤ after) :
    validActorReference roleCount after reference = true := by
  unfold validActorReference at valid ⊢
  split <;> simp_all <;> omega

private theorem actor_reference_decreases
    {reference next : Nat} (node : 256 ≤ reference)
    (lower : next < 256 + (reference - 256)) : next < reference := by omega

private theorem actor_child_fits_remaining
    {reference next remaining : Nat}
    (lower : next < 256 + (reference - 256))
    (budget : reference - 256 + 2 ≤ remaining + 1) :
    0 < remaining ∧ (next < 256 ∨ next - 256 + 2 ≤ remaining) := by
  constructor
  · exact Nat.lt_of_lt_of_le (Nat.zero_lt_succ _) (Nat.le_of_succ_le_succ budget)
  · by_cases leaf : next < 256
    · exact Or.inl leaf
    · have sum : 256 + (next - 256) < 256 + (reference - 256) := by
        simpa only [Nat.add_sub_of_le (Nat.le_of_not_gt leaf)] using lower
      have indexLower := Nat.lt_of_add_lt_add_left sum
      exact Or.inr (Nat.le_trans (Nat.succ_le_succ (Nat.succ_le_of_lt indexLower))
        (Nat.le_of_succ_le_succ budget))

private theorem actor_reference_fits_full_bound
    {roleCount count reference : Nat}
    (valid : validActorReference roleCount count reference = true) :
    reference < 256 ∨ reference - 256 + 2 ≤ count + 1 := by
  by_cases direct : reference < 256
  · exact Or.inl direct
  · have parts : reference < 65535 ∧ reference - 256 < count := by
      simpa [validActorReference, direct] using valid
    exact Or.inr (by omega)

private theorem bounded_actor_selection_complete
    {rows : List DecodedActorDecision} {selected : Nat → Option RouteArm}
    {roleCount : Nat} {scopes : List Nat}
    (checked : ∀ (index : Nat) (row : DecodedActorDecision),
      rows[index]? = some row → row.check roleCount index scopes = true)
    (chosen : ∀ (index : Nat) (row : DecodedActorDecision), rows[index]? = some row →
      selected row.scope = some .left ∨ selected row.scope = some .right)
    (fuel : Nat) :
    ∀ reference, validActorReference roleCount rows.length reference = true →
      0 < fuel → (reference < 256 ∨ reference - 256 + 2 ≤ fuel) →
      ∃ actor, selectActorWithBound rows selected fuel reference = some actor ∧
        actor < roleCount := by
  induction fuel with
  | zero => intro reference valid positive enough; omega
  | succ remaining ih =>
      intro reference valid positive enough
      by_cases direct : reference < 256
      · refine ⟨reference, ?_, ?_⟩
        · simp [selectActorWithBound, direct]
        · simpa [validActorReference, direct] using valid
      · have inRows : reference - 256 < rows.length := by
          have parts : reference < 65535 ∧ reference - 256 < rows.length := by
            simpa [validActorReference, direct] using valid
          exact parts.2
        let row := rows[reference - 256]
        have rowAt : rows[reference - 256]? = some row := List.getElem?_eq_getElem inRows
        have rowChecked := checked _ row rowAt
        have decreasing := accepted_actor_decision_references_decrease rowChecked
        have rowFields := rowChecked
        simp only [DecodedActorDecision.check, Bool.and_eq_true] at rowFields
        have leftValid := rowFields.1.1.1.1
        have rightValid := rowFields.1.1.1.2
        have budget : reference - 256 + 2 ≤ remaining + 1 := enough.resolve_left direct
        have nextBudget (next : Nat) (decreases : next < 256 + (reference - 256)) :
            0 < remaining ∧ (next < 256 ∨ next - 256 + 2 ≤ remaining) := by
          exact actor_child_fits_remaining decreases budget
        rcases chosen _ row rowAt with left | right
        · have nextValid := actor_reference_valid_with_more_rows leftValid (Nat.le_of_lt inRows)
          have enoughNext := nextBudget row.left decreasing.1
          obtain ⟨actor, result, range⟩ := ih row.left nextValid enoughNext.1 enoughNext.2
          refine ⟨actor, ?_, range⟩
          have lower := actor_reference_decreases (Nat.le_of_not_gt direct) decreasing.1
          simp [selectActorWithBound, direct, rowAt, left, lower, result]
        · have nextValid := actor_reference_valid_with_more_rows rightValid (Nat.le_of_lt inRows)
          have enoughNext := nextBudget row.right decreasing.2
          obtain ⟨actor, result, range⟩ := ih row.right nextValid enoughNext.1 enoughNext.2
          refine ⟨actor, ?_, range⟩
          have lower := actor_reference_decreases (Nat.le_of_not_gt direct) decreasing.2
          simp [selectActorWithBound, direct, rowAt, right, lower, result]

/-- The descriptor-derived bound cannot truncate a legal reference chain.
This is quantified over all row counts, actor identifiers and total choices,
not just the concrete Rust fixtures. -/
theorem checked_actor_reference_resolves_within_its_descriptor_bound
    {columns : DecodedProgramColumns} {selected : Nat → Option RouteArm}
    {roleCount reference : Nat} {scopes : List Nat}
    (checked : ∀ (index : Nat) (row : DecodedActorDecision), columns.actorDecisions[index]? = some row →
      row.check roleCount index scopes = true)
    (chosen : ∀ (index : Nat) (row : DecodedActorDecision), columns.actorDecisions[index]? = some row →
      selected row.scope = some .left ∨ selected row.scope = some .right)
    (valid : validActorReference roleCount columns.actorDecisions.length reference = true) :
    ∃ actor, columns.selectedActor? selected reference = some actor ∧ actor < roleCount := by
  exact bounded_actor_selection_complete checked chosen _ reference valid
    (Nat.zero_lt_succ _) (actor_reference_fits_full_bound valid)

private def sourceScope?
    (markers : List DecodedScopeMarker) (scope : Nat) : Option EndpointDelegation.Scope := do
  let (left, right) ← routeArmRanges? markers scope
  if left.2 = right.1 then
    pure { start := left.1, split := left.2, stop := right.2 }
  else none

def sourcePathArm?
    (markers : List DecodedScopeMarker) (path : EndpointDelegation.NormalizedPath)
    (scope : Nat) : Option RouteArm := do
  let sourceScope ← sourceScope? markers scope
  let selection ← resolveUnique? (path.selections.filter (fun row => row.scope == sourceScope))
  pure selection.arm

def DecodedProgramColumns.sourcePathMatches
    (columns : DecodedProgramColumns) (markers : List DecodedScopeMarker)
    (path : EndpointDelegation.NormalizedPath) : Bool :=
  path.bindings.all fun (event, binding) =>
    match columns.actorBindings[event]? with
    | none => false
    | some (sender, receiver) =>
        columns.selectedActor? (sourcePathArm? markers path) sender == some binding.senderActor &&
        columns.selectedActor? (sourcePathArm? markers path) receiver == some binding.receiverActor

theorem matching_source_path_has_the_source_actors
    {columns : DecodedProgramColumns} {markers : List DecodedScopeMarker}
    {path : EndpointDelegation.NormalizedPath} {event : Nat}
    {binding : EndpointDelegation.Binding}
    (agrees : columns.sourcePathMatches markers path = true)
    (member : (event, binding) ∈ path.bindings) :
    ∃ sender receiver, columns.actorBindings[event]? = some (sender, receiver) ∧
      columns.selectedActor? (sourcePathArm? markers path) sender = some binding.senderActor ∧
      columns.selectedActor? (sourcePathArm? markers path) receiver = some binding.receiverActor := by
  have atBinding := List.all_eq_true.mp agrees (event, binding) member
  cases bindingAt : columns.actorBindings[event]? with
  | none => simp [bindingAt] at atBinding
  | some pair =>
      rcases pair with ⟨sender, receiver⟩
      exact ⟨sender, receiver, rfl, by simpa [bindingAt] using atBinding⟩

private def eraseGlobalLane (event : GlobalEvent) : GlobalEvent := { event with lane := 0 }

private def ownershipColumnsMatch
    (image : RustDescriptorImage) (choreo : Choreo)
    (normalized : EndpointDelegation.Normalization) (columns : DecodedProgramColumns)
    (atoms : List DecodedProgramAtom) (markers : List DecodedScopeMarker) : Bool :=
  0 < image.roleCount && image.roleCount ≤ 256 &&
  columns.atomCount == image.atomCount &&
  columns.routeResolverCount == image.routeResolverCount &&
  columns.routeParticipantCount == image.routeParticipantCount &&
  columns.scopeMarkerCount == image.scopeMarkerCount &&
  (atoms.map (fun atom => eraseGlobalLane atom.globalEvent)) ==
    (choreo.globalEvents.map eraseGlobalLane) &&
  markers == choreo.canonicalScopeMarkers &&
  columns.initialActors ==
    (normalized.issuance.map (fun issue => (issue.endpoint.role, issue.actor))) &&
  if choreo.hasOwnedPayload then
    columns.actorBindings.length == atoms.length &&
    columns.actorBindings.all (fun (sender, receiver) =>
      validActorReference image.roleCount columns.actorDecisions.length sender &&
      validActorReference image.roleCount columns.actorDecisions.length receiver) &&
    columns.actorDecisions.zipIdx.all (fun (row, index) =>
      row.check image.roleCount index
        ((canonicalProgramSource choreo).resolvers.map DecodedRouteResolver.scope)) &&
    normalized.paths.all (columns.sourcePathMatches markers)
  else
    columns.actorBindings.isEmpty && columns.actorDecisions.isEmpty &&
    columns.initialActors.isEmpty

private def ownershipData? (image : RustDescriptorImage) (choreo : Choreo) :
    Option (EndpointDelegation.Source × EndpointDelegation.Normalization ×
      DecodedProgramColumns × List DecodedProgramAtom × List DecodedScopeMarker) := do
  let source ← choreo.ownershipSource?
  let normalized ← EndpointDelegation.normalize source
  let columns ← image.decodeProgramColumns?
  let atoms ← image.decodeAtoms?
  let markers ← image.decodeScopeMarkers?
  pure (source, normalized, columns, atoms, markers)

private theorem ownership_data_binds_decoders
    {image : RustDescriptorImage} {choreo : Choreo}
    {source : EndpointDelegation.Source} {normalized : EndpointDelegation.Normalization}
    {columns : DecodedProgramColumns} {atoms : List DecodedProgramAtom}
    {markers : List DecodedScopeMarker}
    (decoded : ownershipData? image choreo = some (source, normalized, columns, atoms, markers)) :
    choreo.ownershipSource? = some source ∧
    EndpointDelegation.normalize source = some normalized ∧
    image.decodeProgramColumns? = some columns ∧ image.decodeAtoms? = some atoms ∧
    image.decodeScopeMarkers? = some markers := by
  unfold ownershipData? at decoded
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq, Prod.mk.injEq] at decoded
  rcases decoded with ⟨source', sourceAt, normalized', normalizedAt, columns', columnsAt,
    atoms', atomsAt, markers', markersAt, rfl, rfl, rfl, rfl, rfl⟩
  exact ⟨sourceAt, normalizedAt, columnsAt, atomsAt, markersAt⟩

/-- Ownership translation validation for the exact global bytes. Ordinary
topology, lanes and runtime commits have their own obligations; this predicate
is a necessary part of their shared descriptor certificate, not a substitute. -/
def RustDescriptorImage.ownershipCheck (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  match ownershipData? image choreo with
  | none => false
  | some (_, normalized, columns, atoms, markers) =>
      ownershipColumnsMatch image choreo normalized columns atoms markers

/-- Accepted owned bytes use the initial issuers and every conditional holder
derived by independently interpreting the original global source. -/
theorem accepted_owned_program_binds_source_ownership
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.ownershipCheck choreo = true)
    (owned : choreo.hasOwnedPayload = true) :
    ∃ source normalized columns atoms markers,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧
      image.decodeProgramColumns? = some columns ∧ image.decodeAtoms? = some atoms ∧
      image.decodeScopeMarkers? = some markers ∧
      columns.initialActors = normalized.issuance.map (fun issue => (issue.endpoint.role, issue.actor)) ∧
      ∀ path ∈ normalized.paths, columns.sourcePathMatches markers path = true := by
  cases decoded : ownershipData? image choreo with
  | none => simp [RustDescriptorImage.ownershipCheck, decoded] at accepted
  | some data =>
      rcases data with ⟨source, normalized, columns, atoms, markers⟩
      have fields := ownership_data_binds_decoders decoded
      have checked : ownershipColumnsMatch image choreo normalized columns atoms markers = true := by
        simpa [RustDescriptorImage.ownershipCheck, decoded] using accepted
      simp only [ownershipColumnsMatch, owned, ↓reduceIte, Bool.and_eq_true] at checked
      refine ⟨source, normalized, columns, atoms, markers, fields.1, fields.2.1,
        fields.2.2.1, fields.2.2.2.1, fields.2.2.2.2, ?_, ?_⟩
      · exact of_decide_eq_true (by simpa using checked.1.2)
      · exact List.all_eq_true.mp checked.2.2

end Hibana
