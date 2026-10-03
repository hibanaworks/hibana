#!/usr/bin/env python3
"""Independent Z3 conditions for the sorted source-marker window rewrite.

Unbounded induction conditions accompany the Lean proof. Bounded executable
models include duplicate offsets and an arbitrary ordered state transition, so
selection, malformed-candidate outcomes, and fold order are not simplified away.
"""
from z3 import And, BoolVal, Function, If, Implies, Int, IntSort, IntVal, Not, Or, Solver, sat, unsat
import json

checks = []
def check(name, assumptions, bad, expected=unsat):
    solver = Solver()
    solver.set(timeout=10000)
    solver.add(*assumptions, bad)
    actual = solver.check()
    assert actual == expected, (name, actual, solver.model() if actual == sat else '')
    checks.append({'name': name, 'result': str(actual)})

n, lo, hi, q, i, j = [Int(x) for x in ['n', 'lo', 'hi', 'q', 'i', 'j']]
t = Function('offset', IntSort(), IntSort())
mid = lo + (hi - lo) / 2
bounds = [0 <= lo, lo < hi, hi <= n]
check('unbounded midpoint bounds', bounds, Not(And(lo <= mid, mid < hi)))
check('unbounded right interval strictly shrinks', bounds,
      Not(And(0 <= hi - (mid + 1), hi - (mid + 1) < hi - lo)))
check('unbounded left interval strictly shrinks', bounds,
      Not(And(0 <= mid - lo, mid - lo < hi - lo)))
check('unbounded right search preserves smaller prefix',
      bounds + [0 <= i, i < mid + 1, t(mid) < q,
                Implies(i < lo, t(i) < q), Implies(i <= mid, t(i) <= t(mid))],
      Not(t(i) < q))
check('unbounded left search preserves greater-or-equal suffix',
      bounds + [mid <= i, i < n, q <= t(mid), t(mid) <= t(i)], Not(q <= t(i)))
check('unbounded no equal row before lower bound',
      [0 <= i, i < lo, t(i) < q], t(i) == q)
check('unbounded stop excludes all later matches',
      [0 <= j, j <= i, i < n, q < t(j), t(j) <= t(i)], t(i) == q)

visit = Function('visit', IntSort(), IntSort(), IntSort())
initial = Int('initial')
def model(length, sorted_required=True, first_only=False, reverse=False):
    offsets = [Int(f'offset_{length}_{k}') for k in range(length)]
    # row contents/validity/selection are abstracted by the arbitrary visit
    # function; different call order is observable, including an error result.
    old = initial
    for k in range(length):
        old = If(offsets[k] == q, visit(old, k), old)
    low, high = IntVal(0), IntVal(length)
    def lookup(index):
        out = IntVal(0)
        for k in reversed(range(length)):
            out = If(index == k, offsets[k], out)
        return out
    for _ in range(length + 1):
        middle = low + (high - low) / 2
        active = low < high
        right = lookup(middle) < q
        low, high = If(And(active, right), middle + 1, low), If(And(active, Not(right)), middle, high)
    out = initial
    running = BoolVal(True)
    for k in (reversed(range(length)) if reverse else range(length)):
        eligible = k >= low
        running = And(running, Not(And(eligible, offsets[k] > q)))
        matched = And(running, eligible, offsets[k] == q)
        if first_only:
            matched = And(matched, k == low)
        out = If(matched, visit(out, k), out)
    assumptions = [0 <= q] + [0 <= x for x in offsets]
    if sorted_required:
        assumptions += [offsets[k] <= offsets[k+1] for k in range(length-1)]
    return assumptions, old != out, offsets

for length in range(13):
    assumptions, differs, _ = model(length)
    check(f'ordered fold exact length {length}', assumptions, differs)

assumptions, differs, offsets = model(3, sorted_required=False)
check('negative control: unsorted input can change result', assumptions, differs, sat)
assumptions, differs, offsets = model(3, first_only=True)
check('negative control: keeping only one tie changes result',
      assumptions + [offsets[0] == q, offsets[1] == q], differs, sat)
assumptions, differs, offsets = model(3, reverse=True)
check('negative control: reordering equal offsets changes result',
      assumptions + [offsets[0] == q, offsets[1] == q, offsets[2] == q], differs, sat)
print(json.dumps({'status': 'PASS', 'checks': checks}, indent=2))
