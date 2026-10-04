#!/usr/bin/env python3
"""Bounded endpoint/IO integration model, not an arbitrary-Rust proof."""
from z3 import And, Bool, Implies, Int, Not, Solver, sat, unsat

current, rx_op, tx_op = (Int(n) for n in ("current", "rx_op", "tx_op"))
rx_settled, tx_settled, rx_received, tx_received, rx_ok, tx_ok = (
    Bool(n) for n in ("rx_settled", "tx_settled", "rx_received", "tx_received", "rx_ok", "tx_ok")
)
# Completion constructors are an explicit integration obligation: actual IO
# must have settled before its notification can be received.
premise = And(Implies(rx_received, rx_settled), Implies(tx_received, tx_settled))
joined = And(rx_received, tx_received, rx_ok, tx_ok, rx_op == current, tx_op == current)

def check(name, condition, expected, premises=True):
    solver = Solver()
    if premises:
        solver.add(premise)
    solver.add(condition)
    result = solver.check()
    assert result == expected, (name, result, solver)
    print(name, result)

check("normal-path-is-reachable", joined, sat)
check("RX-only-is-not-normal-return", And(joined, Not(tx_received)), unsat)
check("acceptance-is-not-completion", And(joined, Not(tx_settled)), unsat)
check("failure-is-not-normal-return", And(joined, Not(tx_ok)), unsat)
check("old-RX-operation-is-not-current", And(joined, rx_op != current), unsat)
check("old-TX-operation-is-not-current", And(joined, tx_op != current), unsat)
check("weakened-RX-only-rule-is-unsafe",
      And(rx_received, rx_ok, rx_op == current, Not(tx_settled)), sat)
check("fabricated-completion-breaks-IO-guarantee",
      And(joined, Not(tx_settled)), sat, premises=False)

# Single extraction is modeled as consuming the sole owned resource.
owned0 = Int("owned0")
owned1 = owned0 - 1
owned2 = owned1 - 1
check("two-successful-extractions-from-one-owner",
      And(owned0 == 1, owned0 > 0, owned1 > 0, owned2 >= 0), unsat)
# Independent stop is possible without either completion; it does not authorize reuse.
stopped = Bool("stopped")
check("emergency-stop-before-join",
      And(stopped, Not(rx_received), Not(tx_received), Not(joined)), sat)
print("Explicit resource join: 6 UNSAT obligations, 4 SAT witnesses.")
