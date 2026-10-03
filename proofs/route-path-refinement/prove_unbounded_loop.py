"""Unbounded inductive verification conditions for the proposed two-array loop.

n, category, cursor, IDs, tags, and all witness indices are symbolic. There is
no event-count unrolling. The concrete transition is the one in the prior
check_route_path_refinement.py; ghost owner/p are proof state only.

The arithmetic cardinality facts for p = number of processed event slots are
proved independently in ProcessedCardinality.lean. These facts require no
assumption about nesting/laminarity of routes.
"""
from z3 import *
import json, time, pathlib
OUT=pathlib.Path(__file__).parent
S=65535
records=[]

def check(name, assumptions, goal):
    s=Solver(); s.set(timeout=20000)
    s.add(*assumptions, Not(goal))
    start=time.monotonic(); result=s.check(); elapsed=time.monotonic()-start
    rec=dict(name=name,result=str(result),elapsed_seconds=elapsed)
    if result==unknown:rec['reason']=s.reason_unknown()
    if result==sat:rec['counterexample']=str(s.model())
    records.append(rec)
    print(f'{name}: {result} ({elapsed:.3f}s)',flush=True)
    if result!=unsat:print(rec,flush=True)
    return result==unsat

n,oc,c,i,next_id,p=Ints('n old_count category cursor next_id processed_count')
old,tags,ids,remap,owner=[Array(x,IntSort(),IntSort()) for x in ['old','tags','ids','remap','owner']]
a,b,k=Ints('a b k')
V=lambda e:And(0<=e,e<n)
D=lambda e,cat,cur:And(V(e),Or(tags[e]<cat,And(tags[e]==cat,e<cur)))
C=lambda e,cat,cur:And(V(e),tags[e]==cat,e<cur)
base=[0<=n,n<=S,0<=oc,oc<=n,0<=c,c<=3,0<=i,i<=n,0<=next_id,next_id<=p,p<=n,
      ForAll([a],Implies(V(a),And(0<=old[a],old[a]<oc,0<=tags[a],tags[a]<3)))]

def inv(cat,cur,ns,rs,os,cnt):
    return [
      ForAll([a],Implies(And(V(a),Not(D(a,cat,cur))),ns[a]==old[a])),
      ForAll([a],Implies(D(a,cat,cur),And(0<=ns[a],ns[a]<cnt,ns[a]<S))),
      ForAll([a,b],Implies(And(D(a,cat,cur),D(b,cat,cur)),
          (ns[a]==ns[b])==And(old[a]==old[b],tags[a]==tags[b]))),
      ForAll([a],Implies(C(a,cat,cur),And(rs[old[a]]==ns[a],rs[old[a]]!=S))),
      ForAll([k],Implies(And(0<=k,k<oc,rs[k]!=S),
          And(C(os[k],cat,cur),old[os[k]]==k,ns[os[k]]==rs[k])))
    ]

# Initial state (ids copy the old partition; next ID resets per feature).
init=[0<=n,n<=S,0<=oc,oc<=n,
      ForAll([a],Implies(V(a),And(0<=old[a],old[a]<oc,0<=tags[a],tags[a]<3)))]
for j,goal in enumerate(inv(0,0,old,K(IntSort(),IntVal(S)),owner,0)):
    check(f'initial invariant {j}',init,goal)

assumptions=base+inv(c,i,ids,remap,owner,next_id)+[c<3,i<n]
match=tags[i]==c
key=ids[i]
fresh=remap[key]==S
value=If(fresh,next_id,remap[key])
new_ids=If(match,Store(ids,i,value),ids)
new_map=If(And(match,fresh),Store(remap,key,next_id),remap)
new_owner=If(And(match,fresh),Store(owner,key,i),owner)
new_next=next_id+If(And(match,fresh),1,0)
new_p=p+If(match,1,0)
# p counts distinct processed slots: a matching slot is not yet processed.
# Both constraints below follow from ProcessedCardinality.lean, not SMT bounds.
cardinality=[Implies(match,p<n)]
check('matching event still contains its original old ID',assumptions,Implies(match,key==old[i]))
check('conditional remap index is in old-class bounds',assumptions,Implies(match,And(0<=key,key<oc,key<n)))
check('new IDs monotone and allocation count is at most processed slots',assumptions+cardinality,
      And(next_id<=new_next,new_next<=next_id+1,0<=new_next,new_next<=new_p,new_p<=n))
check('fresh stored ID excludes sentinel, increment fits u16',assumptions+cardinality,
      Implies(And(match,fresh),And(0<=next_id,next_id<n,next_id<S,new_next<=S)))
for j,goal in enumerate(inv(c,i+1,new_ids,new_map,new_owner,new_next)):
    check(f'event step preserves invariant {j}',assumptions+cardinality,goal)
# Read-back/injectivity of map entries is a consequence of the same witnesses.
check('non-sentinel remap entries are injective',assumptions,
      ForAll([a,b],Implies(And(0<=a,a<oc,0<=b,b<oc,remap[a]!=S,remap[b]!=S),
                          (remap[a]==remap[b])==(a==b))))
# Reset only the mapping, never the running next-ID counter.
reset=base+inv(c,n,ids,remap,owner,next_id)+[c<3]
for j,goal in enumerate(inv(c+1,0,ids,K(IntSort(),IntVal(S)),owner,next_id)):
    check(f'category reset preserves invariant {j}',reset,goal)
# After category 2, every valid event has been processed.
final=base+inv(3,0,ids,remap,owner,next_id)+[p==n]
check('all events processed after categories zero one two',final,ForAll([a],Implies(V(a),D(a,3,0))))
check('final exact equality of old-class and membership pairs',final,
      ForAll([a,b],Implies(And(V(a),V(b)),
                          (ids[a]==ids[b])==And(old[a]==old[b],tags[a]==tags[b]))))
check('final IDs compactly bounded and sentinel excluded',final,
      And(0<=next_id,next_id<=n,next_id<=S,
          ForAll([a],Implies(V(a),And(0<=ids[a],ids[a]<next_id,ids[a]<n,ids[a]<S)))))
check('nonempty domain allocates at least one ID',final,Implies(n>0,next_id>0))
check('scratch addresses and byte size fit 32-bit usize',base+[n<=S],
      And(4*n<=2**32-1,Implies(And(0<=k,k<oc),k<n)))
(OUT/'unbounded-vc-results.json').write_text(json.dumps(dict(
  event_bound='none (symbolic n with production u16 capacity precondition)',
  solver_timeout_ms=20000,limits_increased=False,results=records),indent=2)+'\n')
if any(r['result']!='unsat' for r in records):raise SystemExit(1)
print(f'ALL {len(records)} UNBOUNDED LOOP VCS PROVED',flush=True)
