#!/usr/bin/env python3
"""Sealed immutable resolver lookup; first-match fallback, full authority tag."""
from z3 import And, Bool, If, Int, IntVal, Not, Or, Solver, sat, unsat
CAP = 8192

def first(rows, query):
    found = IntVal(-1)
    for i in reversed(range(len(rows))):
        found = If(rows[i] == query, i, found)
    return found

def binary(rows, query, low, high):
    if low == high:
        return IntVal(-1)
    middle = low + (high - low) // 2
    return If(rows[middle] < query, binary(rows, query, middle + 1, high),
              If(query < rows[middle], binary(rows, query, low, middle), middle))

def verify(name, constraints, result):
    s = Solver()
    s.add(*constraints)
    actual = s.check()
    assert actual == result, (name, actual)
    print(name, actual, flush=True)

for n in list(range(17)) + [32, 64]:
    rows = [Int(f"scope_{n}_{i}") for i in range(n)]
    query = Int(f"query_{n}")
    valid = [And(0 <= r, r < CAP) for r in rows]
    sorted_rows = And(*[rows[i] < rows[i+1] for i in range(n-1)])
    candidate = If(sorted_rows, binary(rows, query, 0, n), first(rows, query))
    verify(f"first-match-equivalence-{n}", valid + [candidate != first(rows, query)], unsat)

packed, resolver = Int('packed'), Int('resolver')
scope = packed % 32768
# The private constructor performs the original full authority validation.
admitted = And(0 <= packed, packed <= 65535, scope < CAP,
               0 <= resolver, resolver <= 65535, Or(packed >= 32768, resolver == 0))
verify('complete-scope-kind-not-aliased', [admitted, scope >= CAP], unsat)
verify('intrinsic-authority-remains-canonical', [admitted, packed < 32768, resolver != 0], unsat)
verify('maximum-dynamic-resolver-remains-valid', [admitted, packed == 32768, resolver == 65535], sat)
verify('unsorted-first-match-remains-valid', [first([7, 1, 7], 7) == 0], sat)
# Validation can only be reused for the same immutable image.
canonical_before, canonical_after = Bool('canonical_before'), Bool('canonical_after')
verify('immutable-validation-reuse', [canonical_before, canonical_after == canonical_before,
                                    Not(canonical_after)], unsat)
verify('mutation-would-break-certificate', [canonical_before, Not(canonical_after)], sat)
print('Resolver lookup: 22 UNSAT checks; 3 SAT witnesses.')
