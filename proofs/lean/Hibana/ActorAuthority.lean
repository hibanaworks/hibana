import Hibana.ActorRouting

namespace Hibana

/-- Decode the retained slot and the program's first matching resolver row.
The accepted source identity separately excludes duplicate or wrong rows. -/
def RustDescriptorImage.routeControllerAt?
    (image : RustDescriptorImage) (slot : Nat) : Option Nat := do
  let scope ← image.decodeRouteScope? slot
  let resolvers ← image.decodeRouteResolvers?
  (resolvers.find? fun row => row.scope == scope).bind (·.controller)

/-- Interpret the two first-visible rows exactly as the cursor's
`contains_route_controller`. The operation kind and logical view come from
the actual event bytes, rather than the physical actor number. -/
def RustDescriptorImage.routeControllerPresent?
    (image : RustDescriptorImage) (slot controller : Nat) : Option Bool := do
  let arms ← [0, 1].mapM fun arm => do
    let events ← image.decodeRouteArmEvents? slot arm
    match events.head? with
    | none => some false
    | some event => do
        let action ← image.eventAction? event
        let view ← image.eventView? event
        pure (match action with
          | .send _ _ _ | .local _ _ => view == controller
          | .recv _ _ _ => false)
  pure (arms.any id)

/-- Independently find the first actor action in each source arm. A local
unit operation can control a choice; a receive cannot. -/
def sourceActorControllerPresent?
    (markers : List DecodedScopeMarker)
    (candidates : List (EndpointDelegation.ActionId × EndpointDelegation.Action))
    (scope controller : Nat) : Option Bool := do
  let (left, right) ← routeArmRanges? markers scope
  pure ([left, right].any fun bounds =>
    match candidates.find? (fun (action, _) =>
        bounds.1 ≤ action.event && action.event < bounds.2) with
    | none => false
    | some (_, action) =>
        (action.side == .send || action.side == .local) && action.endpoint.role == controller)

/-- Kernel entry additionally binds controller capability to its issued
logical view. Co-residence at one actor does not grant this capability. -/
def controllerViewAllows (controller : Nat) (present : Bool) (view : Nat) : Bool :=
  present && controller == view

theorem controller_authority_belongs_to_its_logical_view
    {controller view : Nat} {present : Bool}
    (allowed : controllerViewAllows controller present view = true) :
    view = controller := by
  simp only [controllerViewAllows, Bool.and_eq_true, beq_iff_eq] at allowed
  exact allowed.2.symm

private def controllerRowMatches
    (image : RustDescriptorImage) (resolvers : List DecodedRouteResolver)
    (markers : List DecodedScopeMarker)
    (candidates : List (EndpointDelegation.ActionId × EndpointDelegation.Action))
    (scope slot : Nat) : Bool :=
  match (resolvers.find? fun row => row.scope == scope).bind (·.controller) with
  | none => false
  | some controller =>
      match image.routeControllerPresent? slot controller,
          sourceActorControllerPresent? markers candidates scope controller with
      | some actual, some expected => actual == expected
      | _, _ => false

/-- Necessary controller authority translation check for exact owned images.
It binds complete resolver identities and first-visible controller witnesses
to source. Ingress, reentry, and transfer commit remain separate obligations. -/
def RustDescriptorImage.actorControllerCheck
    (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorRowsCheck choreo &&
    match choreo.ownershipSource?, image.decodeRouteResolvers?, image.decodeRouteScopeRows? with
    | some source, some resolvers, some scopes =>
        match EndpointDelegation.normalize source with
        | none => false
        | some normalized =>
            let canonical := canonicalProgramSource choreo
            let candidates := source.actorCandidateActions normalized.paths image.role
            resolvers == canonical.resolvers &&
              scopes.zipIdx.all fun (scope, slot) =>
                controllerRowMatches image resolvers canonical.markers candidates scope slot
    | _, _, _ => false

/-- Every accepted first-visible controller witness has the source's exact
resolver identity and agrees with source candidate actions. Any authorized
view is that controller's logical role, regardless of its physical actor. -/
theorem accepted_actor_controllers_bind_source_authority
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorControllerCheck choreo = true) :
    image.actorRowsCheck choreo = true ∧
    ∃ source normalized resolvers scopes,
      choreo.ownershipSource? = some source ∧
      EndpointDelegation.normalize source = some normalized ∧
      image.decodeRouteResolvers? = some resolvers ∧
      image.decodeRouteScopeRows? = some scopes ∧
      resolvers = (canonicalProgramSource choreo).resolvers ∧
      ∀ scope slot, (scope, slot) ∈ scopes.zipIdx →
        ∃ controller present,
          (resolvers.find? fun row => row.scope == scope).bind (·.controller) = some controller ∧
          image.routeControllerPresent? slot controller = some present ∧
          sourceActorControllerPresent? (canonicalProgramSource choreo).markers
            (source.actorCandidateActions normalized.paths image.role) scope controller =
              some present ∧
          ∀ view, controllerViewAllows controller present view = true → view = controller := by
  unfold RustDescriptorImage.actorControllerCheck at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨accepted.1, ?_⟩
  cases sourceAt : choreo.ownershipSource? with
  | none => simp [sourceAt] at accepted
  | some source =>
      cases resolversAt : image.decodeRouteResolvers? with
      | none => simp [sourceAt, resolversAt] at accepted
      | some resolvers =>
          cases scopesAt : image.decodeRouteScopeRows? with
          | none => simp [sourceAt, resolversAt, scopesAt] at accepted
          | some scopes =>
              cases normalizedAt : EndpointDelegation.normalize source with
              | none => simp [sourceAt, resolversAt, scopesAt, normalizedAt] at accepted
              | some normalized =>
                  have checked := accepted.2
                  simp only [sourceAt, resolversAt, scopesAt, normalizedAt, Bool.and_eq_true] at checked
                  refine ⟨source, normalized, resolvers, scopes, rfl, normalizedAt,
                    rfl, rfl, beq_iff_eq.mp checked.1, ?_⟩
                  intro scope slot member
                  have row := List.all_eq_true.mp checked.2 (scope, slot) member
                  cases controllerAt :
                      (resolvers.find? fun row => row.scope == scope).bind (·.controller) with
                  | none => simp [controllerRowMatches, controllerAt] at row
                  | some controller =>
                      cases actualAt : image.routeControllerPresent? slot controller with
                      | none => simp [controllerRowMatches, controllerAt, actualAt] at row
                      | some actual =>
                          cases expectedAt : sourceActorControllerPresent?
                              (canonicalProgramSource choreo).markers
                              (source.actorCandidateActions normalized.paths image.role)
                              scope controller with
                          | none =>
                              simp [controllerRowMatches, controllerAt, actualAt, expectedAt] at row
                          | some expected =>
                              have same : actual = expected := by
                                simpa [controllerRowMatches, controllerAt, actualAt, expectedAt]
                                  using row
                              refine ⟨controller, actual, rfl, actualAt, ?_, ?_⟩
                              · simpa only [same] using expectedAt
                              · intro view allowed
                                exact controller_authority_belongs_to_its_logical_view allowed

end Hibana
