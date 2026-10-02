#!/usr/bin/env python3
from z3 import *
CAP = 8192
INVALID, ABSENT = -2, -1

def step(q, slot, raw, found):
    return If(found == INVALID, INVALID, If(raw >= CAP, INVALID,
        If(raw == q, If(found == ABSENT, slot, INVALID), found)))

def scan(rows, q):
    out = IntVal(ABSENT)
    for slot, raw in enumerate(rows): out = step(q, slot, raw, out)
    return out

def cert(rows):
    return And(*[r < CAP for r in rows], *[a < b for a, b in zip(rows, rows[1:])])

def search(rows, q, lo, hi):
    if lo >= hi: return IntVal(ABSENT)
    mid = lo + (hi-lo)//2
    return If(rows[mid] < q, search(rows, q, mid+1, hi),
        If(q < rows[mid], search(rows, q, lo, mid), mid))

def check(name, constraints, expected=unsat):
    s=Solver(); s.add(*constraints)
    result=s.check()
    assert result == expected, (name,result,s.model() if result==sat else '')
    print(name+': '+str(result), flush=True)

q=Int('query')
# Symbolic inductive verification conditions for the half-open binary loop.
lo,hi,n,mid,i=Ints('lo hi n mid i')
v,vi=Ints('value value_i')
bounds=[0<=lo,lo<=hi,hi<=n,lo<hi,mid==lo+(hi-lo)/2]
check('midpoint and strict width decrease',bounds+[Or(mid<lo,mid>=hi,hi-(mid+1)>=hi-lo,mid-lo>=hi-lo)])
check('right step excludes no match',bounds+[lo<=i,i<hi,vi==q,v<q,
    Implies(i<mid,vi<v),Implies(i==mid,vi==v),i<mid+1])
check('left step excludes no match',bounds+[lo<=i,i<hi,vi==q,q<v,
    Implies(mid<i,v<vi),Implies(i==mid,vi==v),i>=mid])
check('equality branch has exact match',[Not(v<q),Not(q<v),v!=q])
check('adjacent sorted induction transitivity',[vi<v,v<q,Not(vi<q)])

for size in list(range(35))+[42,64,79]:
    rows=[Int(f'raw_{size}_{j}') for j in range(size)]
    old=scan(rows,q)
    new=If(cert(rows),search(rows,q,0,size),old)
    check(f'arbitrary-u16 table length {size}',[q>=0,q<=65535]+[And(r>=0,r<=65535) for r in rows]+[new!=old])

cases=[
    ('shifted hit',list(range(1,35)),17,16,True),
    ('gapped hit',list(range(1,68,2)),19,9,True),
    ('gap absent',list(range(1,68,2)),18,ABSENT,True),
    ('wrong kind',list(range(1,35)),8192,ABSENT,True),
    ('absent sentinel',list(range(1,35)),65535,ABSENT,True),
    ('empty',[],0,ABSENT,True),
    ('duplicate queried',[0,0],0,INVALID,False),
    ('duplicate unrelated',[0,0],1,ABSENT,False),
    ('invalid row',[8192],0,INVALID,False),
    ('permutation fallback',[1,0],0,1,False),
]
for name,rows,query,expected,certified in cases:
    out=If(cert(rows),search(rows,query,0,len(rows)),scan(rows,query))
    check(name,[out==expected,cert(rows)==certified],sat)
check('unguarded-binary mutant',[search([0,0],0,0,2)!=scan([0,0],0)],sat)
print('PASS: binary search equivalence, inductive conditions, and nonvacuity')
