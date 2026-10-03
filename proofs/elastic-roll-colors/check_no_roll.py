#!/usr/bin/env python3
"""Fresh abstraction of NoRollFastPath.lean; original Z3 source was not retained.

This reproduces the two recorded outcomes, not a byte-identical original query.
Scratch is total/read-only. Source row validation is outside this rewrite.
"""
from z3 import Bool, Function, If, Int, IntSort, Not, Solver, sat, unsat
source = Int('source')
has_roll = Bool('has_roll')
finish = Function('finish', IntSort(), IntSort())
# Total read-only scratch does not alter the returned source.
late = If(has_roll, finish(source), source)
early = If(has_roll, finish(source), source)
for name, constraints, expected in [
    ('equivalence', [late != early], unsat),
    ('nonvacuous no-roll source preserved', [Not(has_roll), early == source], sat),
]:
    solver = Solver()
    solver.add(*constraints)
    actual = solver.check()
    assert actual == expected, (name, actual)
    print(f'{name}: {actual}')
