#!/usr/bin/env python3
"""Full compact-domain arithmetic; search semantics use SortedRouteIndex.lean."""
from z3 import Ints, Solver, And, Not, unsat, sat

offset, count, probe, blob = Ints('offset count probe blob')
domain = And(offset >= 0, offset <= 65535, count >= 0, count <= 65535,
             probe >= 0, probe < count, blob >= 0)
bound = offset + 2 * count <= blob
claims = (offset + 2 * probe < blob, offset + 2 * probe + 1 < blob,
          offset + 2 * count < 2**32)
for claim in claims:
    s = Solver(); s.add(domain, bound, Not(claim))
    assert s.check() == unsat
s = Solver(); s.add(domain, Not(offset + 2 * probe + 1 < blob))
assert s.check() == sat
print('PASS 3 UNSAT compact probe/bounds obligations; missing-bound SAT witness')
