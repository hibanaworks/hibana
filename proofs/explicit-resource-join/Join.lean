import Hibana.GlobalSemantics

namespace ExplicitResourceJoin
open Hibana

/-- The normal communication path of tests/explicit_resource_join.rs.
Physical IO completion is an integration premise, not a property inferred
from a message label by GlobalSemantics. -/
def choreography : Choreo := .seq
  (.par
    (.seq (.send 2 0 0 0) (.route .intrinsic (.send 0 2 1 0) (.send 0 2 2 0)))
    (.seq (.send 2 1 3 0) (.route .intrinsic (.send 1 2 4 0) (.send 1 2 5 0))))
  (.seq (.route .intrinsic (.send 2 3 6 0) (.send 2 3 7 0)) (.send 3 2 8 0))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 4 choreography) trace).isSome

def requests : List GlobalOperation := [.send 0, .recv 0, .send 3, .recv 3]
def rx : List GlobalOperation := [.send 1, .recv 1]
def tx : List GlobalOperation := [.send 4, .recv 4]
def returned : List GlobalOperation := [.send 6, .recv 6, .send 8, .recv 8]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem actual_join_can_return : accepted (requests ++ rx ++ tx ++ returned) = true := by decide
theorem reverse_completion_can_return : accepted (requests ++ tx ++ rx ++ returned) = true := by decide
theorem rx_alone_cannot_return : accepted (requests ++ rx ++ [.send 6]) = false := by decide
theorem tx_acceptance_cannot_return :
    accepted (requests ++ rx ++ [.send 4, .send 6]) = false := by decide
theorem return_cannot_be_duplicated :
    accepted (requests ++ rx ++ tx ++ returned ++ [.send 6]) = false := by decide

/-- Application-level completion evidence. Rust tests tie construction to
actual receives and operation identity. This model does not verify arbitrary
Rust or the native IO backend. -/
structure Completion where
  operation : Nat
  settled : Bool
  successful : Bool
  received : Bool

def normalReturn (current : Nat) (rx tx : Completion) : Prop :=
  rx.operation = current ∧ tx.operation = current ∧
  rx.settled = true ∧ tx.settled = true ∧
  rx.successful = true ∧ tx.successful = true ∧
  rx.received = true ∧ tx.received = true

theorem return_requires_both_settled (current : Nat) (rx tx : Completion)
    (h : normalReturn current rx tx) : rx.settled = true ∧ tx.settled = true :=
  ⟨h.2.2.1, h.2.2.2.1⟩

theorem stale_receive_rejected (current : Nat) (rx tx : Completion)
    (h : rx.operation ≠ current) : ¬ normalReturn current rx tx := by
  intro joined
  exact h joined.1

theorem failed_transmit_rejected (current : Nat) (rx tx : Completion)
    (h : tx.successful = false) : ¬ normalReturn current rx tx := by
  intro joined
  have yes := joined.2.2.2.2.2.1
  simp [h] at yes

/-- Physical stop has no completion argument. Only subsequent return/reuse
requires settling. This expresses the design obligation, not stop latency. -/
def stop (_ : Bool) : Bool := true
theorem stop_independent_of_join (joined : Bool) : stop joined = true := rfl

#print axioms actual_join_can_return
#print axioms reverse_completion_can_return
#print axioms rx_alone_cannot_return
#print axioms tx_acceptance_cannot_return
#print axioms return_cannot_be_duplicated
#print axioms return_requires_both_settled
#print axioms stale_receive_rejected
#print axioms failed_transmit_rejected
#print axioms stop_independent_of_join
end ExplicitResourceJoin
