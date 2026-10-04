"""Bounded SMT model of read-only failure reporting, not the Rust compiler."""
from z3 import And, Bool, If, IntVal, Not, Or, Solver, unsat

for count in range(1, 33):
    obligations = [Bool(f"ok_{i}") for i in range(count)]
    first = IntVal(-1)
    for index in reversed(range(count)):
        first = If(obligations[index], first, index)
    solver = Solver()
    solver.add(Or((first == -1) != And(*obligations),
                  And(first != -1, Or(first < 0, first >= count)),
                  Or(*[And(first == i, obligations[i]) for i in range(count)]),
                  Or(*[And(first == i, Not(obligations[j]))
                       for i in range(count) for j in range(i)])))
    assert solver.check() == unsat, count
print("32 bounded SMT checks: acceptance unchanged; witness is the first failed obligation")
