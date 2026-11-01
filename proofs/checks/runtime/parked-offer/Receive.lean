import Std

/-! A parked receive frame retains its port's Rx borrow independently of the
public operation lease. This model specifies lane exclusion and the final
Pending/OutOfOrder decision of recv::poll_recv_frame_source. It does not prove
Rust pointer validity or source-to-image refinement; real Endpoint regressions,
generated admission probes and Miri check that integration. -/
namespace ParkedReceive

def pollable (retained : Option Nat) (lane : Nat) : Bool :=
  decide (retained ≠ some lane)

def available (retained : Option Nat) (lanes : List Nat) : List Nat :=
  lanes.filter (pollable retained)

theorem a_retained_port_is_never_polled (lane : Nat) :
    pollable (some lane) lane = false := by
  simp [pollable]

theorem an_independent_port_remains_pollable (held lane : Nat)
    (distinct : held ≠ lane) : pollable (some held) lane = true := by
  simp [pollable, distinct]

theorem an_unborrowed_port_remains_pollable (lane : Nat) :
    pollable none lane = true := by
  simp [pollable]

theorem scanning_excludes_the_retained_port (held : Nat) (lanes : List Nat) :
    held ∉ available (some held) lanes := by
  simp [available, pollable]

theorem scanning_preserves_every_independent_candidate
    (held lane : Nat) (lanes : List Nat) (distinct : held ≠ lane) :
    lane ∈ available (some held) lanes ↔ lane ∈ lanes := by
  simp [available, pollable, distinct]

theorem scanning_introduces_no_candidate
    (retained : Option Nat) (lane : Nat) (lanes : List Nat)
    (present : lane ∈ available retained lanes) : lane ∈ lanes := by
  exact (List.mem_filter.mp present).1

def outOfOrder (excluded pending : Bool) : Bool := excluded && !pending

theorem only_excluded_candidates_are_rejected :
    outOfOrder true false = true := by decide

theorem pending_independent_input_is_not_rejected (excluded : Bool) :
    outOfOrder excluded true = false := by
  simp [outOfOrder]

theorem waiting_without_a_retained_frame_is_not_rejected (pending : Bool) :
    outOfOrder false pending = false := by
  simp [outOfOrder]

#print axioms a_retained_port_is_never_polled
#print axioms an_independent_port_remains_pollable
#print axioms an_unborrowed_port_remains_pollable
#print axioms scanning_excludes_the_retained_port
#print axioms scanning_preserves_every_independent_candidate
#print axioms scanning_introduces_no_candidate
#print axioms only_excluded_candidates_are_rejected
#print axioms pending_independent_input_is_not_rejected
#print axioms waiting_without_a_retained_frame_is_not_rejected
end ParkedReceive
