import Hibana.ActorAuthority

namespace Hibana

/-- Matching owns no progress state. `admitted` is the result of the shared
actor's existing admission check. The active logical view comes from the
Endpoint, after the transport preamble has checked its target, run and lane. -/
def RustDescriptorImage.actorInboundRowMatches
    (image : RustDescriptorImage) (view : Nat) (key : InboundOperationKey)
    (admitted : Nat → Bool) (row : DecodedActorRow) : Bool :=
  row.view == view && admitted row.eventId &&
    match image.eventOperation? row.eventId with
    | none => false
    | some operation => operation.matchesInbound key

def RustDescriptorImage.matchingActorInboundRows?
    (image : RustDescriptorImage) (view : Nat) (key : InboundOperationKey)
    (admitted : Nat → Bool) : Option (List DecodedActorRow) :=
  image.decodeActorRows?.map fun rows =>
    rows.filter (image.actorInboundRowMatches view key admitted)

/-- The same singleton rule as Rust's UniqueMatch. A shared physical actor
does not replace the logical-view guard, including when wire keys are reused. -/
def RustDescriptorImage.resolveActorInbound?
    (image : RustDescriptorImage) (view : Nat) (key : InboundOperationKey)
    (admitted : Nat → Bool) : Option DecodedActorRow :=
  (image.matchingActorInboundRows? view key admitted).bind resolveUnique?

theorem resolved_actor_ingress_is_unique
    {image : RustDescriptorImage} {view : Nat} {key : InboundOperationKey}
    {admitted : Nat → Bool} {row : DecodedActorRow}
    (resolved : image.resolveActorInbound? view key admitted = some row) :
    image.matchingActorInboundRows? view key admitted = some [row] := by
  unfold RustDescriptorImage.resolveActorInbound? at resolved
  cases matching : image.matchingActorInboundRows? view key admitted with
  | none => simp [matching] at resolved
  | some rows =>
      have singleton : rows = [row] := resolveUnique?_eq_some_iff.mp
        (by simpa [matching] using resolved)
      simp [singleton]

private theorem resolved_actor_ingress_has_member
    {image : RustDescriptorImage} {view : Nat} {key : InboundOperationKey}
    {admitted : Nat → Bool} {row : DecodedActorRow}
    (resolved : image.resolveActorInbound? view key admitted = some row) :
    ∃ rows, image.decodeActorRows? = some rows ∧ row ∈ rows ∧
      image.actorInboundRowMatches view key admitted row = true := by
  have unique := resolved_actor_ingress_is_unique resolved
  unfold RustDescriptorImage.matchingActorInboundRows? at unique
  cases decoded : image.decodeActorRows? with
  | none => simp [decoded] at unique
  | some rows =>
      have filtered : rows.filter (image.actorInboundRowMatches view key admitted) = [row] :=
        Option.some.inj (by simpa [decoded] using unique)
      have member : row ∈ rows.filter (image.actorInboundRowMatches view key admitted) := by
        rw [filtered]
        simp
      exact ⟨rows, rfl, (List.mem_filter.mp member).1, (List.mem_filter.mp member).2⟩

private theorem matching_inbound_action_is_receive
    {operation : DescriptorOperation} {key : InboundOperationKey}
    (matched : operation.matchesInbound key = true) :
    ∃ peer label schema, operation.action = .recv peer label schema := by
  cases actionAt : operation.action with
  | send peer label schema => simp [DescriptorOperation.matchesInbound, actionAt] at matched
  | «local» label schema => simp [DescriptorOperation.matchesInbound, actionAt] at matched
  | recv peer label schema => exact ⟨peer, label, schema, rfl⟩

theorem resolved_actor_ingress_binds_view_and_complete_key
    {image : RustDescriptorImage} {view : Nat} {key : InboundOperationKey}
    {admitted : Nat → Bool} {row : DecodedActorRow}
    (resolved : image.resolveActorInbound? view key admitted = some row) :
    ∃ operation,
      image.eventOperation? row.eventId = some operation ∧
      image.eventView? row.eventId = some view ∧ row.view = view ∧
      row.actionId.side = .receive ∧ operation.matchesInbound key = true ∧
      admitted row.eventId = true := by
  obtain ⟨rows, decoded, member, checked⟩ := resolved_actor_ingress_has_member resolved
  have identity := decoded_actor_rows_bind_operation_identity decoded member
  simp only [RustDescriptorImage.actorInboundRowMatches, Bool.and_eq_true] at checked
  have sameView : row.view = view := beq_iff_eq.mp checked.1.1
  cases operationAt : image.eventOperation? row.eventId with
  | none => simp [operationAt] at checked
  | some operation =>
      have matched : operation.matchesInbound key = true := by
        simpa [operationAt] using checked.2
      have fields := decoded_event_operation_binds_exact_columns operationAt
      obtain ⟨action, actionAt, sideAt⟩ := identity.2
      have sameAction : action = operation.action :=
        Option.some.inj (actionAt.symm.trans fields.2.1)
      obtain ⟨peer, label, schema, receiveAt⟩ := matching_inbound_action_is_receive matched
      have receives : row.actionId.side = .receive := by
        simpa only [sameAction, receiveAt] using sideAt
      refine ⟨operation, rfl, ?_, sameView, receives, matched, checked.1.2⟩
      simpa only [sameView] using identity.1

/-- Every matched, admitted row is a receive of this logical view on a source
path of the accepted global. This does not establish the admission predicate,
fresh-roll laws, preamble validation or transport authentication by itself. -/
theorem accepted_actor_ingress_binds_source_receive
    {image : RustDescriptorImage} {choreo : Choreo} {view : Nat}
    {key : InboundOperationKey} {admitted : Nat → Bool} {row : DecodedActorRow}
    (accepted : image.actorRowsCheck choreo = true)
    (resolved : image.resolveActorInbound? view key admitted = some row) :
    ∃ source normalized path,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧ path ∈ normalized.paths ∧
      EndpointDelegation.action? path.bindings row.actionId = some {
        actor := image.role, endpoint := { component := 0, role := view }, side := .receive
      } := by
  obtain ⟨rows, rowsAt, member, _⟩ := resolved_actor_ingress_has_member resolved
  obtain ⟨source, normalized, expectedRows, sourceAt, normalizedAt, expectedAt, candidates⟩ :=
    accepted_actor_rows_bind_candidate_operations accepted
  have sameRows : rows = expectedRows := Option.some.inj (rowsAt.symm.trans expectedAt)
  subst expectedRows
  have belongs : row.sourceAction image.role ∈
      source.actorCandidateActions normalized.paths image.role := by
    rw [← candidates]
    exact List.mem_map.mpr ⟨row, member, rfl⟩
  change (row.actionId, ({
    actor := image.role, endpoint := { component := 0, role := row.view }, side := row.actionId.side
  } : EndpointDelegation.Action)) ∈ _ at belongs
  obtain ⟨path, pathMember, actionAt, _⟩ := source_candidate_action_has_a_source_path belongs
  obtain ⟨operation, _, _, sameView, receives, _, _⟩ :=
    resolved_actor_ingress_binds_view_and_complete_key resolved
  refine ⟨source, normalized, path, sourceAt, normalizedAt, pathMember, ?_⟩
  simpa only [sameView, receives] using actionAt

end Hibana
