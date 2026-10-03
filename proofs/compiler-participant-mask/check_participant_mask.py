"""Concrete 32-byte/8-bit-role SMT bridge and finite 256-role error-order checks.
Arbitrary source/role-list induction is in ParticipantMask.lean. Every UNSAT
claim has bounded solver time; SAT witnesses guard non-vacuity and wrong-mask
counterexamples. Nothing edits or bypasses production validators.
"""
from z3 import *
import time
checks=[]

def check(name, formulas, want=unsat, show=()):
    solver=Solver(); solver.set(timeout=20000); solver.add(*formulas)
    before=time.monotonic(); got=solver.check(); elapsed=time.monotonic()-before
    if got!=want:
        raise AssertionError((name,str(got),solver.model() if got==sat else solver.reason_unknown()))
    print(f'PASS {name}: {got} ({elapsed:.6f}s)',flush=True)
    if show and got==sat:
        model=solver.model()
        print('  witness: '+', '.join(f'{x}={model.eval(x,model_completion=True)}' for x in show),flush=True)
    checks.append((name,str(got),elapsed))

def word(role): return Extract(7,3,role)
def bit(role): return BitVecVal(1,8) << ZeroExt(5,Extract(2,0,role))
def insert(mask,role): return Store(mask,word(role),Select(mask,word(role)) | bit(role))
def contains(mask,role): return Select(mask,word(role)) & bit(role) != 0
zero=K(BitVecSort(5),BitVecVal(0,8))
mask=Array('arbitrary_mask',BitVecSort(5),BitVecSort(8));role,query=BitVecs('role query',8)
check('byte insertion is exact over every role and every existing mask',[
    contains(insert(mask,role),query) != Or(contains(mask,query),role==query)])
check('duplicate and self-send insertion is idempotent',[
    insert(insert(mask,role),role) != insert(mask,role)])
check('byte and bit positions cover complete role domain',[
    Or(BV2Int(word(role)) >= 32, BV2Int(Extract(2,0,role)) >= 8,
       8*BV2Int(word(role))+BV2Int(Extract(2,0,role)) != BV2Int(role))])
check('role255 uses final byte bit7 and never wraps',[
    role==255, Or(word(role)!=31,Extract(2,0,role)!=7,
                  BV2Int(role)+1!=256, BV2Int(role)+1>=2**32)])

# Exact source-mask completeness, including duplicates and self-sends.
for n in [0,1,2,4,8,16]:
    senders=[BitVec(f'from_{n}_{i}',8) for i in range(n)]
    receivers=[BitVec(f'to_{n}_{i}',8) for i in range(n)]
    actual=zero
    for sender,receiver in zip(senders,receivers):actual=insert(insert(actual,sender),receiver)
    expected=Or(*[Or(sender==query,receiver==query) for sender,receiver in zip(senders,receivers)])
    check(f'exact nonempty-or-empty mask completeness n={n}',[contains(actual,query)!=expected])
    if n:
        check(f'nonvacuous duplicate/self/255 source n={n}',[
            *[sender==255 for sender in senders],*[receiver==255 for receiver in receivers],
            contains(actual,BitVecVal(255,8)),Not(contains(actual,BitVecVal(0,8)))],sat)

# None=0, outbound=1, inbound=2. Payload/logical label do not affect the
# absent-role lemma. Finite compact indexes here are always below65535.
def next_selector(senders,receivers,start,end,role):
    kind=IntVal(0);index=IntVal(-1)
    for i in reversed(range(len(senders))):
        matches=And(start<=i,i<end,Or(senders[i]==role,receivers[i]==role))
        kind=If(matches,If(senders[i]==role,1,2),kind)
        index=If(matches,i,index)
    return kind,index
for n in [1,2,4,8,16]:
    senders=[BitVec(f'arm_from_{n}_{i}',8) for i in range(n)]
    receivers=[BitVec(f'arm_to_{n}_{i}',8) for i in range(n)]
    actual=zero
    for sender,receiver in zip(senders,receivers):actual=insert(insert(actual,sender),receiver)
    a,b,c,d=Ints(f'left_start_{n} left_end_{n} right_start_{n} right_end_{n}')
    bounds=[0<=a,a<=b,b<=n,0<=c,c<=d,d<=n]
    left=next_selector(senders,receivers,a,b,query)
    right=next_selector(senders,receivers,c,d,query)
    check(f'absence forces both bounded selector searches None n={n}',[
        *bounds,Not(contains(actual,query)),Or(left[0]!=0,right[0]!=0)])
    # Direct None/None arm of observer_path_decision returns Accept.
    check(f'absent observer merge accepts n={n}',[
        *bounds,Not(contains(actual,query)),Not(And(left[0]==0,right[0]==0))])

# Ordered first failing role+error are preserved for all256 possible roles;
# missing bits may omit only successes. Encoding includes role, so returning a
# later error with the same enum value would still fail this check.
for n in [0,1,2,26,256]:
    present=[Bool(f'present_{n}_{r}') for r in range(n)]
    error=[Int(f'error_{n}_{r}') for r in range(n)]
    old=new=IntVal(0)
    for r in reversed(range(n)):
        old=If(error[r]!=0,(r+1)*8+error[r],old)
        new=If(And(present[r],error[r]!=0),(r+1)*8+error[r],new)
    assumptions=[And(0<=error[r],error[r]<=5,Implies(Not(present[r]),error[r]==0)) for r in range(n)]
    check(f'ascending first-role and exact-error preservation n={n}',[*assumptions,old!=new])
    if n:
        check(f'nonvacuous late rejection n={n}',[
            *assumptions,*[error[r]==0 for r in range(n-1)],present[-1],error[-1]==5,
            old==(n*8+5),new==old],sat)

# Unchanged global and per-route stages are kept before role scanning.
prior=Ints('causality parallel reentry passive_child controller selector')
old_observer,new_observer=Ints('old_observer new_observer')
def pipeline(observer):
    result=observer
    for error in reversed(prior):result=If(error!=0,error,result)
    return result
check('arbitrary earlier invalid-stage errors retain priority and exact value',[
    old_observer==new_observer,pipeline(old_observer)!=pipeline(new_observer)])
for i in range(len(prior)):
    check(f'nonvacuous earlier-stage error priority stage={i}',[
        *[prior[j]==0 for j in range(i)],prior[i]==i+1,
        old_observer==99,new_observer==99,pipeline(old_observer)==i+1],sat)

# Concrete counterexample to sender-only collection. Same controller0 in both
# arms, but receiver1 has a selector in only one arm. Correct validation rejects
# at role1; a sender-only mask skips every observer and incorrectly accepts.
senders=[BitVecVal(0,8),BitVecVal(0,8)]
receivers=[BitVecVal(255,8),BitVecVal(1,8)]
correct=zero;wrong=zero
for sender,receiver in zip(senders,receivers):
    correct=insert(insert(correct,sender),receiver);wrong=insert(wrong,sender)
def observer_good(role):
    lk,li=next_selector(senders,receivers,0,1,role)
    rk,ri=next_selector(senders,receivers,1,2,role)
    return Or(And(lk==0,rk==0),And(lk==2,rk==2,li!=ri))
def first_failure(m):
    result=IntVal(0)
    for r in reversed(range(256)):
        role=BitVecVal(r,8)
        fail=And(contains(m,role),role!=0,Not(observer_good(role)))
        result=If(fail,r+1,result)
    return result
check('counterexample sender-only mask wrongly omits receiver1 and255',[
    first_failure(correct)==2,first_failure(wrong)==0,
    contains(correct,BitVecVal(255,8)),Not(contains(wrong,BitVecVal(255,8)))],sat)
# A mask that deliberately omits one truly failing role is likewise unsound.
check('counterexample arbitrary wrong omission changes first error',[
    Not(contains(zero,BitVecVal(1,8))),Not(observer_good(BitVecVal(1,8)))],sat)

# Meaningful accepting route with sparse maximal role: controller0 emits to255
# in either arm, whose distinct inbound occurrence identities distinguish them.
senders=[BitVecVal(0,8),BitVecVal(0,8)];receivers=[BitVecVal(255,8),BitVecVal(255,8)]
actual=zero
for sender,receiver in zip(senders,receivers):actual=insert(insert(actual,sender),receiver)
left=next_selector(senders,receivers,0,1,BitVecVal(255,8));right=next_selector(senders,receivers,1,2,BitVecVal(255,8))
check('positive sparse route with distinct inbound evidence and role255',[
    contains(actual,BitVecVal(0,8)),contains(actual,BitVecVal(255,8)),
    Not(contains(actual,BitVecVal(24,8))),left[0]==2,right[0]==2,left[1]!=right[1]],sat)
print(f'ALL PARTICIPANT MASK Z3 CHECKS PASSED: {len(checks)} claims/witnesses',flush=True)
