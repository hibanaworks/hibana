import Std

/-! Source-linked models of controller offer descent and conflict-chain priority.
They prove the mathematical selection rules for arbitrary finite trees/chains.
Production descriptor decoding, affine preview ownership and atomic publication
remain checked by Rust regressions and the existing kernel refinement contracts.
-/
namespace ControllerOffer

inductive Tree where
  | leaf (label : Nat)
  | route (resolver : Nat) (left right : Tree)
  deriving DecidableEq

abbrev Selection := Nat × List (Nat × Bool)

def withDecision (resolver : Nat) (arm : Bool) : Option Selection → Option Selection
  | none => none
  | some (label, path) => some (label, (resolver, arm) :: path)

def select (decide : Nat → Option Bool) : Tree → Option Selection
  | .leaf label => some (label, [])
  | .route resolver left right =>
    match decide resolver with
    | none => none
    | some false => withDecision resolver false (select decide left)
    | some true => withDecision resolver true (select decide right)

def firstLabel : Tree → Nat
  | .leaf label => label
  | .route _ left _ => firstLabel left

/-- A scope's canonical entry and an observed descendant are distinct
locators. Preserve the operation and lane of the row that passed admission. -/
structure ResolverEntry where
  event : Nat
  lane : Nat
  deriving DecidableEq

def firstAdmitted (entries : List ResolverEntry) (admitted : ResolverEntry → Bool) :
    Option ResolverEntry := entries.find? admitted

theorem a_resolver_candidate_is_the_admitted_entry
    (entries : List ResolverEntry) (admitted : ResolverEntry → Bool) (row : ResolverEntry) :
    firstAdmitted entries admitted = some row → row ∈ entries ∧ admitted row = true := by
  induction entries with
  | nil => simp [firstAdmitted]
  | cons first rest ih =>
      intro selected
      cases allowed : admitted first with
      | false =>
          have found : firstAdmitted rest admitted = some row := by
            simpa [firstAdmitted, allowed] using selected
          obtain ⟨member, valid⟩ := ih found
          exact ⟨List.mem_cons_of_mem first member, valid⟩
      | true =>
          have same : first = row := by simpa [firstAdmitted, allowed] using selected
          subst row
          exact ⟨List.mem_cons_self, allowed⟩

theorem an_eligible_scope_does_not_admit_its_blocked_descendant :
    firstAdmitted [⟨1, 2⟩, ⟨3, 1⟩] (fun row => row.event == 1) = some ⟨1, 2⟩ ∧
    ((ResolverEntry.mk 3 1).event == 1) = false := by decide

#print axioms a_resolver_candidate_is_the_admitted_entry
#print axioms an_eligible_scope_does_not_admit_its_blocked_descendant

/-- A committed outgoing choice can proceed without a frame. Among candidates
with the same local readiness, preserve transport/controller ordering. -/
def offerPriority (base : Nat) (locallyReady unmade : Bool) : Nat :=
  (if locallyReady then 16 else 0) + 2 * base + if unmade then 1 else 0

theorem an_unmade_choice_precedes_an_equally_informed_retained_choice
    (base : Nat) (outgoing : Bool) :
    offerPriority base outgoing false < offerPriority base outgoing true := by
  simp [offerPriority]

theorem known_outgoing_precedes_an_unframed_receive
    (readyBase waitingBase : Nat) (outgoingUnmade receivingUnmade : Bool)
    (bounded : waitingBase ≤ 4) :
    offerPriority waitingBase false receivingUnmade <
      offerPriority readyBase true outgoingUnmade := by
  cases outgoingUnmade <;> cases receivingUnmade <;> simp [offerPriority] <;> omega

theorem primary_offer_priority_is_preserved
    (lower higher : Nat) (ready firstUnmade secondUnmade : Bool)
    (ordered : lower < higher) :
    offerPriority lower ready firstUnmade <
      offerPriority higher ready secondUnmade := by
  have adjacent : lower + 1 ≤ higher := ordered
  have doubled : 2 * lower + 2 ≤ 2 * higher := by
    simpa [Nat.mul_add] using Nat.mul_le_mul_left 2 adjacent
  have raw : 2 * lower + 1 < 2 * higher :=
    Nat.lt_of_lt_of_le (Nat.add_lt_add_left (by decide : (1 : Nat) < 2) _) doubled
  let offset := if ready then 16 else 0
  have strict : offset + 2 * lower + 1 < offset + 2 * higher := by
    simpa [Nat.add_assoc] using Nat.add_lt_add_left raw offset
  have left : offerPriority lower ready firstUnmade ≤ offset + 2 * lower + 1 := by
    cases firstUnmade <;> simp [offerPriority, offset]
  have right : offset + 2 * higher ≤ offerPriority higher ready secondUnmade := by
    cases secondUnmade <;> simp [offerPriority, offset]
  exact Nat.lt_of_le_of_lt left (Nat.lt_of_lt_of_le strict right)

theorem offer_priority_fits_its_runtime_byte
    (base : Nat) (outgoing unmade : Bool) (bounded : base ≤ 4) :
    offerPriority base outgoing unmade ≤ 25 := by
  cases outgoing <;> cases unmade <;> simp [offerPriority] <;> omega

#print axioms an_unmade_choice_precedes_an_equally_informed_retained_choice
#print axioms known_outgoing_precedes_an_unframed_receive
#print axioms primary_offer_priority_is_preserved
#print axioms offer_priority_fits_its_runtime_byte

/-- Reusing a selected branch supplies its known decision, not a new receive
observation. Exact event admission and frame matching are separate checks. -/
def reuseKnownDecision (coherent receiving framed : Bool) : Bool :=
  coherent && (!receiving || framed)

theorem a_retained_receive_requires_an_acquired_frame (coherent framed : Bool)
    (allowed : reuseKnownDecision coherent true framed = true) : framed = true := by
  cases coherent <;> cases framed <;> simp_all [reuseKnownDecision]

theorem reusing_a_decision_requires_the_committed_branch
    (coherent receiving framed : Bool)
    (allowed : reuseKnownDecision coherent receiving framed = true) : coherent = true := by
  cases coherent <;> simp_all [reuseKnownDecision]

theorem a_known_send_requires_no_incoming_frame :
    reuseKnownDecision true false false = true := by decide

#print axioms a_retained_receive_requires_an_acquired_frame
#print axioms reusing_a_decision_requires_the_committed_branch
#print axioms a_known_send_requires_no_incoming_frame

theorem leaf_has_no_decisions (decide : Nat → Option Bool) (label : Nat) :
    select decide (.leaf label) = some (label, []) := rfl

theorem rejected_parent_cannot_reach_a_descendant
    (decide : Nat → Option Bool) (resolver : Nat) (left right : Tree)
    (rejected : decide resolver = none) :
    select decide (.route resolver left right) = none := by
  simp [select, rejected]

theorem every_selected_ancestor_has_its_own_decision
    (decide : Nat → Option Bool) (tree : Tree) (label : Nat) (path : List (Nat × Bool))
    (selected : select decide tree = some (label, path)) :
    ∀ resolver arm, (resolver, arm) ∈ path → decide resolver = some arm := by
  induction tree generalizing label path with
  | leaf value =>
    simp [select] at selected
    rcases selected with ⟨_, rfl⟩
    simp
  | route resolver left right leftIH rightIH =>
    cases decision : decide resolver with
    | none => simp [select, decision] at selected
    | some arm =>
      cases arm <;>
        simp only [select, decision] at selected
      · cases child : select decide left with
        | none => simp [child, withDecision] at selected
        | some pair =>
          rcases pair with ⟨childLabel, childPath⟩
          simp only [child, withDecision, Option.some.injEq, Prod.mk.injEq] at selected
          rcases selected with ⟨rfl, rfl⟩
          intro query value membership
          rcases List.mem_cons.mp membership with same | inherited
          · rcases Prod.mk.inj same with ⟨rfl, rfl⟩
            exact decision
          · exact leftIH childLabel childPath child query value inherited
      · cases child : select decide right with
        | none => simp [child, withDecision] at selected
        | some pair =>
          rcases pair with ⟨childLabel, childPath⟩
          simp only [child, withDecision, Option.some.injEq, Prod.mk.injEq] at selected
          rcases selected with ⟨rfl, rfl⟩
          intro query value membership
          rcases List.mem_cons.mp membership with same | inherited
          · rcases Prod.mk.inj same with ⟨rfl, rfl⟩
            exact decision
          · exact rightIH childLabel childPath child query value inherited

structure Membership where
  scope : Nat
  required : Bool
  selected : Option Bool

def Membership.pending (row : Membership) : Bool :=
  match row.selected with
  | none => true
  | some selected => selected != row.required

/-- Resident conflict chains are visited from the event's inner scope outward. -/
def priority (path : List Membership) : Option Nat :=
  path.foldl (fun candidate row => if row.pending then some row.scope else candidate) none

theorem pending_ancestor_precedes_every_descendant
    (descendants : List Membership) (ancestor : Membership)
    (pending : ancestor.pending = true) :
    priority (descendants ++ [ancestor]) = some ancestor.scope := by
  simp [priority, List.foldl_append, pending]

theorem coherent_ancestor_preserves_descendant_priority
    (descendants : List Membership) (ancestor : Membership)
    (coherent : ancestor.pending = false) :
    priority (descendants ++ [ancestor]) = priority descendants := by
  simp [priority, List.foldl_append, coherent]

theorem an_unselected_ancestor_is_pending (scope : Nat) (required : Bool) :
    (Membership.mk scope required none).pending = true := rfl

theorem a_selected_matching_ancestor_is_coherent (scope : Nat) (arm : Bool) :
    (Membership.mk scope arm (some arm)).pending = false := by
  cases arm <;> rfl

/-- Source admission guard: a different pending scope must be at a route entry.
Membership in the old arm alone does not establish that entry. -/
def align (different atEntry : Bool) (current canonical : Nat) : Nat :=
  if different && !atEntry then canonical else current

theorem pending_ancestor_realigns_to_its_entry (current canonical : Nat) :
    align true false current canonical = canonical := rfl

theorem an_actual_entry_keeps_its_cursor (different : Bool) (current canonical : Nat) :
    align different true current canonical = current := by
  cases different <;> rfl

/-- Source offer-refresh guard: only a completed reentry scope that contains
this node supersedes its old nested scope. Completion is descriptor-derived. -/
def enclosingReentry (complete contains : Bool) (node enclosing : Nat) : Nat :=
  if complete && contains then enclosing else node

theorem completed_envelope_owns_the_new_visit (node enclosing : Nat) :
    enclosingReentry true true node enclosing = enclosing := rfl

theorem incomplete_envelope_keeps_its_child (contains : Bool) (node enclosing : Nat) :
    enclosingReentry false contains node enclosing = node := by
  cases contains <;> rfl

def observedTree : Tree := .route 101 (.leaf 1) (.route 102 (.leaf 2) (.leaf 3))

/-- A projected arm's first operation, before any runtime branch decision. -/
inductive FirstOperation where
  | send (role : Nat)
  | receive (role : Nat)
  | local (role : Nat)
  deriving DecidableEq

def controlsFromFirst (controller : Nat) : Option FirstOperation → Bool
  | some (.send role) | some (.local role) => decide (role = controller)
  | some (.receive _) | none => false

def containsController (controller : Nat)
    (left right : Option FirstOperation) : Bool :=
  controlsFromFirst controller left || controlsFromFirst controller right

def viewControls (controller view : Nat)
    (left right : Option FirstOperation) : Bool :=
  containsController controller left right && decide (view = controller)

theorem absent_arms_do_not_control (controller : Nat) :
    containsController controller none none = false := rfl

theorem incoming_arms_do_not_control (controller left right : Nat) :
    containsController controller (some (.receive left)) (some (.receive right)) = false := rfl

theorem first_controller_send_establishes_membership
    (controller : Nat) (right : Option FirstOperation) :
    containsController controller (some (.send controller)) right = true := by
  change (decide (controller = controller) || controlsFromFirst controller right) = true
  exact congrArg (fun same : Bool => same || controlsFromFirst controller right)
    (decide_eq_true rfl)

theorem first_controller_local_action_establishes_membership
    (controller : Nat) (right : Option FirstOperation) :
    containsController controller (some (.local controller)) right = true := by
  change (decide (controller = controller) || controlsFromFirst controller right) = true
  exact congrArg (fun same : Bool => same || controlsFromFirst controller right)
    (decide_eq_true rfl)

theorem another_logical_view_has_no_controller_authority
    (controller view : Nat) (left right : Option FirstOperation)
    (different : view ≠ controller) :
    viewControls controller view left right = false := by
  simp [viewControls, different]

theorem controller_view_requires_projected_membership
    (controller : Nat) (left right : Option FirstOperation)
    (absent : containsController controller left right = false) :
    viewControls controller controller left right = false := by
  unfold viewControls
  rw [absent]
  rfl
def observedDecision (_ : Nat) : Option Bool := some true

/-- An unresolved compiled holder choice remains a prerequisite. It cannot be
used to erase a candidate event from completion or grant an operation right. -/
def actorMayRemain (actor : Nat) : Option Nat → Bool
  | none => true
  | some holder => decide (holder = actor)

theorem an_unknown_actor_binding_is_not_excluded (actor : Nat) :
    actorMayRemain actor none = true := rfl

theorem the_selected_local_actor_is_not_excluded (actor : Nat) :
    actorMayRemain actor (some actor) = true := by
  simp [actorMayRemain]

theorem a_selected_foreign_actor_is_excluded
    (actor holder : Nat) (different : holder ≠ actor) :
    actorMayRemain actor (some holder) = false := by
  simp [actorMayRemain, different]

def waitsForOtherViews (held pending : List Nat) : Bool :=
  !pending.isEmpty && !(pending.any fun role => held.contains role)

theorem a_pending_own_operation_cannot_be_waited_out
    (held pending : List Nat) (view : Nat) (owned : view ∈ held) (own : view ∈ pending) :
    waitsForOtherViews held pending = false := by
  have present : (pending.any fun role => held.contains role) = true :=
    List.any_eq_true.mpr ⟨view, own, by simp [owned]⟩
  unfold waitsForOtherViews
  rw [present]
  cases pending.isEmpty <;> rfl

theorem a_pending_payload_operation_cannot_be_waited_out
    (view resource : Nat) (pending : List Nat) (own : resource ∈ pending) :
    waitsForOtherViews [view, resource] pending = false :=
  a_pending_own_operation_cannot_be_waited_out _ _ resource (by simp) own

theorem waiting_for_another_view_never_authorizes_a_commit
    (held pending : List Nat)
    (waiting : waitsForOtherViews held pending = true) :
    pending.isEmpty = false := by
  cases empty : pending.isEmpty <;> simp_all [waitsForOtherViews]

theorem a_finished_predecessor_set_does_not_wait (held : List Nat) :
    waitsForOtherViews held [] = false := rfl

theorem old_first_label_is_an_unchosen_descendant :
    firstLabel (.route 102 (.leaf 2) (.leaf 3)) = 2 := rfl

theorem nested_right_decisions_select_the_actual_leaf :
    select observedDecision observedTree = some (3, [(101, true), (102, true)]) := by
  decide

-- Direct sends may enter a parallel arm at any of its enabled events. Every
-- resolved ancestor still constrains that event; a message-selected child is
-- committed normally but has no resolver audit to publish.
abbrev SendChoiceRow := Option Bool × Bool

def permitsSendRow (row : SendChoiceRow) : Bool :=
  match row.1 with
  | none => true
  | some selected => selected == row.2

def permitsSendPath (rows : List SendChoiceRow) : Bool := rows.all permitsSendRow

def sendResolverAudits (rows : List SendChoiceRow) : List Bool :=
  rows.filterMap fun row => row.1.map fun _ => row.2

theorem every_admitted_send_checks_its_resolved_ancestors
    (rows : List SendChoiceRow) (row : SendChoiceRow) (selected : Bool)
    (member : row ∈ rows) (resolved : row.1 = some selected)
    (admitted : permitsSendPath rows = true) : selected = row.2 := by
  have checked := List.all_eq_true.mp admitted row member
  simpa [permitsSendRow, resolved] using checked

theorem an_intrinsic_child_preserves_its_parents_send_admission
    (arm : Bool) (rows : List SendChoiceRow) :
    permitsSendPath ((none, arm) :: rows) = permitsSendPath rows := by
  simp [permitsSendPath, permitsSendRow]

theorem an_intrinsic_child_has_no_resolver_audit
    (arm : Bool) (rows : List SendChoiceRow) :
    sendResolverAudits ((none, arm) :: rows) = sendResolverAudits rows := by
  simp [sendResolverAudits]

theorem checking_only_the_canonical_entry_can_accept_the_wrong_arm :
    (if false then permitsSendRow (some false, true) else true) = true ∧
    permitsSendPath [(some false, true), (none, false)] = false := by
  decide

def choiceSites : Tree → List Nat
  | .leaf _ => []
  | .route site left right => site :: (choiceSites left ++ choiceSites right)

-- The reset subtree determines which choices can be made again. Completion
-- of an ancestor alone does not erase a choice outside that subtree.
def resumeWithin (body : Tree) (retained fresh : Nat → Option Bool) (site : Nat) : Option Bool :=
  if site ∈ choiceSites body then fresh site else retained site

theorem entering_a_child_preserves_choices_outside_its_body
    (body : Tree) (retained fresh : Nat → Option Bool) (site : Nat)
    (outside : site ∉ choiceSites body) :
    resumeWithin body retained fresh site = retained site := by
  simp [resumeWithin, outside]

theorem every_selected_unreset_ancestor_keeps_its_decision
    (body tree : Tree) (retained fresh : Nat → Option Bool)
    (label : Nat) (path : List (Nat × Bool))
    (selected : select (resumeWithin body retained fresh) tree = some (label, path))
    (site : Nat) (arm : Bool) (member : (site, arm) ∈ path)
    (outside : site ∉ choiceSites body) : retained site = some arm := by
  have decision := every_selected_ancestor_has_its_own_decision
    (resumeWithin body retained fresh) tree label path selected site arm member
  simpa [resumeWithin, outside] using decision

theorem refreshing_a_completed_parent_can_reject_a_valid_child_visit :
    select (resumeWithin (.leaf 221) (fun _ => some false) (fun _ => some true))
      (.route 319 (.leaf 221) (.leaf 222)) = some (221, [(319, false)]) ∧
    select (fun _ => some true) (.route 319 (.leaf 221) (.leaf 222)) =
      some (222, [(319, true)]) := by
  decide

#print axioms entering_a_child_preserves_choices_outside_its_body
#print axioms every_selected_unreset_ancestor_keeps_its_decision
#print axioms refreshing_a_completed_parent_can_reject_a_valid_child_visit

#print axioms every_admitted_send_checks_its_resolved_ancestors
#print axioms an_intrinsic_child_preserves_its_parents_send_admission
#print axioms an_intrinsic_child_has_no_resolver_audit
#print axioms checking_only_the_canonical_entry_can_accept_the_wrong_arm

#print axioms leaf_has_no_decisions
#print axioms rejected_parent_cannot_reach_a_descendant
#print axioms every_selected_ancestor_has_its_own_decision
#print axioms pending_ancestor_precedes_every_descendant
#print axioms coherent_ancestor_preserves_descendant_priority
#print axioms an_unselected_ancestor_is_pending
#print axioms a_selected_matching_ancestor_is_coherent
#print axioms pending_ancestor_realigns_to_its_entry
#print axioms an_actual_entry_keeps_its_cursor
#print axioms completed_envelope_owns_the_new_visit
#print axioms incomplete_envelope_keeps_its_child
#print axioms old_first_label_is_an_unchosen_descendant
#print axioms nested_right_decisions_select_the_actual_leaf
#print axioms absent_arms_do_not_control
#print axioms incoming_arms_do_not_control
#print axioms first_controller_send_establishes_membership
#print axioms first_controller_local_action_establishes_membership
#print axioms another_logical_view_has_no_controller_authority
#print axioms controller_view_requires_projected_membership
#print axioms an_unknown_actor_binding_is_not_excluded
#print axioms the_selected_local_actor_is_not_excluded
#print axioms a_selected_foreign_actor_is_excluded
#print axioms a_pending_own_operation_cannot_be_waited_out
#print axioms a_pending_payload_operation_cannot_be_waited_out
#print axioms waiting_for_another_view_never_authorizes_a_commit
#print axioms a_finished_predecessor_set_does_not_wait
end ControllerOffer
