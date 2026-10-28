import Std

/-! The collecting continuation in endpoint/kernel/offer.rs. Frame is opaque:
the staged value includes its existing transport observation and payload lease.
This model proves the guards and handoff, not arbitrary Rust effects or transport
fairness. Exact admission still belongs to the existing Selecting continuation.
-/
namespace Hibana.ParallelOfferIngress

inductive Observation (Frame : Type) where
  | pending
  | frame (value : Frame)
  | failed (error : Nat)
  deriving DecidableEq

inductive Collection (Frame : Type) where
  | waiting
  | evidence (ingress : Option Frame)
  | failed (error : Nat)
  deriving DecidableEq

-- poll_collect_offer_evidence never waits after an ingress has been acquired.
def collect {Frame : Type} (owned : Option Frame) (selected : Collection Frame) :
    Collection Frame :=
  match owned with
  | some frame => .evidence (some frame)
  | none => selected

inductive Continuation (Frame : Type) where
  | waiting
  | selecting (ingress : Frame)
  | resolving (ingress : Option Frame)
  | failed (error : Nat)
  deriving DecidableEq

structure Step (Frame : Type) where
  next : Continuation Frame
  context : Nat
  inspectedActive : Bool

-- context represents the unchanged FrontierVisitSet moved by take(). The active
-- observation is used only after collect returned Pending with empty ingress.
def advance {Frame : Type} (context : Nat) (owned : Option Frame)
    (selected : Collection Frame) (active : Observation Frame) : Step Frame :=
  match collect owned selected with
  | .evidence ingress => ⟨.resolving ingress, context, false⟩
  | .failed error => ⟨.failed error, context, false⟩
  | .waiting =>
    match active with
    | .pending => ⟨.waiting, context, true⟩
    | .frame frame => ⟨.selecting frame, context, true⟩
    | .failed error => ⟨.failed error, context, true⟩

-- The existing active-lane scan polls in descriptor order; a failed observation
-- terminates before later lanes, and absence supplies no artificial frame.
def scan {Frame : Type} : List (Observation Frame) → Observation Frame
  | [] => .pending
  | .pending :: rest => scan rest
  | .frame frame :: _ => .frame frame
  | .failed error :: _ => .failed error

theorem owned_is_evidence {Frame : Type} (frame : Frame) (selected : Collection Frame) :
    collect (some frame) selected = .evidence (some frame) := rfl

theorem waiting_requires_empty {Frame : Type} (owned : Option Frame)
    (selected : Collection Frame) (waiting : collect owned selected = .waiting) :
    owned = none := by
  cases owned <;> simp_all [collect]

theorem carried_frame_bypasses_active {Frame : Type} (context : Nat) (frame : Frame)
    (selected : Collection Frame) (active : Observation Frame) :
    advance context (some frame) selected active =
      Step.mk (.resolving (some frame)) context false := rfl

theorem selected_evidence_bypasses_active {Frame : Type} (context : Nat)
    (ingress : Option Frame) (active : Observation Frame) :
    advance context none (.evidence ingress) active =
      Step.mk (.resolving ingress) context false := rfl

theorem selected_failure_bypasses_active {Frame : Type} (context error : Nat)
    (active : Observation Frame) :
    advance context none (.failed error) active = Step.mk (.failed error) context false := rfl

theorem independent_frame_is_carried {Frame : Type} (context : Nat) (frame : Frame) :
    advance context none .waiting (.frame frame) = Step.mk (.selecting frame) context true := rfl

theorem no_actual_frame_keeps_waiting {Frame : Type} (context : Nat) :
    advance (Frame := Frame) context none .waiting .pending =
      Step.mk .waiting context true := rfl

theorem active_failure_is_terminal {Frame : Type} (context error : Nat) :
    advance (Frame := Frame) context none .waiting (.failed error) =
      Step.mk (.failed error) context true := rfl

theorem context_is_preserved {Frame : Type} (context : Nat) (owned : Option Frame)
    (selected : Collection Frame) (active : Observation Frame) :
    (advance context owned selected active).context = context := by
  cases owned <;> cases selected <;> cases active <;> rfl

theorem selecting_requires_actual_empty {Frame : Type} (context : Nat)
    (owned : Option Frame) (selected : Collection Frame) (active : Observation Frame)
    (frame : Frame) (selectedFrame : (advance context owned selected active).next = .selecting frame) :
    owned = none ∧ selected = .waiting ∧ active = .frame frame := by
  cases owned <;> cases selected <;> cases active <;> simp_all [advance, collect]

theorem scan_frame_was_observed {Frame : Type} (inputs : List (Observation Frame))
    (frame : Frame) (received : scan inputs = .frame frame) :
    Observation.frame frame ∈ inputs := by
  induction inputs with
  | nil => simp [scan] at received
  | cons first rest ih =>
    cases first with
    | pending => exact List.mem_cons_of_mem _ (ih received)
    | frame value =>
      simp only [scan, Observation.frame.injEq] at received
      subst value
      exact List.mem_cons_self
    | failed error => simp [scan] at received

theorem pending_prefix_preserves_scan {Frame : Type}
    (leading suffix : List (Observation Frame))
    (pending : ∀ observation ∈ leading, observation = .pending) :
    scan (leading ++ suffix) = scan suffix := by
  induction leading with
  | nil => rfl
  | cons first rest ih =>
    have firstPending := pending first (List.mem_cons_self)
    have restPending : ∀ observation ∈ rest, observation = .pending := by
      intro observation member
      exact pending observation (List.mem_cons_of_mem first member)
    subst first
    simp only [List.cons_append, scan]
    exact ih restPending

theorem first_failure_precedes_later_frame {Frame : Type} (error : Nat)
    (leading rest : List (Observation Frame))
    (pending : ∀ observation ∈ leading, observation = .pending) :
    scan (leading ++ (.failed error :: rest)) = .failed error := by
  rw [pending_prefix_preserves_scan leading (.failed error :: rest) pending]
  rfl

-- An observed frame must be considered even without a selected continuation.
theorem observed_frame_enables_selection :
    collect (Frame := Nat) none .waiting = .waiting ∧
    (advance 17 none .waiting (.frame 42)).next = .selecting 42 := by
  exact ⟨rfl, rfl⟩

-- Resolving keeps the unique enabled receive selected from the carried frame.
-- A cached child may describe another parallel lane in the same selected arm.
inductive PassivePreview where
  | receive (row : Nat)
  | descend (scope : Nat)
  | ready
  deriving DecidableEq

def passivePreview (exactTarget child : Option Nat) : PassivePreview :=
  match exactTarget with
  | some row => .receive row
  | none => match child with
    | some scope => .descend scope
    | none => .ready

theorem an_exact_receive_keeps_its_row (row : Nat) (child : Option Nat) :
    passivePreview (some row) child = .receive row := rfl

theorem an_exact_receive_cannot_restart_at_a_child (row scope : Nat) :
    passivePreview (some row) (some scope) ≠ .descend scope := by
  simp [passivePreview]

theorem passive_descent_requires_an_unresolved_target (target child : Option Nat)
    (scope : Nat) (descended : passivePreview target child = .descend scope) :
    target = none ∧ child = some scope := by
  cases target <;> cases child <;> simp_all [passivePreview]

theorem an_unresolved_target_can_descend (scope : Nat) :
    passivePreview none (some scope) = .descend scope := rfl

-- A lane mask belongs to one poll only. Repeated Pending observations can be
-- omitted within this cut; the transport must wake the next poll on readiness.
-- This does not keep admission or transport observations across polls.
def uninspected {Frame : Type} (seen : Nat → Bool)
    (inputs : List (Nat × Observation Frame)) : List (Nat × Observation Frame) :=
  inputs.filter fun entry => !seen entry.1

theorem an_uninspected_direction_remains {Frame : Type} (seen : Nat → Bool)
    (inputs : List (Nat × Observation Frame)) (entry : Nat × Observation Frame)
    (present : entry ∈ inputs) (fresh : seen entry.1 = false) :
    entry ∈ uninspected seen inputs := by
  simp [uninspected, present, fresh]

theorem an_inspected_direction_is_not_repeated {Frame : Type} (seen : Nat → Bool)
    (inputs : List (Nat × Observation Frame)) (entry : Nat × Observation Frame)
    (inspected : seen entry.1 = true) :
    entry ∉ uninspected seen inputs := by
  simp [uninspected, inspected]

theorem skipping_pending_directions_preserves_scan {Frame : Type} (seen : Nat → Bool)
    (inputs : List (Nat × Observation Frame))
    (pending : ∀ entry ∈ inputs, seen entry.1 = true → entry.2 = .pending) :
    scan ((uninspected seen inputs).map Prod.snd) = scan (inputs.map Prod.snd) := by
  induction inputs with
  | nil => rfl
  | cons entry rest ih =>
    have restPending : ∀ item ∈ rest, seen item.1 = true → item.2 = .pending := by
      intro item member inspected
      exact pending item (List.mem_cons_of_mem entry member) inspected
    by_cases inspected : seen entry.1 = true
    · have observed := pending entry List.mem_cons_self inspected
      simp only [uninspected, List.filter_cons, inspected, Bool.not_true,
        Bool.false_eq_true, ↓reduceIte, List.map_cons, observed, scan]
      exact ih restPending
    · have fresh : seen entry.1 = false := by
        cases value : seen entry.1 <;> simp_all
      cases entry with
      | mk lane observation =>
        cases observation <;> simp_all [uninspected, scan]

#print axioms owned_is_evidence
#print axioms waiting_requires_empty
#print axioms carried_frame_bypasses_active
#print axioms selected_evidence_bypasses_active
#print axioms selected_failure_bypasses_active
#print axioms independent_frame_is_carried
#print axioms no_actual_frame_keeps_waiting
#print axioms active_failure_is_terminal
#print axioms context_is_preserved
#print axioms selecting_requires_actual_empty
#print axioms scan_frame_was_observed
#print axioms pending_prefix_preserves_scan
#print axioms first_failure_precedes_later_frame
#print axioms observed_frame_enables_selection
#print axioms an_exact_receive_keeps_its_row
#print axioms an_exact_receive_cannot_restart_at_a_child
#print axioms passive_descent_requires_an_unresolved_target
#print axioms an_unresolved_target_can_descend
#print axioms an_uninspected_direction_remains
#print axioms an_inspected_direction_is_not_repeated
#print axioms skipping_pending_directions_preserves_scan

end Hibana.ParallelOfferIngress
