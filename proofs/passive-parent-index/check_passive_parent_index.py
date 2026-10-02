#!/usr/bin/env python3
"""Z3 equivalence of certified owner+exact-edge lookup and ordered legacy scan.
Keys encode complete parent Route IDs plus binary arm. Slot order, valid reads,
edge/owner agreement, and first-match early return are modeled separately.
"""
from z3 import *
INVALID, ABSENT = -2, -1
CAPACITY = 8192

def array_from(values):
    out = K(IntSort(), IntVal(ABSENT))
    for i, value in enumerate(values): out = Store(out, i, value)
    return out

def key(scopes, row):
    return 2 * scopes[row // 2] + row % 2

def valid_fact(scopes, owners, edges, well_formed, row):
    child = edges[row]
    return And(well_formed[row], Or(child == ABSENT,
        And(child > row // 2, child < len(scopes), Select(owners, child) == key(scopes, row))))

def scan(scopes, owners, edges, well_formed, query):
    result = IntVal(ABSENT)
    for row in reversed(range(len(edges))):
        result = If(Not(valid_fact(scopes, owners, edges, well_formed, row)), INVALID,
            If(And(edges[row] >= 0, edges[row] == query), key(scopes,row), result))
    return result

def certified(scopes, owners_list, owners, edges, well_formed):
    return And(*[And(0 <= s, s < CAPACITY) for s in scopes],
        *[a < b for a,b in zip(scopes,scopes[1:])],
        *[owner >= ABSENT for owner in owners_list],
        *[valid_fact(scopes,owners,edges,well_formed,r) for r in range(len(edges))])

def indexed(scopes, owners, edges, query):
    owner = Select(owners, query)
    result = IntVal(ABSENT)
    for row in reversed(range(len(edges))):
        result = If(owner == key(scopes,row),
            If(edges[row] == query, owner, ABSENT), result)
    return If(And(0 <= query, query < len(scopes)), If(owner == INVALID, INVALID, result), ABSENT)

def check(name, constraints, expected):
    s = Solver(); s.add(*constraints)
    got = s.check()
    assert got == expected, (name, got, s.model() if got == sat else '')
    print(f'{name}: {got}', flush=True)

for n in list(range(9)) + [16,34,50]:
    scopes = [Int(f'scope_{n}_{i}') for i in range(n)]
    own = [Int(f'owner_{n}_{i}') for i in range(n)]
    owners = array_from(own)
    edges = [Int(f'edge_{n}_{i}') for i in range(2*n)]
    well = [Bool(f'row_ok_{n}_{i}') for i in range(2*n)]
    q = Int(f'query_{n}')
    cert = certified(scopes,own,owners,edges,well)
    old = scan(scopes,owners,edges,well,q)
    new = If(cert,indexed(scopes,owners,edges,q),old)
    constraints = [q >= ABSENT] + [And(-2 <= o, o < 2*CAPACITY) for o in own]
    constraints += [And(ABSENT <= e, e <= 65534) for e in edges]
    # Scope validity/order is part of the certificate, not assumed for fallback.
    constraints += [And(0 <= raw,raw <= 65535) for raw in scopes]
    check(f'arbitrary raw relation with {n} routes', constraints+[new != old], unsat)

# Witnesses distinguish valid ownership, actual edges, global validity, and order.
def witness(name, scopes, own, edges, well, query, result, cert_expected):
    owners = array_from(own)
    cert = certified(scopes,own,owners,edges,well)
    old = scan(scopes,owners,edges,well,query)
    new = If(cert,indexed(scopes,owners,edges,query),old)
    check(name,[cert==cert_expected,new==result,old==result],sat)

witness('positive gapped scopes',[3,7],[-1,6],[1,-1,-1,-1],[True]*4,1,6,True)
witness('missing edge despite owner',[3,7],[-1,6],[-1]*4,[True]*4,1,ABSENT,True)
witness('missing query',[3,7],[-1,6],[1,-1,-1,-1],[True]*4,2,ABSENT,True)
witness('wrong owner fallback',[3,7],[-1,7],[1,-1,-1,-1],[True]*4,1,INVALID,False)
witness('wrong arm fallback',[3,7],[-1,6],[-1,1,-1,-1],[True]*4,1,INVALID,False)
witness('duplicate child after first match',[1,4,8],[-1,-1,2],[2,-1,2,-1,-1,-1],[True]*6,2,2,False)
witness('self edge fallback',[3],[6],[0,-1],[True]*2,0,INVALID,False)
witness('backward edge fallback',[3,7],[14,-1],[-1,-1,0,-1],[True]*4,0,INVALID,False)
witness('passive cycle preserves early hit',[3,7],[14,6],[1,-1,0,-1],[True]*4,1,6,False)
witness('passive cycle rejects backward edge',[3,7],[14,6],[1,-1,0,-1],[True]*4,0,INVALID,False)
witness('unused cyclic owners grant no edge',[3,7],[14,6],[-1]*4,[True]*4,0,ABSENT,True)
witness('malformed row after first hit',[3,7],[-1,6],[1,-1,-1,-1],[True,True,True,False],1,6,False)
witness('malformed row before hit',[3,7],[-1,7],[-1,1,-1,-1],[False,True,True,True],1,INVALID,False)
witness('unused invalid owner keeps harmless miss',[3,7],[-2,-1],[-1]*4,[True]*4,1,ABSENT,False)
witness('empty relation',[],[],[],[],0,ABSENT,True)

check('owner-only mutant falsely creates edge',[
    Select(array_from([-1,6]),1) != scan([3,7],array_from([-1,6]),[-1]*4,[True]*4,1)],sat)
check('unguarded-index mutant skips malformed predecessor',[
    indexed([3,7],array_from([-1,7]),[-1,1,-1,-1],1) !=
    scan([3,7],array_from([-1,7]),[-1,1,-1,-1],[False,True,True,True],1)],sat)
check('unguarded owner-read mutant changes harmless miss',[
    indexed([3,7],array_from([-2,-1]),[-1]*4,0) !=
    scan([3,7],array_from([-2,-1]),[-1]*4,[True]*4,0)],sat)
print('PASS: certified reverse lookup, malformed fallback, topology, and nonvacuity')
