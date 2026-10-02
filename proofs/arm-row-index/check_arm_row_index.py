#!/usr/bin/env python3
"""Raw packed-field bounded SMT supplement to the arbitrary-list Lean proof."""
from z3 import *


def relations(n):
    event = [BitVec(f'event_{n}_{i}', 32) for i in range(n)]
    meta = [BitVec(f'meta_{n}_{i}', 32) for i in range(n)]
    readable = [Bool(f'readable_{n}_{i}') for i in range(n)]
    starts = [BV2Int(LShR(x, 16)) for x in event]
    lens = [BV2Int(x & 65535) for x in event]
    enc = [BV2Int(LShR(x, 16) & 255) for x in meta]
    steps = [If(lens[i] == 0, 0, enc[i] + 1) for i in range(n)]
    core = [And(readable[i], event[i] != 0xffffffff,
                Or(lens[i] != 0, starts[i] == 0),
                meta[i] & 0xff000000 == 0) for i in range(n)]
    stepok = [Or(lens[i] != 0, enc[i] == 0) for i in range(n)]
    events, lanes, query = Ints(f'events_{n} lanes_{n} query_{n}')
    bounds = And(events >= 0, events <= 65535, lanes >= 0, lanes <= 65535)
    totals = [Sum(steps[:i + 1]) for i in range(n)]
    checked = [And(core[i], stepok[i], starts[i] + lens[i] <= events,
                   (lens[i] == 0) == (steps[i] == 0), totals[i] <= lanes,
                   *[And(core[j], stepok[j]) for j in range(i)]) for i in range(n)]
    certificate = And(*[And(core[i], stepok[i], starts[i] + lens[i] <= events,
                          (lens[i] == 0) == (steps[i] == 0), totals[i] <= lanes)
                        for i in range(n)])
    # Encode return (success plus exact two packed words), not just presence.
    original_ok = Or(*[And(query == i, checked[i]) for i in range(n)])
    direct_ok = Or(*[And(query == i, core[i]) for i in range(n)])
    def selected(xs):
        out = BitVecVal(0, 32)
        for i in reversed(range(n)):
            out = If(query == i, xs[i], out)
        return out
    original = (original_ok, If(original_ok, selected(event), 0),
                If(original_ok, selected(meta), 0))
    direct = (direct_ok, If(direct_ok, selected(event), 0),
              If(direct_ok, selected(meta), 0))
    return locals()


def assert_unsat(s, what):
    r = s.check()
    if r != unsat:
        raise AssertionError((what, r, s.model() if r == sat else s.reason_unknown()))

for n in [0, 1, 2, 3, 4, 8, 16, 34, 50, 68, 100]:
    d = relations(n)
    s = Solver(); s.set(timeout=120000)
    s.add(d['bounds'], d['certificate'])
    s.add(Or(*[a != b for a, b in zip(d['original'], d['direct'])]))
    assert_unsat(s, f'certified raw equality n={n}')
    s = Solver(); s.set(timeout=120000); s.add(d['bounds'])
    hybrid = tuple(If(d['certificate'], fast, old) for fast, old in zip(d['direct'], d['original']))
    s.add(Or(*[a != b for a, b in zip(d['original'], hybrid)]))
    assert_unsat(s, f'hybrid raw equality n={n}')
    print(f'PASS raw packed-row certificate and guarded equivalence: {n} rows', flush=True)

# Machine-word bridge: max u16 directory length, max 256 decoded lane steps.
k = Int('machine_rows'); stride = Int('machine_stride')
s = Solver(); s.add(k >= 0, k <= 65535, stride >= 0, stride <= 256, k * stride > 2**32 - 1)
assert_unsat(s, 'u32 prefix safety')
print('PASS prefix arithmetic bound: 65535 * 256 < 2^32')


def witness(name, events, metas, query, event_limit, lane_limit,
            want_cert, want_old, want_direct, read=None):
    d = relations(len(events)); s = Solver(); s.add(d['bounds'])
    for i, value in enumerate(events):
        s.add(d['event'][i] == value, d['meta'][i] == metas[i],
              d['readable'][i] == (True if read is None else read[i]))
    s.add(d['query'] == query, d['events'] == event_limit, d['lanes'] == lane_limit)
    s.add(d['certificate'] == want_cert, d['original_ok'] == want_old,
          d['direct_ok'] == want_direct)
    if s.check() != sat:
        raise AssertionError(name)
    print('PASS witness: ' + name)

row = lambda start, length: (start << 16) | length
meta = lambda steps: ((steps - 1) << 16) | 65535 if steps else 65535
witness('empty image absent query', [], [], 0, 0, 0, True, False, False)
witness('positive nonempty exact row', [row(2, 3)], [meta(2)], 0, 5, 2, True, True, True)
witness('positive canonical zero row', [0], [meta(0)], 0, 0, 0, True, True, True)
witness('gapped event ranges remain valid', [row(1, 1), row(10, 2)], [meta(1), meta(2)], 1, 12, 3, True, True, True)
witness('out of range on certified image', [row(0, 1)], [meta(1)], 1, 1, 1, True, False, False)
witness('later malformed row preserves earlier success', [row(0, 1), 0xffffffff], [meta(1), meta(1)], 0, 1, 2, False, True, True)
witness('later prefix overflow preserves earlier success', [row(0, 1), row(0, 1)], [meta(1), meta(1)], 0, 1, 1, False, True, True)
witness('invalid predecessor must not be skipped', [0xffffffff, row(0, 1)], [meta(1), meta(1)], 1, 1, 2, False, False, True)
witness('unreadable predecessor must not be skipped', [row(0, 1), row(0, 1)], [meta(1), meta(1)], 1, 1, 2, False, False, True, [False, True])
witness('zero row with encoded step must reject', [0], [meta(2)], 0, 0, 0, False, False, True)
witness('selected event bound must reject', [row(0, 2)], [meta(1)], 0, 1, 1, False, False, True)
witness('selected cumulative prefix must reject', [row(0, 1), row(0, 1)], [meta(2), meta(2)], 1, 1, 3, False, False, True)
witness('selected reserved byte rejected by both', [row(0, 1)], [0x0100ffff], 0, 1, 1, False, False, False)
print('ALL ARM ROW INDEX Z3 CHECKS PASSED')
