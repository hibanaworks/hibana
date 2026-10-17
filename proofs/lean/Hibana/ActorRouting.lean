import Hibana.ActorProgress
import Hibana.Authority

namespace Hibana

/-- Read the exact event conflict and route-parent columns, using Rust's
route-count bound. A missing parent or cycle has no accepted path. -/
private def routeMembershipPath?
    (parent : Nat → Option (Option (Nat × Nat))) :
    Nat → Option (Nat × Nat) → Option (List (Nat × Nat))
  | 0, _ => none
  | _ + 1, none => some []
  | remaining + 1, some member => do
      let next ← parent member.1
      pure (member :: (← routeMembershipPath? parent remaining next))

def RustDescriptorImage.eventRoutePath?
    (image : RustDescriptorImage) (event : Nat) : Option (List (Nat × Nat)) := do
  let member ← image.eventConflictMembership? event
  routeMembershipPath? image.routeParentMembership? (image.routeScopeCount + 1) member

def canonicalEventRoutePath?
    (source : CanonicalProgramSource) (effIndex : Nat) : Option (List (Nat × Nat)) :=
  let initial := (canonicalRouteArmAt? source effIndex).map fun arm => (arm.scope, arm.arm)
  let parent := fun scope =>
    if source.resolvers.any (fun row => row.scope == scope) then
      some ((canonicalParentRouteArm? source scope).map fun arm => (arm.scope, arm.arm))
    else none
  routeMembershipPath? parent (source.resolvers.length + 1) initial

/-- The conflict filter permits an unknown choice. It is not route authority:
controller, ingress evidence and reentry admission have additional checks.
Committed choices take precedence over the candidate operation's preview. -/
def routePathAllows
    (path : List (Nat × Nat))
    (selected preview : Nat → Option RouteArm) : Bool :=
  path.all fun (scope, arm) =>
    match (selected scope).orElse (fun _ => preview scope) with
    | none => true
    | some chosen => encodeRouteArm chosen == arm

theorem committed_route_choice_cannot_be_overridden_by_preview
    {path : List (Nat × Nat)} {selected : Nat → Option RouteArm}
    {scope arm : Nat} {chosen : RouteArm}
    (member : (scope, arm) ∈ path)
    (committed : selected scope = some chosen)
    (different : encodeRouteArm chosen ≠ arm)
    (preview : Nat → Option RouteArm) :
    routePathAllows path selected preview = false := by
  apply Bool.eq_false_iff.mpr
  intro allowed
  have row := List.all_eq_true.mp allowed (scope, arm) member
  simp [committed, different] at row

theorem preview_route_choice_rejects_a_conflicting_path
    {path : List (Nat × Nat)} {selected preview : Nat → Option RouteArm}
    {scope arm : Nat} {chosen : RouteArm}
    (member : (scope, arm) ∈ path)
    (unknown : selected scope = none)
    (observed : preview scope = some chosen)
    (different : encodeRouteArm chosen ≠ arm) :
    routePathAllows path selected preview = false := by
  apply Bool.eq_false_iff.mpr
  intro allowed
  have row := List.all_eq_true.mp allowed (scope, arm) member
  simp [unknown, observed, different] at row

theorem matching_choice_history_allows_route_path
    {path : List (Nat × Nat)} {selected preview : Nat → Option RouteArm}
    (agrees : ∀ scope arm, (scope, arm) ∈ path → ∀ chosen,
      (selected scope).orElse (fun _ => preview scope) = some chosen →
      encodeRouteArm chosen = arm) :
    routePathAllows path selected preview = true := by
  apply List.all_eq_true.mpr
  intro (scope, arm) member
  change (match (selected scope).orElse (fun _ => preview scope) with
    | none => true
    | some chosen => encodeRouteArm chosen == arm) = true
  cases choiceAt : (selected scope).orElse (fun _ => preview scope) with
  | none => rfl
  | some chosen => exact beq_iff_eq.mpr (agrees scope arm member chosen choiceAt)

def RustDescriptorImage.actorRouteCheck
    (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  image.actorRowsCheck choreo && image.routeArmColumnsCoherent &&
    image.routeCommitColumnsCoherent &&
    let source := canonicalProgramSource choreo
    match image.decodeRouteScopeRows?, image.decodeRouteScopeConflictRows? with
    | some scopes, some parents =>
        scopes.length == parents.length &&
        (scopes.zip parents).all (fun (scope, parent) =>
          source.resolvers.any (fun row => row.scope == scope) &&
            parent == canonicalRouteScopeConflict source scope) &&
        (List.range image.eventCount).all fun event =>
          match image.eventEffIndex? event,
              image.eventRoutePath? event with
          | some effIndex, some actual =>
              canonicalEventRoutePath? source effIndex == some actual
          | _, _ => false
    | _, _ => false

/-- The exact decoded route path has the same partial-history filter as the
source path for every committed history and preview. No extra route state is
introduced; this is a necessary component, not complete route authority. -/
theorem accepted_actor_routes_bind_source_choice_filter
    {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : image.actorRouteCheck choreo = true) :
    image.actorRowsCheck choreo = true ∧
    ∀ event ∈ List.range image.eventCount,
      ∃ effIndex actual,
        image.eventEffIndex? event = some effIndex ∧
        image.eventRoutePath? event = some actual ∧
        canonicalEventRoutePath? (canonicalProgramSource choreo) effIndex = some actual ∧
        ∀ selected preview expected,
          canonicalEventRoutePath? (canonicalProgramSource choreo) effIndex = some expected →
          routePathAllows actual selected preview = routePathAllows expected selected preview := by
  unfold RustDescriptorImage.actorRouteCheck at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨accepted.1.1.1, ?_⟩
  cases scopesAt : image.decodeRouteScopeRows? with
  | none => simp [scopesAt] at accepted
  | some scopes =>
      cases parentsAt : image.decodeRouteScopeConflictRows? with
      | none => simp [scopesAt, parentsAt] at accepted
      | some parents =>
          have checked := accepted.2
          simp only [scopesAt, parentsAt, Bool.and_eq_true] at checked
          intro event member
          have row := List.all_eq_true.mp checked.2 event member
          cases effAt : image.eventEffIndex? event with
          | none => simp [effAt] at row
          | some eff =>
              cases pathAt : image.eventRoutePath? event with
              | none => simp [effAt, pathAt] at row
              | some actual =>
                  have sourceAt : canonicalEventRoutePath?
                      (canonicalProgramSource choreo) eff = some actual := by
                    simpa [effAt, pathAt] using row
                  refine ⟨eff, actual, rfl, rfl, sourceAt, ?_⟩
                  intro selected preview expected expectedAt
                  have same : actual = expected := Option.some.inj (sourceAt.symm.trans expectedAt)
                  exact congrArg (fun path => routePathAllows path selected preview) same

end Hibana
