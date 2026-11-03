import Hibana.GlobalSemantics

namespace ExplicitResourceJoin
open Hibana

/-- The logical communication order of tests/explicit_resource_join.rs.
The Endpoint payloads are represented here as sends: this fixture proves the
join order, while affine custody is checked by the real owned Endpoint runs.
Physical IO completion and result identity remain integration premises. -/
def choreography : Choreo := .seq
  (.par
    (.seq (.send 2 0 9 0)
      (.seq (.send 2 4 0 0)
        (.seq (.route .intrinsic (.send 4 2 1 0) (.send 4 2 2 0))
          (.send 0 2 9 0))))
    (.seq (.send 2 1 10 0)
      (.seq (.send 2 5 3 0)
        (.seq (.route .intrinsic (.send 5 2 4 0) (.send 5 2 5 0))
          (.send 1 2 10 0)))))
  (.route .intrinsic
    (.seq (.send 2 3 6 0)
      (.seq (.send 2 3 9 0)
        (.seq (.send 2 3 10 0)
          (.seq (.par (.send 4 2 11 0) (.send 5 2 12 0))
            (.send 3 2 8 0)))))
    (.send 2 3 7 0))

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def accepted (trace : List GlobalOperation) : Bool :=
  (run (GlobalConfig.initial 1 6 choreography) trace).isSome

def requests : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .send 5, .recv 5, .send 6, .recv 6]
def rx : List GlobalOperation := [.send 2, .recv 2, .send 4, .recv 4]
def tx : List GlobalOperation := [.send 7, .recv 7, .send 9, .recv 9]
def returned : List GlobalOperation :=
  [.send 10, .recv 10, .send 11, .recv 11, .send 12, .recv 12,
   .send 13, .recv 13, .send 14, .recv 14, .send 15, .recv 15]

set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem actual_join_can_return : accepted (requests ++ rx ++ tx ++ returned) = true := by decide
theorem reverse_completion_can_return : accepted (requests ++ tx ++ rx ++ returned) = true := by decide
theorem rx_alone_cannot_return : accepted (requests ++ rx ++ [.send 10]) = false := by decide
theorem tx_acceptance_cannot_return :
    accepted (requests ++ rx ++ [.send 7, .send 10]) = false := by decide
theorem loan_acceptance_cannot_return :
    accepted (requests ++ rx ++ [.send 7, .recv 7, .send 9, .send 10]) = false := by decide
theorem both_loans_must_be_received :
    accepted (requests ++ [.send 2, .recv 2, .send 4, .send 7, .recv 7, .send 9, .send 10])
      = false := by decide
theorem return_cannot_be_duplicated :
    accepted (requests ++ rx ++ tx ++ returned ++ [.send 10]) = false := by decide

/-- Application-level completion evidence. Rust tests tie construction to
actual result receives, returned Endpoint loans, and operation identity. This model does not verify arbitrary
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

/-- The emergency-stop fixture has a resident command, a stop message, and
an unfinished IO completion preceding reuse. The stop and resident command
are both enabled without the IO completion; no latency bound is asserted. -/
def stopChoreography : Choreo := .par
  (.roll (.send 0 1 20 0))
  (.par (.send 2 3 21 0) (.seq (.send 4 3 22 0) (.send 3 5 23 0)))

theorem stop_independent_of_join :
    (run (GlobalConfig.initial 1 6 stopChoreography)
      [.send 1, .recv 1, .send 0, .recv 0]).isSome = true := by decide

#print axioms actual_join_can_return
#print axioms reverse_completion_can_return
#print axioms rx_alone_cannot_return
#print axioms tx_acceptance_cannot_return
#print axioms loan_acceptance_cannot_return
#print axioms both_loans_must_be_received
#print axioms return_cannot_be_duplicated
#print axioms return_requires_both_settled
#print axioms stale_receive_rejected
#print axioms failed_transmit_rejected
#print axioms stop_independent_of_join
end ExplicitResourceJoin
