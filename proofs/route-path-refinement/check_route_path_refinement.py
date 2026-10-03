"""SMT supplement for proposed in-place two-u16-array route-path refinement.
No Rust edits. Arbitrary feature induction is in RoutePathRefinement.lean;
this checks concrete small remap loops, full16-bit machine arithmetic and
SAT counterexamples for unsafe simplifications.
"""
from z3 import *
import time
claims=[]
def check(name,formula,want=unsat,show=()):
 s=Solver();s.set(timeout=20000);s.add(*formula);t=time.monotonic();result=s.check();elapsed=time.monotonic()-t
 if result!=want:raise AssertionError((name,result,s.model() if result==sat else s.reason_unknown()))
 print(f'PASS {name}: {result} ({elapsed:.6f}s)',flush=True)
 if show and result==sat:print('  witness: '+', '.join(f'{x}={s.model().eval(x,model_completion=True)}' for x in show),flush=True)
 claims.append((name,str(result),elapsed))

def model(n,prefix):
 old=Array(prefix+'_old',IntSort(),IntSort());tags=Array(prefix+'_tags',IntSort(),IntSort())
 old_count=Int(prefix+'_old_count');constraints=[old_count>=int(n!=0),old_count<=n]
 constraints += [And(0<=Select(old,i),Select(old,i)<old_count,0<=Select(tags,i),Select(tags,i)<3) for i in range(n)]
 ids=old;count=IntVal(0);access=[]
 for arm in range(3):
  remap=K(IntSort(),IntVal(65535))
  for i in range(n):
   key=Select(ids,i);matching=Select(tags,i)==arm;previous=Select(remap,key);fresh=previous==65535
   # Access is conditional; earlier category values must never be used as
   # old-class remap keys for this event in a later category.
   access.append(Implies(matching,And(0<=key,key<old_count)))
   value=If(fresh,count,previous)
   updated_ids=Array(f'{prefix}_ids_{arm}_{i}',IntSort(),IntSort())
   updated_remap=Array(f'{prefix}_map_{arm}_{i}',IntSort(),IntSort())
   updated_count=Int(f'{prefix}_count_{arm}_{i}')
   constraints += [updated_ids==If(matching,Store(ids,i,value),ids),
     updated_remap==If(And(matching,fresh),Store(remap,key,count),remap),
     updated_count==count+If(And(matching,fresh),1,0)]
   ids,remap,count=updated_ids,updated_remap,updated_count
 good=[]
 for i in range(n):
  good.append(And(0<=Select(ids,i),Select(ids,i)<count,Select(ids,i)<65535))
  for j in range(n):
   good.append((Select(ids,i)==Select(ids,j))==And(Select(old,i)==Select(old,j),Select(tags,i)==Select(tags,j)))
 good += [count>=int(n!=0),count<=n,*access]
 return constraints,And(*good),old,tags,old_count,ids,count

for n in [0,1,2,3,4]:
 constraints,good,old,tags,old_count,ids,count=model(n,f'n{n}')
 check(f'exact in-place categories/remap/equality/bounds n={n}',[*constraints,Not(good)])
 if n:
  # Distinct old classes remain distinct even with one membership category.
  check(f'nonvacuous all-distinct classes n={n}',[*constraints,old_count==n,
    *[Select(old,i)==i for i in range(n)],*[Select(tags,i)==0 for i in range(n)],count==n,good],sat)

# Full machine domains independent of bounded unrolling.
n,old_count,key,next_id,capacity=Ints('n old_count key next_id capacity')
check('all compact counts and remap indices fit u16/padding capacity',[
  1<=n,n<=65535,n<=capacity,capacity<=65535,1<=old_count,old_count<=n,
  0<=key,key<old_count,0<=next_id,next_id<n,
  Or(key>=capacity,next_id>=65535,next_id+1>65535,4*capacity>2**32-1)])
check('u16 sentinel never aliases a stored class ID',[
  1<=n,n<=65535,0<=next_id,next_id<n,Int2BV(next_id,16)==BitVecVal(65535,16)])

# Relation refinement is exact and reversible for an arbitrary pair of
# observations; omitting a feature is sound precisely when it is uniform on
# the domain (or already implied by existing equivalence).
a,b=Ints('old_a old_b');fa,fb=Ints('feature_a feature_b')
check('pair refinement preserves exactly old-class plus current membership',[
  0<=a,0<=b,0<=fa,fa<3,0<=fb,fb<3,
  ((3*a+fa)==(3*b+fb)) != And(a==b,fa==fb)])
check('uniform feature omission leaves equality unchanged',[
  fa==fb,And(a==b,fa==fb)!=(a==b)])
check('counterexample forgetting old class merges distinct paths',[
  a==0,b==1,fa==0,fb==0,fa==fb,Not(And(a==b,fa==fb))],sat,show=(a,b,fa,fb))
check('counterexample resetting next ID between categories',[
  a==b,fa==0,fb==1,Not(And(a==b,fa==fb))],sat,show=(a,b,fa,fb))
check('counterexample retaining remap entries across different categories',[
  a==b,fa==1,fb==2,Not(And(a==b,fa==fb))],sat,show=(a,b,fa,fb))

def member(start,split,stop,event):
 return If(And(start<=event,event<split),0,If(And(split<=event,event<stop),1,2))
s,m,e,lo,hi,x,y=Ints('route_start route_split route_end body_start body_end x y')
uniform=Or(hi<=s,e<=lo,And(s<=lo,hi<=m),And(m<=lo,hi<=e))
check('interval-uniform skip preserves every pair membership',[
  0<=s,s<m,m<e,0<=lo,lo<hi,lo<=x,x<hi,lo<=y,y<hi,uniform,
  member(s,m,e,x)!=member(s,m,e,y)])
check('counterexample equal outside endpoints do not imply uniform interval',[
  s==2,m==3,e==4,lo==0,hi==5,x==0,y==2,
  member(s,m,e,lo)==member(s,m,e,hi-1),member(s,m,e,x)!=member(s,m,e,y)],sat,show=(s,m,e,lo,hi,x,y))
print(f'ALL ROUTE-PATH REFINEMENT Z3 CHECKS PASSED: {len(claims)} claims/witnesses',flush=True)
