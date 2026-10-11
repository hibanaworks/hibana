import Std

/-! A source-list refinement model for one compiler-only optimization:
collect the union of every event's sender and receiver, then omit globally
absent roles from the existing ascending observer-validation loop. No runtime
authority, projection admission, descriptor bytes, or validation stage changes.
The arbitrary-list proof is supplemented by concrete 32-byte Z3 bit operations.
-/
namespace Hibana.ParticipantMask

abbrev Role := Fin 256
structure Event where
  sender : Role
  receiver : Role
  label : Nat
  schema : Nat
  deriving DecidableEq, Repr
abbrev Mask := Role → Bool

def insert (mask : Mask) (role : Role) : Mask :=
  fun query => mask query || decide (role = query)

def addEvent (mask : Mask) (event : Event) : Mask :=
  insert (insert mask event.sender) event.receiver

def collect : List Event → Mask → Mask
  | [], mask => mask
  | head :: tail, mask => collect tail (addEvent mask head)

def occurs (event : Event) (role : Role) : Bool :=
  decide (event.sender = role) || decide (event.receiver = role)

def participants (events : List Event) : Mask := collect events (fun _ => false)

theorem collect_exact (events : List Event) (mask : Mask) (role : Role) :
    collect events mask role = (mask role || events.any (fun event => occurs event role)) := by
  induction events generalizing mask with
  | nil => simp [collect]
  | cons head tail ih =>
      simp [collect, ih, addEvent, insert, occurs, Bool.or_assoc]

theorem participants_exact (events : List Event) (role : Role) :
    participants events role = events.any (fun event => occurs event role) := by
  simp [participants, collect_exact]

theorem absence_pointwise (events : List Event) (role : Role)
    (h : participants events role = false) :
    ∀ event ∈ events, event.sender ≠ role ∧ event.receiver ≠ role := by
  rw [participants_exact] at h
  simpa [List.any_eq_false, occurs] using h

theorem member_marked (events : List Event) (role : Role) (event : Event)
    (he : event ∈ events) (hr : event.sender = role ∨ event.receiver = role) :
    participants events role = true := by
  cases hm : participants events role with
  | true => rfl
  | false =>
      have ha := absence_pointwise events role hm event he
      rcases hr with hs | ht
      · exact False.elim (ha.1 hs)
      · exact False.elim (ha.2 ht)

theorem duplicate_idempotent (mask : Mask) (event : Event) :
    addEvent (addEvent mask event) event = addEvent mask event := by
  funext role
  simp [addEvent, insert, Bool.or_assoc, Bool.or_left_comm, Bool.or_comm]

theorem self_send_marked (events : List Event) (event : Event)
    (he : event ∈ events) (hself : event.sender = event.receiver) :
    participants events event.sender = true ∧ participants events event.receiver = true := by
  constructor
  · exact member_marked events event.sender event he (Or.inl rfl)
  · exact member_marked events event.receiver event he (Or.inl hself)

inductive Selector where
  | outbound (sender : Role) (label schema : Nat)
  | inbound (eventIndex : Nat)
  deriving DecidableEq, Repr

def selector (event : Event) (index : Nat) (role : Role) : Option Selector :=
  if event.sender = role then some (.outbound event.sender event.label event.schema)
  else if event.receiver = role then some (.inbound index) else none

def nextSelector : List Event → Nat → Role → Option Selector
  | [], _, _ => none
  | head :: tail, index, role =>
      match selector head index role with
      | some selected => some selected
      | none => nextSelector tail (index + 1) role

def selectors : List Event → Nat → Role → List Selector
  | [], _, _ => []
  | head :: tail, index, role =>
      match selector head index role with
      | some selected => selected :: selectors tail (index + 1) role
      | none => selectors tail (index + 1) role

theorem absence_search_none (events : List Event) (index : Nat) (role : Role)
    (ha : ∀ event ∈ events, event.sender ≠ role ∧ event.receiver ≠ role) :
    nextSelector events index role = none ∧ selectors events index role = [] := by
  induction events generalizing index with
  | nil => simp [nextSelector, selectors]
  | cons head tail ih =>
      have hh := ha head (by simp)
      have ht : ∀ event ∈ tail, event.sender ≠ role ∧ event.receiver ≠ role := by
        intro event he; exact ha event (by simp [he])
      simpa [nextSelector, selectors, selector, hh.1, hh.2] using ih (index + 1) ht

/-- Same selector-pair decision as observer_path_decision; equal inbound
identities continue to the next pair. Other pairs fail except None/None. -/
def observerMerge : List Selector → List Selector → Bool
  | [], [] => true
  | .inbound left :: ls, .inbound right :: rs =>
      if left = right then observerMerge ls rs else true
  | _, _ => false

def observer (left right : List Event) (leftStart rightStart : Nat) (role : Role) : Bool :=
  observerMerge (selectors left leftStart role) (selectors right rightStart role)

theorem absent_arms (events left right : List Event) (leftStart rightStart : Nat)
    (role : Role) (hl : ∀ event ∈ left, event ∈ events)
    (hr : ∀ event ∈ right, event ∈ events) (hm : participants events role = false) :
    nextSelector left leftStart role = none ∧ nextSelector right rightStart role = none ∧
    observer left right leftStart rightStart role = true := by
  have absent := absence_pointwise events role hm
  have al : ∀ event ∈ left, event.sender ≠ role ∧ event.receiver ≠ role := by
    intro event he; exact absent event (hl event he)
  have ar : ∀ event ∈ right, event.sender ≠ role ∧ event.receiver ≠ role := by
    intro event he; exact absent event (hr event he)
  obtain ⟨nl, sl⟩ := absence_search_none left leftStart role al
  obtain ⟨nr, sr⟩ := absence_search_none right rightStart role ar
  exact ⟨nl, nr, by simp [observer, sl, sr, observerMerge]⟩

inductive Error where
  | routeControllerMismatch
  | receiveLaneCausalityConflict
  | parallelAmbiguousEndpointSelector
  | reentryAmbiguousEndpointSelector
  | projectionRouteUnprojectable
  deriving DecidableEq, Repr

def checkRole (controller : Role) (left right : List Event) (leftStart rightStart : Nat)
    (role : Role) : Option Error :=
  if role = controller ∨ observer left right leftStart rightStart role = true
  then none else some .projectionRouteUnprojectable

/-- Returns the failing role too, a stronger property than identical Error. -/
def ordered : List Role → (Role → Option Error) → Option (Role × Error)
  | [], _ => none
  | role :: rest, check =>
      match check role with
      | none => ordered rest check
      | some error => some (role, error)

theorem ordered_skip_successful (roles : List Role) (mask : Mask)
    (check : Role → Option Error) (hs : ∀ role, mask role = false → check role = none) :
    ordered (roles.filter mask) check = ordered roles check := by
  induction roles with
  | nil => simp [ordered]
  | cons role rest ih =>
      cases hm : mask role
      · simp [List.filter, hm, ordered, hs role hm, ih]
      · simp [List.filter, hm, ordered, ih]

theorem observer_validation_exact (events left right : List Event) (leftStart rightStart : Nat)
    (controller : Role) (roles : List Role)
    (hl : ∀ event ∈ left, event ∈ events) (hr : ∀ event ∈ right, event ∈ events) :
    ordered (roles.filter (participants events)) (checkRole controller left right leftStart rightStart) =
    ordered roles (checkRole controller left right leftStart rightStart) := by
  apply ordered_skip_successful
  intro role hm
  have h := (absent_arms events left right leftStart rightStart role hl hr hm).2.2
  simp [checkRole, h]

/-- Stable filtering cannot permute ascending roles or duplicate any role. -/
theorem skipped_roles_are_sublist (roles : List Role) (mask : Mask) :
    List.Sublist (roles.filter mask) roles := List.filter_sublist

structure Route where
  controller : Role
  left : List Event
  right : List Event
  leftStart : Nat
  rightStart : Nat
  preError : Option Error

/-- preError is controller/selector validation in its unchanged order. -/
def routeOriginal (roles : List Role) (route : Route) : Option Error :=
  match route.preError with
  | some error => some error
  | none => (ordered roles (checkRole route.controller route.left route.right route.leftStart route.rightStart)).map Prod.snd

def routeOptimized (events : List Event) (roles : List Role) (route : Route) : Option Error :=
  match route.preError with
  | some error => some error
  | none => (ordered (roles.filter (participants events)) (checkRole route.controller route.left route.right route.leftStart route.rightStart)).map Prod.snd

theorem route_exact (events : List Event) (roles : List Role) (route : Route)
    (hl : ∀ event ∈ route.left, event ∈ events) (hr : ∀ event ∈ route.right, event ∈ events) :
    routeOptimized events roles route = routeOriginal roles route := by
  simp [routeOptimized, routeOriginal, observer_validation_exact events route.left route.right route.leftStart route.rightStart route.controller roles hl hr]

def firstError : List (Option Error) → Option Error
  | [] => none
  | none :: rest => firstError rest
  | some error :: _ => some error

def originalPipeline (priorStages : List (Option Error)) (roles : List Role) (routes : List Route) : Option Error :=
  firstError (priorStages ++ routes.map (routeOriginal roles))

def optimizedPipeline (priorStages : List (Option Error)) (events : List Event)
    (roles : List Role) (routes : List Route) : Option Error :=
  firstError (priorStages ++ routes.map (routeOptimized events roles))

/-- Arbitrary earlier-stage errors, arbitrary routes, and identical first error. -/
theorem pipeline_exact (priorStages : List (Option Error)) (events : List Event)
    (roles : List Role) (routes : List Route)
    (h : ∀ route ∈ routes, (∀ event ∈ route.left, event ∈ events) ∧
                          (∀ event ∈ route.right, event ∈ events)) :
    optimizedPipeline priorStages events roles routes = originalPipeline priorStages roles routes := by
  have he : routes.map (routeOptimized events roles) = routes.map (routeOriginal roles) := by
    apply List.map_congr_left
    intro route hr
    exact route_exact events roles route (h route hr).1 (h route hr).2
  simp [optimizedPipeline, originalPipeline, he]

def roleCount : List Event → Nat
  | [] => 1
  | event :: rest => max (event.sender.val + 1) (max (event.receiver.val + 1) (roleCount rest))

theorem role_count_bounds (events : List Event) : 1 ≤ roleCount events ∧ roleCount events ≤ 256 := by
  induction events with
  | nil => simp [roleCount]
  | cons event rest ih =>
      have hs := event.sender.isLt
      have ht := event.receiver.isLt
      simp only [roleCount]
      omega

theorem participant_below_bound (events : List Event) (event : Event) (he : event ∈ events) :
    event.sender.val < roleCount events ∧ event.receiver.val < roleCount events := by
  induction events with
  | nil => simp at he
  | cons head rest ih =>
      simp only [List.mem_cons] at he
      rcases he with rfl | ht
      · simp only [roleCount]; omega
      · have h := ih ht
        simp only [roleCount]; omega

theorem byte_index_bounds (role : Role) :
    role.val / 8 < 32 ∧ role.val % 8 < 8 ∧ (role.val / 8) * 8 + role.val % 8 = role.val := by
  have h := role.isLt
  omega

theorem full_domain_counter_no_u8_wrap : 255 + 1 = (256 : Nat) ∧ 256 < 2 ^ 32 := by decide
example : (255 : Nat) / 8 = 31 ∧ (255 : Nat) % 8 = 7 := by decide

#print axioms collect_exact
#print axioms absence_search_none
#print axioms observer_validation_exact
#print axioms pipeline_exact
#print axioms role_count_bounds
#print axioms participant_below_bound
#print axioms byte_index_bounds
end Hibana.ParticipantMask
