namespace OrdinaryOfferIngress

inductive Match where
  | missing
  | ambiguous
  | ordinary
  | branch (arm : Bool)

inductive Action where
  | reject
  | restore
  | preview (arm : Bool)
  deriving DecidableEq

def action : Match → Action
  | .missing | .ambiguous => .reject
  | .ordinary => .restore
  | .branch arm => .preview arm

structure State (Payload : Type) where
  position : Nat
  selected : Option Bool
  receive : Nat → List Payload
  offers : List Nat

-- This models successful requeue of the one borrowed frame. Classification
-- assumes an exact, unique enabled user receive from the descriptor matcher.
def restore (s : State Payload) (lane : Nat) (frame : Payload) : State Payload :=
  { s with
    receive := fun candidate =>
      if candidate = lane then frame :: s.receive candidate else s.receive candidate
    offers := s.offers.filter (fun candidate => decide (candidate ≠ lane)) }

theorem ordinary_has_no_branch_authority : action .ordinary = .restore := rfl

theorem preview_requires_unique_branch (m : Match) (arm : Bool)
    (chosen : action m = .preview arm) : m = .branch arm := by
  cases m <;> simp_all [action]

theorem missing_and_ambiguous_reject :
    action .missing = .reject ∧ action .ambiguous = .reject := ⟨rfl, rfl⟩

theorem restore_preserves_progress (s : State Payload) (lane : Nat) (frame : Payload) :
    (restore s lane frame).position = s.position ∧
    (restore s lane frame).selected = s.selected := ⟨rfl, rfl⟩

theorem restore_keeps_exact_frame_at_head (s : State Payload) (lane : Nat) (frame : Payload) :
    (restore s lane frame).receive lane = frame :: s.receive lane := by
  simp [restore]

theorem restore_preserves_other_queue (s : State Payload) (lane other : Nat)
    (frame : Payload) (different : other ≠ lane) :
    (restore s lane frame).receive other = s.receive other := by
  simp [restore, different]

theorem restored_lane_is_not_polled_again (s : State Payload) (lane : Nat) (frame : Payload) :
    lane ∉ (restore s lane frame).offers := by
  simp [restore]

theorem independent_offer_stays_enabled (s : State Payload) (lane other : Nat)
    (frame : Payload) (different : other ≠ lane) (enabled : other ∈ s.offers) :
    other ∈ (restore s lane frame).offers := by
  simp [restore, different, enabled]

#print axioms ordinary_has_no_branch_authority
#print axioms preview_requires_unique_branch
#print axioms missing_and_ambiguous_reject
#print axioms restore_preserves_progress
#print axioms restore_keeps_exact_frame_at_head
#print axioms restore_preserves_other_queue
#print axioms restored_lane_is_not_polled_again
#print axioms independent_offer_stays_enabled
end OrdinaryOfferIngress
