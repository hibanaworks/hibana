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

/-- Adding choice evidence within a visit cannot change an already known
actor. This uses the same immutable decision rows and traversal bound. -/
theorem known_actor_is_stable_under_choice_extension
    {columns : DecodedProgramColumns} {known extended : Nat → Option RouteArm}
    (extension : ∀ scope arm, known scope = some arm → extended scope = some arm)
    {reference actor : Nat}
    (found : columns.selectedActor? known reference = some actor) :
    columns.selectedActor? extended reference = some actor := by
  have stable : ∀ fuel reference actor,
      selectActorWithBound columns.actorDecisions known fuel reference = some actor →
      selectActorWithBound columns.actorDecisions extended fuel reference = some actor := by
    intro fuel
    induction fuel with
    | zero => intro reference actor impossible; simp [selectActorWithBound] at impossible
    | succ remaining ih =>
        intro reference actor accepted
        by_cases direct : reference < 256
        · simpa only [selectActorWithBound, if_pos direct] using accepted
        · cases rowAt : columns.actorDecisions[reference - 256]? with
          | none => simp [selectActorWithBound, direct, rowAt] at accepted
          | some row =>
              cases armAt : known row.scope with
              | none => simp [selectActorWithBound, direct, rowAt, armAt] at accepted
              | some arm =>
                  have agreed := extension row.scope arm armAt
                  cases arm with
                  | left =>
                      by_cases lower : row.left < reference
                      · have prior : selectActorWithBound columns.actorDecisions known
                            remaining row.left = some actor := by
                          simpa [selectActorWithBound, direct, rowAt, armAt, lower] using accepted
                        have next := ih row.left actor prior
                        simpa [selectActorWithBound, direct, rowAt, agreed, lower] using next
                      · simp [selectActorWithBound, direct, rowAt, armAt, lower] at accepted
                  | right =>
                      by_cases lower : row.right < reference
                      · have prior : selectActorWithBound columns.actorDecisions known
                            remaining row.right = some actor := by
                          simpa [selectActorWithBound, direct, rowAt, armAt, lower] using accepted
                        have next := ih row.right actor prior
                        simpa [selectActorWithBound, direct, rowAt, agreed, lower] using next
                      · simp [selectActorWithBound, direct, rowAt, armAt, lower] at accepted
  exact stable (columns.actorDecisions.length + 1) reference actor found

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

/-- Every known partial result agrees with every source path extending that
evidence. Unknown results grant no actor and require no current-holder map. -/
theorem matching_source_path_binds_partially_known_actors
    {columns : DecodedProgramColumns} {markers : List DecodedScopeMarker}
    {path : EndpointDelegation.NormalizedPath} {known : Nat → Option RouteArm}
    {event : Nat} {binding : EndpointDelegation.Binding}
    (agrees : columns.sourcePathMatches markers path = true)
    (extension : ∀ scope arm, known scope = some arm →
      sourcePathArm? markers path scope = some arm)
    (member : (event, binding) ∈ path.bindings) :
    ∃ sender receiver, columns.actorBindings[event]? = some (sender, receiver) ∧
      (∀ actor, columns.selectedActor? known sender = some actor → actor = binding.senderActor) ∧
      (∀ actor, columns.selectedActor? known receiver = some actor → actor = binding.receiverActor) := by
  obtain ⟨sender, receiver, bindingAt, sourceSender, sourceReceiver⟩ :=
    matching_source_path_has_the_source_actors agrees member
  refine ⟨sender, receiver, bindingAt, ?_, ?_⟩
  · intro actor found
    have stable := known_actor_is_stable_under_choice_extension extension found
    exact Option.some.inj (stable.symm.trans sourceSender)
  · intro actor found
    have stable := known_actor_is_stable_under_choice_extension extension found
    exact Option.some.inj (stable.symm.trans sourceReceiver)

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

/-- Exact owned bytes bind partially known actors to the original source on
every compatible path. This is within one visit, not a rule for stale choices
after a repeated-region reset or an ownership-transfer commit theorem. -/
theorem accepted_owned_program_binds_partial_choice_evidence
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.ownershipCheck choreo = true)
    (owned : choreo.hasOwnedPayload = true) :
    ∃ source normalized columns atoms markers,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧
      image.decodeProgramColumns? = some columns ∧ image.decodeAtoms? = some atoms ∧
      image.decodeScopeMarkers? = some markers ∧
      ∀ path ∈ normalized.paths, ∀ known : Nat → Option RouteArm,
        (∀ scope arm, known scope = some arm → sourcePathArm? markers path scope = some arm) →
        ∀ event binding, (event, binding) ∈ path.bindings →
          ∃ sender receiver, columns.actorBindings[event]? = some (sender, receiver) ∧
            (∀ actor, columns.selectedActor? known sender = some actor →
              actor = binding.senderActor) ∧
            (∀ actor, columns.selectedActor? known receiver = some actor →
              actor = binding.receiverActor) := by
  obtain ⟨source, normalized, columns, atoms, markers,
      sourceAt, normalizedAt, columnsAt, atomsAt, markersAt, _, paths⟩ :=
    accepted_owned_program_binds_source_ownership accepted owned
  refine ⟨source, normalized, columns, atoms, markers,
    sourceAt, normalizedAt, columnsAt, atomsAt, markersAt, ?_⟩
  intro path pathMember known extension event binding member
  exact matching_source_path_binds_partially_known_actors (paths path pathMember) extension member

/-- Immutable identity of one actor-local row. Its local event index and source
occurrence are different coordinates; neither is a second progress cursor. -/
structure DecodedActorRow where
  eventId : Nat
  actionId : EndpointDelegation.ActionId
  view : Nat
  actorReference : Nat
  deriving Repr, DecidableEq

def DecodedActorRow.sourceAction (row : DecodedActorRow) (actor : Nat) :
    EndpointDelegation.ActionId × EndpointDelegation.Action :=
  (row.actionId, {
    actor, endpoint := { component := 0, role := row.view }, side := row.actionId.side
  })

private def actorReferenceFrom?
    (columns : DecodedProgramColumns) (atom : DecodedProgramAtom)
    (occurrence view : Nat) : Option Nat :=
  if columns.actorBindings.isEmpty then some view else do
    let (sender, receiver) ← columns.actorBindings[occurrence]?
    if view == atom.sender then some sender
    else if view == atom.receiver then some receiver else none

private def RustDescriptorImage.actorRowFrom?
    (image : RustDescriptorImage) (atoms : List DecodedProgramAtom)
    (columns : DecodedProgramColumns) (eventId : Nat) : Option DecodedActorRow := do
    let effIndex ← image.eventEffIndex? eventId
    let entry ← resolveUnique?
      (atoms.zipIdx.filter fun (atom, _) => atom.effIndex == effIndex)
    let action ← image.eventAction? eventId
    let view ← image.eventView? eventId
    let reference ← actorReferenceFrom? columns entry.1 entry.2 view
    let side := match action with
      | .send _ _ _ => EndpointDelegation.Side.send
      | .recv _ _ _ => EndpointDelegation.Side.receive
      | .local _ _ => EndpointDelegation.Side.local
    pure { eventId, actionId := { event := entry.2, side }, view, actorReference := reference }

def RustDescriptorImage.decodeActorRows? (image : RustDescriptorImage) :
    Option (List DecodedActorRow) := do
  let atoms ← image.decodeAtoms?
  let columns ← image.decodeProgramColumns?
  (List.range image.eventCount).mapM (image.actorRowFrom? atoms columns)

private theorem decoded_actor_row_binds_view
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {columns : DecodedProgramColumns} {eventId : Nat} {row : DecodedActorRow}
    (decoded : image.actorRowFrom? atoms columns eventId = some row) :
    row.eventId = eventId ∧ image.eventView? eventId = some row.view ∧
      ∃ action, image.eventAction? eventId = some action ∧ row.actionId.side =
        match action with
        | .send _ _ _ => .send
        | .recv _ _ _ => .receive
        | .local _ _ => .local := by
  unfold RustDescriptorImage.actorRowFrom? at decoded
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq] at decoded
  rcases decoded with ⟨effIndex, _, pair, _, action, actionAt, view, viewAt, reference, _, same⟩
  cases same
  exact ⟨rfl, viewAt, action, actionAt, rfl⟩

private theorem actor_row_mapM_member_binds_view
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {columns : DecodedProgramColumns} {indices : List Nat} {rows : List DecodedActorRow}
    {row : DecodedActorRow}
    (decoded : indices.mapM (image.actorRowFrom? atoms columns) = some rows)
    (member : row ∈ rows) : image.eventView? row.eventId = some row.view ∧
      ∃ action, image.eventAction? row.eventId = some action ∧ row.actionId.side =
        match action with
        | .send _ _ _ => .send
        | .recv _ _ _ => .receive
        | .local _ _ => .local := by
  induction indices generalizing rows with
  | nil => simp at decoded; subst rows; simp at member
  | cons index rest ih =>
      cases headAt : image.actorRowFrom? atoms columns index with
      | none => simp [headAt] at decoded
      | some head =>
          cases tailAt : rest.mapM (image.actorRowFrom? atoms columns) with
          | none => simp [headAt, tailAt] at decoded
          | some tail =>
              have same : rows = head :: tail :=
                (Option.some.inj (by simpa [headAt, tailAt] using decoded)).symm
              rw [same] at member
              rcases List.mem_cons.mp member with equal | member
              · subst row
                have fields := decoded_actor_row_binds_view headAt
                simpa only [fields.1] using fields.2
              · exact ih tailAt member

theorem decoded_actor_rows_bind_operation_identity
    {image : RustDescriptorImage} {rows : List DecodedActorRow} {row : DecodedActorRow}
    (decoded : image.decodeActorRows? = some rows) (member : row ∈ rows) :
    image.eventView? row.eventId = some row.view ∧
      ∃ action, image.eventAction? row.eventId = some action ∧ row.actionId.side =
        match action with
        | .send _ _ _ => .send
        | .recv _ _ _ => .receive
        | .local _ _ => .local := by
  unfold RustDescriptorImage.decodeActorRows? at decoded
  cases atomsAt : image.decodeAtoms? with
  | none => simp [atomsAt] at decoded
  | some atoms =>
      cases columnsAt : image.decodeProgramColumns? with
      | none => simp [atomsAt, columnsAt] at decoded
      | some columns =>
          have rowsAt : (List.range image.eventCount).mapM
              (image.actorRowFrom? atoms columns) = some rows := by
            simpa [atomsAt, columnsAt] using decoded
          exact actor_row_mapM_member_binds_view rowsAt member

private theorem actor_row_from_retains_source_position
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {columns : DecodedProgramColumns} {event : Nat} {row : DecodedActorRow}
    (decoded : image.actorRowFrom? atoms columns event = some row) :
    ∃ eff atom, image.eventEffIndex? row.eventId = some eff ∧
      resolveUnique? (atoms.zipIdx.filter fun entry => entry.1.effIndex == eff) =
        some (atom, row.actionId.event) := by
  unfold RustDescriptorImage.actorRowFrom? at decoded
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq] at decoded
  obtain ⟨eff, effAt, entry, entryAt, action, _, view, _, reference, _, same⟩ := decoded
  cases same
  exact ⟨eff, entry.1, effAt, entryAt⟩

private theorem actor_row_mapM_retains_source_position
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {columns : DecodedProgramColumns} {indices : List Nat} {rows : List DecodedActorRow}
    {row : DecodedActorRow}
    (decoded : indices.mapM (image.actorRowFrom? atoms columns) = some rows)
    (member : row ∈ rows) :
    ∃ eff atom, image.eventEffIndex? row.eventId = some eff ∧
      resolveUnique? (atoms.zipIdx.filter fun entry => entry.1.effIndex == eff) =
        some (atom, row.actionId.event) := by
  induction indices generalizing rows with
  | nil => simp at decoded; subst rows; simp at member
  | cons index rest ih =>
      cases headAt : image.actorRowFrom? atoms columns index with
      | none => simp [headAt] at decoded
      | some head =>
          cases tailAt : rest.mapM (image.actorRowFrom? atoms columns) with
          | none => simp [headAt, tailAt] at decoded
          | some tail =>
              have same : rows = head :: tail :=
                (Option.some.inj (by simpa [headAt, tailAt] using decoded)).symm
              rw [same] at member
              rcases List.mem_cons.mp member with equal | member
              · subst row; exact actor_row_from_retains_source_position headAt
              · exact ih tailAt member

/-- A local operation's source occurrence is the unique atom at its encoded
effect offset. Two actor images with the same immutable atom table therefore
cannot interpret that offset as different source occurrences. Neither local
route choices nor repetition history occurs in this translation fact. -/
theorem decoded_actor_row_retains_its_unique_source_position
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {rows : List DecodedActorRow} {row : DecodedActorRow}
    (decoded : image.decodeActorRows? = some rows)
    (atomTable : image.decodeAtoms? = some atoms) (member : row ∈ rows) :
    ∃ eff atom, image.eventEffIndex? row.eventId = some eff ∧
      resolveUnique? (atoms.zipIdx.filter fun entry => entry.1.effIndex == eff) =
        some (atom, row.actionId.event) := by
  unfold RustDescriptorImage.decodeActorRows? at decoded
  rw [atomTable] at decoded
  simp only [Option.bind_eq_bind, Option.bind_some, Option.bind_eq_some_iff] at decoded
  obtain ⟨columns, _, mapped⟩ := decoded
  exact actor_row_mapM_retains_source_position mapped member

private theorem decoded_actor_row_indices
    {image : RustDescriptorImage} {atoms : List DecodedProgramAtom}
    {columns : DecodedProgramColumns} {indices : List Nat} {rows : List DecodedActorRow}
    (decoded : indices.mapM (image.actorRowFrom? atoms columns) = some rows) :
    rows.map DecodedActorRow.eventId = indices := by
  induction indices generalizing rows with
  | nil => simp at decoded; subst rows; rfl
  | cons index rest ih =>
      cases headAt : image.actorRowFrom? atoms columns index with
      | none => simp [headAt] at decoded
      | some head =>
          cases tailAt : rest.mapM (image.actorRowFrom? atoms columns) with
          | none => simp [headAt, tailAt] at decoded
          | some tail =>
              have same : rows = head :: tail :=
                (Option.some.inj (by simpa [headAt, tailAt] using decoded)).symm
              rw [same, List.map_cons, (decoded_actor_row_binds_view headAt).1, ih tailAt]

/-- A decoded row's local event ID is its actual list position. Membership
alone therefore suffices to recover that exact immutable descriptor row. -/
theorem decoded_actor_row_is_at_its_own_index
    {image : RustDescriptorImage} {rows : List DecodedActorRow} {row : DecodedActorRow}
    (decoded : image.decodeActorRows? = some rows) (member : row ∈ rows) :
    rows[row.eventId]? = some row := by
  unfold RustDescriptorImage.decodeActorRows? at decoded
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff] at decoded
  obtain ⟨atoms, _, columns, _, mapped⟩ := decoded
  have indices := decoded_actor_row_indices mapped
  obtain ⟨index, located⟩ := List.mem_iff_getElem?.mp member
  have bound := (List.getElem?_eq_some_iff.mp located).1
  have lengths := congrArg List.length indices
  simp only [List.length_map, List.length_range] at lengths
  have identity := congrArg (fun entries => entries[index]?) indices
  simp only [List.getElem?_map, located, Option.map_some] at identity
  rw [List.getElem?_range (i := index) (n := image.eventCount) (by omega)] at identity
  have same := Option.some.inj identity
  simpa only [same] using located

/-- The position used before I/O is the decoded row's own immutable ID.
Unlike membership alone, this also identifies an arbitrary supplied lookup. -/
theorem decoded_actor_row_lookup_preserves_its_index
    {image : RustDescriptorImage} {rows : List DecodedActorRow} {row : DecodedActorRow} {event : Nat}
    (decoded : image.decodeActorRows? = some rows) (located : rows[event]? = some row) :
    row.eventId = event := by
  unfold RustDescriptorImage.decodeActorRows? at decoded
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff] at decoded
  obtain ⟨atoms, _, columns, _, mapped⟩ := decoded
  have indices := decoded_actor_row_indices mapped
  have bound := (List.getElem?_eq_some_iff.mp located).1
  have lengths := congrArg List.length indices
  simp only [List.length_map, List.length_range] at lengths
  have identity := congrArg (fun entries => entries[event]?) indices
  simp only [List.getElem?_map, located, Option.map_some] at identity
  rw [List.getElem?_range (i := event) (n := image.eventCount) (by omega)] at identity
  exact Option.some.inj identity

/-- Source action identities retain both send and receive when two different
logical roles share one physical actor. No runtime holder state is synthesized. -/
def EndpointDelegation.Source.actorActions
    (source : EndpointDelegation.Source) (path : EndpointDelegation.NormalizedPath)
    (actor : Nat) : List (EndpointDelegation.ActionId × EndpointDelegation.Action) :=
  (List.range (3 * source.eventCount)).filterMap fun encoded => do
    let id := EndpointDelegation.decodeActionId encoded
    let action ← EndpointDelegation.action? path.bindings id
    if action.actor == actor then some (id, action) else none

/-- The immutable union of source actions that can belong to this actor.
Each source occurrence is visited once, in send/receive/local order. This is
static row coverage, not current-holder or operation authority. -/
def EndpointDelegation.Source.actorCandidateActions
    (source : EndpointDelegation.Source) (paths : List EndpointDelegation.NormalizedPath)
    (actor : Nat) : List (EndpointDelegation.ActionId × EndpointDelegation.Action) :=
  (List.range (3 * source.eventCount)).filterMap fun encoded =>
    let id := EndpointDelegation.decodeActionId encoded
    paths.findSome? fun path => do
      let action ← EndpointDelegation.action? path.bindings id
      if action.actor == actor then some (id, action) else none

theorem source_candidate_action_has_a_source_path
    {source : EndpointDelegation.Source} {paths : List EndpointDelegation.NormalizedPath}
    {actor : Nat} {id : EndpointDelegation.ActionId} {action : EndpointDelegation.Action}
    (member : (id, action) ∈ source.actorCandidateActions paths actor) :
    ∃ path ∈ paths, EndpointDelegation.action? path.bindings id = some action ∧
      action.actor = actor := by
  obtain ⟨encoded, _, found⟩ := List.mem_filterMap.mp member
  obtain ⟨path, pathMember, atPath⟩ := List.exists_of_findSome?_eq_some found
  cases actionAt : EndpointDelegation.action? path.bindings
      (EndpointDelegation.decodeActionId encoded) with
  | none => simp [actionAt] at atPath
  | some actual =>
      by_cases belongs : actual.actor = actor
      · have pair : (EndpointDelegation.decodeActionId encoded, actual) = (id, action) := by
          simpa [actionAt, belongs] using atPath
        cases pair
        exact ⟨path, pathMember, actionAt, belongs⟩
      · simp [actionAt, belongs] at atPath

def DecodedProgramColumns.actorRowsForPath
    (columns : DecodedProgramColumns) (markers : List DecodedScopeMarker)
    (path : EndpointDelegation.NormalizedPath) (actor : Nat)
    (rows : List DecodedActorRow) : List DecodedActorRow :=
  rows.filter fun row =>
    (path.bindings.any fun (event, _) => event == row.actionId.event) &&
      columns.selectedActor? (sourcePathArm? markers path) row.actorReference == some actor

/-- Every stored row must be a possible source action, and every source path
must retain exactly its own active operations. The static union is used only
for complete row coverage; active ownership still uses that same path, not
the union of possible holders. Dependency and reentry have separate obligations. -/
def RustDescriptorImage.actorRowsCheck (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  match ownershipData? image choreo, image.decodeActorRows? with
  | some (source, normalized, columns, atoms, markers), some rows =>
      ownershipColumnsMatch image choreo normalized columns atoms markers &&
      (((rows.map (·.sourceAction image.role)) ==
        source.actorCandidateActions normalized.paths image.role) &&
      normalized.paths.all fun path =>
        ((columns.actorRowsForPath markers path image.role rows).map
          (·.sourceAction image.role)) == source.actorActions path image.role) &&
      (image.roleBytes.length == image.roleBlobLen && image.receiveIndexCoherent)
  | _, _ => false

theorem accepted_actor_rows_bind_source_operations
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorRowsCheck choreo = true) :
    ∃ source normalized columns atoms markers rows,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧
      image.decodeProgramColumns? = some columns ∧ image.decodeAtoms? = some atoms ∧
      image.decodeScopeMarkers? = some markers ∧ image.decodeActorRows? = some rows ∧
      image.ownershipCheck choreo = true ∧
      ∀ path ∈ normalized.paths,
        ((columns.actorRowsForPath markers path image.role rows).map
          (·.sourceAction image.role)) = source.actorActions path image.role := by
  cases decoded : ownershipData? image choreo with
  | none => simp [RustDescriptorImage.actorRowsCheck, decoded] at accepted
  | some data =>
      rcases data with ⟨source, normalized, columns, atoms, markers⟩
      cases rowsAt : image.decodeActorRows? with
      | none => simp [RustDescriptorImage.actorRowsCheck, decoded, rowsAt] at accepted
      | some rows =>
          have checked : (ownershipColumnsMatch image choreo normalized columns atoms markers = true ∧
              ((rows.map (·.sourceAction image.role)) ==
                source.actorCandidateActions normalized.paths image.role) = true ∧
              (normalized.paths.all fun path =>
                ((columns.actorRowsForPath markers path image.role rows).map
                  (·.sourceAction image.role)) ==
                  source.actorActions path image.role) = true) ∧
              (image.roleBytes.length == image.roleBlobLen && image.receiveIndexCoherent) = true := by
            simpa [RustDescriptorImage.actorRowsCheck, decoded, rowsAt] using accepted
          have fields := ownership_data_binds_decoders decoded
          refine ⟨source, normalized, columns, atoms, markers, rows, fields.1, fields.2.1,
            fields.2.2.1, fields.2.2.2.1, fields.2.2.2.2, rfl, ?_, ?_⟩
          · simpa [RustDescriptorImage.ownershipCheck, decoded] using checked.1.1
          · intro path member
            exact of_decide_eq_true (by simpa using List.all_eq_true.mp checked.1.2.2 path member)

/-- Path filtering cannot hide extra rows belonging to no possible execution
of this actor. The exact static rows remain tied to source action identities. -/
theorem accepted_actor_rows_bind_candidate_operations
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorRowsCheck choreo = true) :
    ∃ source normalized rows,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧
      image.decodeActorRows? = some rows ∧
      rows.map (·.sourceAction image.role) =
        source.actorCandidateActions normalized.paths image.role := by
  cases decoded : ownershipData? image choreo with
  | none => simp [RustDescriptorImage.actorRowsCheck, decoded] at accepted
  | some data =>
      rcases data with ⟨source, normalized, columns, atoms, markers⟩
      cases rowsAt : image.decodeActorRows? with
      | none => simp [RustDescriptorImage.actorRowsCheck, decoded, rowsAt] at accepted
      | some rows =>
          have checked := accepted
          simp only [RustDescriptorImage.actorRowsCheck, decoded, rowsAt,
            Bool.and_eq_true] at checked
          have fields := ownership_data_binds_decoders decoded
          exact ⟨source, normalized, rows, fields.1, fields.2.1, rfl,
            beq_iff_eq.mp checked.1.2.1⟩

end Hibana
