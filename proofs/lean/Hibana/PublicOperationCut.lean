import Hibana.DescriptorTopology
import Hibana.PublicOperationKernel

namespace Hibana

/-- Read-only copies of all resident lane slots, not another runtime registry. -/
structure OperationSlotObservation where
  view : Nat
  lane : Nat
  phase : PublicOperationPhase
  deriving Repr, DecidableEq

/-- Copied from the actual operation handle. A slot itself does not store the
borrow region: the same slot may later serve a different lexical borrow. -/
structure OperationBorrowObservation where
  view : Nat
  lane : Nat
  whole : Bool
  start : Nat
  stop : Nat
  ownedLanes : List Nat
  otherActive : Bool
  laneMaskOffset : Option Nat := none
  deriving Repr, DecidableEq

structure PublicOperationCut where
  views : List Nat
  slots : List OperationSlotObservation
  target : OperationBorrowObservation
  deriving Repr, DecidableEq

def OperationSlotObservation.key (slot : OperationSlotObservation) : Nat × Nat :=
  (slot.view, slot.lane)

/-- The production attachment's immutable view roster: the actor itself,
initially assigned resources, visible logical roles and endpoint payload roles.
An observer cannot choose a smaller roster to hide a resident operation. -/
def RustDescriptorImage.operationViews? (image : RustDescriptorImage) : Option (List Nat) := do
  let columns ← image.decodeProgramColumns?
  let initial := columns.initialActors.filterMap fun (view, actor) =>
    if actor == image.role then some view else none
  let visible ← (List.range image.eventCount).mapM fun row => do
    let view ← image.eventView? row
    let atom ← image.eventAtom? row
    if atom.schema < 4294967296 then pure [view]
    else if atom.schema < 4294967552 then pure [view, atom.schema - 4294967296]
    else none
  pure (canonicalParticipants (image.role :: initial ++ visible.flatten))

/-- Same maximum-lane-plus-one allocation as `view_lane_slot_count`, including
the otherwise empty lane-zero session membership slot. -/
def RustDescriptorImage.operationSlotCount (image : RustDescriptorImage) (view : Nat) : Nat :=
  (List.range image.eventCount).foldl (fun count row =>
    if image.eventView? row = some view then
      match image.eventLane? row with
      | some lane => max count (lane + 1)
      | none => count
    else count) 1

def RustDescriptorImage.operationSlotKeys
    (image : RustDescriptorImage) (views : List Nat) : List (Nat × Nat) :=
  views.flatMap fun view => (List.range (image.operationSlotCount view)).map (view, ·)

/-- Independent membership specification from descriptor rows and the actual
source interval. The production lookup's cached reference is checked below;
no observed lane mask is allowed to become its own authority. -/
def RustDescriptorImage.borrowOwnedLanes
    (image : RustDescriptorImage) (target : OperationBorrowObservation) : List Nat :=
  (List.range (image.operationSlotCount target.view)).filter fun lane =>
    target.whole || (List.range image.eventCount).any fun row =>
      image.eventView? row == some target.view && image.eventLane? row == some lane &&
        match image.eventEffIndex? row with
        | none => false
        | some eff => decide (target.start ≤ eff ∧ eff < target.stop)

def PublicOperationCut.hasOtherActive (cut : PublicOperationCut) : Bool :=
  cut.slots.any fun slot =>
    slot.view == cut.target.view && slot.lane != cut.target.lane &&
      slot.phase != .idle && cut.target.ownedLanes.contains slot.lane

def PublicOperationCut.maskReferenceMatches
    (cut : PublicOperationCut) (image : RustDescriptorImage) : Bool :=
  if cut.target.whole then cut.target.laneMaskOffset == none
  else match cut.target.laneMaskOffset with
    | none => false
    | some offset => match image.checkedParallelBorrowMaskAt? offset with
      | none => false
      | some mask => mask.view == cut.target.view && mask.start == cut.target.start &&
          mask.stop == cut.target.stop && mask.lanes == cut.target.ownedLanes

/-- Check the whole immutable view/slot inventory, not a caller-selected subset.
This establishes neither the existence of a Rust future nor I/O liveness. -/
def PublicOperationCut.matches (cut : PublicOperationCut) (image : RustDescriptorImage) : Bool :=
  image.operationViews? == some cut.views &&
    cut.slots.map OperationSlotObservation.key == image.operationSlotKeys cut.views &&
    cut.slots.any (fun slot => slot.key == (cut.target.view, cut.target.lane)) &&
    decide (cut.target.start < cut.target.stop ∧ cut.target.stop ≤ 65535) &&
    cut.target.ownedLanes == image.borrowOwnedLanes cut.target &&
    cut.target.otherActive == cut.hasOtherActive && cut.maskReferenceMatches image

def PublicOperationCut.targetPhase? (cut : PublicOperationCut) : Option PublicOperationPhase :=
  (cut.slots.find? fun slot => slot.key == (cut.target.view, cut.target.lane)).map (·.phase)

/-- Local fresh-call guard only. Session fault, source readiness, payload
ownership and availability of the actual future are separate obligations. -/
def PublicOperationCut.permitsFreshCall (cut : PublicOperationCut)
    (edge : PublicOperationEdge) : Bool :=
  (edge == .beginSend || edge == .beginRecv || edge == .beginOffer) &&
    !cut.hasOtherActive &&
      match cut.targetPhase? with
      | none => false
      | some phase => (transitionPublicOperation phase edge).lease == .held

theorem a_checked_operation_cut_uses_the_immutable_view_inventory
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true) :
    image.operationViews? = some cut.views := by
  simp only [PublicOperationCut.matches, Bool.and_eq_true, beq_iff_eq,
    decide_eq_true_eq] at checked
  exact checked.1.1.1.1.1.1

theorem a_checked_operation_cut_covers_every_resident_slot
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true) :
    cut.slots.map OperationSlotObservation.key = image.operationSlotKeys cut.views := by
  simp only [PublicOperationCut.matches, Bool.and_eq_true, beq_iff_eq,
    decide_eq_true_eq] at checked
  exact checked.1.1.1.1.1.2

theorem a_checked_operation_cut_uses_the_descriptor_borrow_mask
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true) :
    cut.target.ownedLanes = image.borrowOwnedLanes cut.target := by
  simp only [PublicOperationCut.matches, Bool.and_eq_true, beq_iff_eq,
    decide_eq_true_eq] at checked
  exact checked.1.1.2

theorem a_checked_operation_cut_matches_the_other_active_guard
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true) :
    cut.target.otherActive = cut.hasOtherActive := by
  simp only [PublicOperationCut.matches, Bool.and_eq_true, beq_iff_eq,
    decide_eq_true_eq] at checked
  exact checked.1.2

theorem a_checked_operation_cut_binds_its_cached_lane_reference
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true) : cut.maskReferenceMatches image = true := by
  simp only [PublicOperationCut.matches, Bool.and_eq_true] at checked
  exact checked.2

/-- Lifts a real idle target through the fresh-send operation guard. This is
one local component of enabled-step lifting, not the distributed theorem. -/
theorem an_idle_independent_borrow_can_begin_its_send
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true)
    (idle : cut.targetPhase? = some .idle)
    (clear : cut.target.otherActive = false) :
    cut.permitsFreshCall .beginSend = true := by
  have guard := a_checked_operation_cut_matches_the_other_active_guard checked
  rw [clear] at guard
  simp [PublicOperationCut.permitsFreshCall, ← guard, idle,
    transitionPublicOperation, PublicOperationEdge.expected, PublicOperationEdge.next]

theorem a_busy_borrow_cannot_begin_a_fresh_call
    {cut : PublicOperationCut} {image : RustDescriptorImage}
    (checked : cut.matches image = true)
    (busy : cut.target.otherActive = true) (edge : PublicOperationEdge) :
    cut.permitsFreshCall edge = false := by
  have guard := a_checked_operation_cut_matches_the_other_active_guard checked
  rw [busy] at guard
  simp [PublicOperationCut.permitsFreshCall, ← guard]

end Hibana
