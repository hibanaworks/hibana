#!/usr/bin/env python3
"""Independent Z3 encoding of the Rust left-to-right lookup and certificate.
No solver result named a proof is accepted unless it is UNSAT; SAT witnesses
cover useful success, fallback, malformed, and a deliberately unsound shortcut.
"""
from z3 import *

CAPACITY = 8192
INVALID, ABSENT = -2, -1

def step(query, slot, raw, found):
    return If(found == INVALID, INVALID,
        If(Or(raw < 0, raw >= CAPACITY), INVALID,
            If(raw == query, If(found == ABSENT, slot, INVALID), found)))

def scan(rows, query):
    found = IntVal(ABSENT)
    for slot, raw in enumerate(rows):
        found = step(query, slot, raw, found)
    return found

def certificate(rows):
    return And(len(rows) <= CAPACITY, *[raw == i for i, raw in enumerate(rows)])

def direct(query, count):
    return If(query < count, query, ABSENT)

def indexed(rows, query):
    return If(certificate(rows), direct(query, len(rows)), scan(rows, query))

def expect(name, constraints, expected):
    solver = Solver()
    solver.add(*constraints)
    observed = solver.check()
    assert observed == expected, (name, observed, solver.model() if observed == sat else '')
    print(f'{name}: {observed}')
    return solver

q, n = Ints('query count')
expect('unbounded induction base', [q >= 0, q <= 65535,
    scan([], q) != direct(q, 0)], unsat)
expect('unbounded dense induction step', [q >= 0, q <= 65535,
    n >= 0, n < CAPACITY,
    step(q, n, n, direct(q, n)) != direct(q, n + 1)], unsat)
expect('wrong-kind/absent raw query', [q >= CAPACITY, q <= 65535,
    n >= 0, n <= CAPACITY, direct(q, n) != ABSENT], unsat)
expect('checked little-endian byte bridge', [
    n >= 0, n < CAPACITY,
    (n % 256) + 256 * (n / 256) != n], unsat)

# Arbitrary tables, all raw u16 encodings, both dense and fallback branches.
for count in range(65):
    rows = [Int(f'raw_{count}_{i}') for i in range(count)]
    s = Solver()
    s.add(q >= 0, q <= 65535)
    for raw in rows:
        s.add(raw >= 0, raw <= 65535)
    s.add(indexed(rows, q) != scan(rows, q))
    assert s.check() == unsat, (count, s.model())
print('arbitrary-u16 tables, every count 0..64: unsat')

cases = [
    ('dense hit', list(range(34)), 17, 17, True),
    ('dense absent', list(range(34)), 34, ABSENT, True),
    ('wrong scope kind', list(range(34)), 8192, ABSENT, True),
    ('absent sentinel', list(range(34)), 65535, ABSENT, True),
    ('empty', [], 0, ABSENT, True),
    ('duplicate queried scope', [0, 0], 0, INVALID, False),
    ('duplicate unrelated scope', [0, 0], 1, ABSENT, False),
    ('invalid row kind', [8192], 0, INVALID, False),
    ('absent raw row', [65535], 0, INVALID, False),
    ('permuted fallback hit', [1, 0], 0, 1, False),
    ('sparse fallback hit', [7, 8], 8, 1, False),
]
for name, rows, query, result, dense in cases:
    expect(name, [indexed(rows, query) == result, certificate(rows) == dense], sat)

# Deliberately remove the certificate. A counterexample must be possible.
s = expect('unguarded-direct mutant counterexample', [q == 0,
    direct(q, 2) != scan([0, 0], q)], sat)
print('PASS: equivalence, induction, byte bridge, and nonvacuity gates')
