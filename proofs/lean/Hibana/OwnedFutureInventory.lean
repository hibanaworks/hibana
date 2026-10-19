import Hibana.ResourceValueCorrespondence

namespace Hibana
namespace OwnedFutureInventory
open EndpointTransfer

variable {View Frame Borrow : Type}

/-- Copies of fields from an actual owned-send constructor. `Borrow` is the
lexical target, not a newly allocated operation or a runtime ownership table.
The payload tag alone is neither a Rust value nor a callable future. -/
structure Request (View Borrow : Type) where
  carrier : View
  payload : View
  label : Nat
  borrow : Borrow
  deriving Repr, DecidableEq

/-- An erased inventory of real live future objects, including unpolled ones.
Accepted sender witnesses stay here until their Rust future is dropped; they
have no authority. This relation is not inferred from a resident slot phase. -/
def Covers (requests : List (Request View Borrow)) (state : Observation View Frame) : Prop :=
  (requests.map (·.payload)).Perm (state.preparing ++ state.acceptedFutures)

/-- Future-object boundaries and carrier boundaries in the SAME history.
Polls do not imply acceptance, even if they return Pending. The carrier's
actual affine receipt supplies the acceptance step separately. Ordinary
cursor progress can stutter this inventory without reserving a resource.

Application of this model requires complete authentic constructor/drop and
carrier observations. These are erased diagnostics, never scheduling guards. -/
inductive Boundary : Action → List (Request View Borrow) → Observation View Frame →
    List (Request View Borrow) → Observation View Frame → Prop where
  | construct (state : Observation View Frame) (before after : List View)
      (requests : List (Request View Borrow)) (request : Request View Borrow) :
      Boundary .seal requests { state with usable := before ++ request.payload :: after }
        (request :: requests)
        { state with usable := before ++ after, preparing := request.payload :: state.preparing }
  | accept (step : Step .accept before after) :
      Boundary .accept requests before requests after
  | receive (step : Step .receive before after) :
      Boundary .receive requests before requests after
  | retire (state : Observation View Frame) (left right : List View)
      (earlier later : List (Request View Borrow)) (request : Request View Borrow) :
      Boundary .retireAcceptedFuture (earlier ++ request :: later)
        { state with acceptedFutures := left ++ request.payload :: right }
        (earlier ++ later) { state with acceptedFutures := left ++ right }
  | abandon (state : Observation View Frame) (left right : List View)
      (earlier later : List (Request View Borrow)) (request : Request View Borrow) :
      Boundary .abandonPreparation (earlier ++ request :: later)
        { state with preparing := left ++ request.payload :: right }
        (earlier ++ later) { state with preparing := left ++ right }
  | dropValue (step : Step .dropValue before after) :
      Boundary .dropValue requests before requests after
  | fenceFrame (step : Step .fenceFrame before after) :
      Boundary .fenceFrame requests before requests after
  | poll (request : Request View Borrow) (present : request ∈ requests)
      (state : Observation View Frame) : Boundary .unchanged requests state requests state
  | localBoundary (requests : List (Request View Borrow)) (state : Observation View Frame) :
      Boundary .unchanged requests state requests state

inductive History : List (Request View Borrow) → Observation View Frame →
    List (Request View Borrow) → Observation View Frame → Prop where
  | refl (requests state) : History requests state requests state
  | tail : History firstRequests first middleRequests middle →
      Boundary action middleRequests middle lastRequests last →
      History firstRequests first lastRequests last

end OwnedFutureInventory

open OwnedFutureInventory EndpointTransfer
variable {View Frame Borrow : Type}

theorem OwnedFutureInventory.a_future_boundary_erases_to_the_same_transfer_step
    (boundary : Boundary action first before last after) : Step action before after := by
  cases boundary with
  | construct state left right requests request => exact .seal state left right request.payload
  | accept step | receive step | dropValue step | fenceFrame step => exact step
  | retire state left right earlier later request =>
      exact .retireAcceptedFuture state left right request.payload
  | abandon state left right earlier later request =>
      exact .abandonPreparation state left right request.payload
  | poll | localBoundary => exact .unchanged _

private theorem move_to_front (left right tail : List View) (value : View) :
    ((left ++ value :: right) ++ tail).Perm (value :: ((left ++ right) ++ tail)) :=
  by simpa only [List.append_assoc] using
    (List.perm_middle : (left ++ value :: (right ++ tail)).Perm
      (value :: (left ++ (right ++ tail))))

private theorem remove_request
    (earlier later : List (Request View Borrow)) (request : Request View Borrow)
    (rest before : List View)
    (inventory : ((earlier ++ request :: later).map (·.payload)).Perm before)
    (removed : before.Perm (request.payload :: rest)) :
    ((earlier ++ later).map (·.payload)).Perm rest := by
  have brought :
      ((earlier ++ request :: later).map (·.payload)).Perm
        (request.payload :: ((earlier ++ later).map (·.payload))) := by
    simpa only [List.map_append, List.map_cons] using
      (List.perm_middle :
        (earlier.map (·.payload) ++ request.payload :: later.map (·.payload)).Perm
          (request.payload :: (earlier.map (·.payload) ++ later.map (·.payload))))
  exact List.Perm.cons_inv (brought.symm.trans (inventory.trans removed))

theorem OwnedFutureInventory.actual_future_boundaries_preserve_the_complete_inventory
    (boundary : Boundary action first before last after) (inventory : Covers first before) :
    Covers last after := by
  cases boundary with
  | construct => exact List.Perm.cons _ inventory
  | accept step =>
      cases step with
      | accept state left right value frame =>
          exact inventory.trans ((move_to_front left right state.acceptedFutures value).trans
            (List.perm_middle : ((left ++ right) ++ value :: state.acceptedFutures).Perm
              (value :: ((left ++ right) ++ state.acceptedFutures))).symm)
  | receive step => cases step; exact inventory
  | retire state left right earlier later request =>
      exact remove_request earlier later request _ _ inventory
        (List.Perm.append_left state.preparing
          (List.perm_middle : (left ++ request.payload :: right).Perm
            (request.payload :: (left ++ right))) |>.trans List.perm_middle)
  | abandon state left right earlier later request =>
      exact remove_request earlier later request _ _ inventory
        (move_to_front left right state.acceptedFutures request.payload)
  | dropValue step => cases step; exact inventory
  | fenceFrame step => cases step; exact inventory
  | poll | localBoundary => exact inventory

theorem OwnedFutureInventory.a_complete_future_history_erases_to_its_transfer_history
    (history : History firstRequests first lastRequests last) : Execution first last := by
  induction history with
  | refl => exact .refl _
  | tail _ boundary ih => exact .tail ih (a_future_boundary_erases_to_the_same_transfer_step boundary)

theorem OwnedFutureInventory.a_complete_future_history_covers_every_private_payload
    {requests : List (Request View Borrow)}
    (history : History [] (initial initialValue : Observation View Frame) requests state) :
    Covers requests state := by
  have preserve : ∀ {firstRequests first lastRequests last},
      History (View := View) (Frame := Frame) (Borrow := Borrow)
        firstRequests first lastRequests last →
      Covers firstRequests first → Covers lastRequests last := by
    intro firstRequests first lastRequests last execution covered
    induction execution with
    | refl => exact covered
    | tail _ boundary ih => exact actual_future_boundaries_preserve_the_complete_inventory boundary ih
  exact preserve history (List.Perm.refl [])

/-- A preparing authority has an actual live constructor request, with its
carrier, message label and lexical borrow. Source readiness and issued tags
are not hypotheses that can invent such an object. This is an availability
component, not a theorem that a blocked carrier eventually returns Ready. -/
theorem OwnedFutureInventory.a_private_payload_has_its_actual_request
    (history : History [] (initial initialValue : Observation View Frame) requests state)
    (preparing : value ∈ state.preparing) :
    ∃ request, request ∈ requests ∧ request.payload = value := by
  have present := (a_complete_future_history_covers_every_private_payload history).mem_iff.mpr
    (List.mem_append.mpr (.inl preparing))
  exact List.mem_map.mp present

theorem OwnedFutureInventory.actual_future_history_preserves_unique_authority
    (history : History [] (initial initialValue : Observation View Frame) requests state) :
    state.authorityCount ≤ 1 :=
  one_binding_has_at_most_one_transferable_authority
    (a_complete_future_history_erases_to_its_transfer_history history)

end Hibana
