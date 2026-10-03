#!/usr/bin/env python3
"""Source-derived, read-only allocator proposal model; NOT Rust execution/refinement.
Keeps every baseline unequal color separated for the same source/target/lane key,
and adds body/continuation interference for each elastic roll. This is conservative
and first-fit may reject a colorable graph. No minimal-colorability claim.
The full connection source uses disjoint two-role components and therefore every
Par reuses lane0. Reject a source shape that violates that verified precondition.
"""
from pathlib import Path
import re, json, collections, hashlib, sys
HERE=Path(__file__).resolve().parent
ROOT=Path(sys.argv[1])
def args(s):
 out=[];start=depth=0
 for i,c in enumerate(s):
  if c in '<{([':depth+=1
  elif c in '>})]':depth-=1
  elif c==',' and depth==0:out.append(s[start:i].strip());start=i+1
 if s[start:].strip():out.append(s[start:].strip())
 return out
def aliases(path):
 s=re.sub(r'//[^\n]*','',path.read_text());out={}
 for m in re.finditer(r'pub type (\w+)',s):
  start=s.index('=',m.end())+1;depth=0
  for i in range(start,len(s)):
   c=s[i]
   if c in '<{([':depth+=1
   elif c in '>})]':depth-=1
   elif c==';' and depth==0:out[m[1]]=s[start:i].strip();break
 return out
def ast(v,a,roles):
 name=v.split('<')[0].strip()
 if name=='g::Send':
  p=args(v[v.index('<')+1:v.rindex('>')]);return ('Send',roles[p[0]],roles[p[1]])
 if name in ['g::Seq','g::Route','g::Roll','g::Par']:
  return (name[3:],*[ast(x,a,roles) for x in args(v[v.index('<')+1:v.rindex('>')])])
 return ast(a[name],a,roles)
def lower(tree):
 events=[];scopes=[];published_routes=[];count=collections.Counter()
 def members(s,i): return 0 if s['start']<=i<s['mid'] else 1 if s['mid']<=i<s['end'] else 2
 def path(i):return tuple(members(s,i) for s in published_routes)
 def key(i):return tuple(events[i]['key'])
 def walk(n,ancestors):
  typ=n[0];count[typ]+=1
  if typ=='Send':
   events.append({'id':len(events),'key':[n[1],n[2],0],'baseline_color':0});return
  if typ=='Seq':walk(n[1],ancestors);walk(n[2],ancestors);return
  s={'id':len(scopes),'kind':typ,'start':len(events),'ancestors':list(ancestors)};scopes.append(s)
  walk(n[1],ancestors+[(s['id'],0)]);s['mid']=len(events)
  if typ!='Roll':walk(n[2],ancestors+[(s['id'],1)])
  s['end']=len(events)
  if typ=='Route':
   published_routes.append(s)
   for k in {key(i) for i in range(s['mid'],s['end'])}:
    used={events[i]['baseline_color'] for i in range(s['start'],s['mid']) if key(i)==k}
    remap={}
    for i in range(s['mid'],s['end']):
     if key(i)!=k:continue
     old=events[i]['baseline_color']
     if old not in remap:
      c=next(v for v in range(256) if v not in used);remap[old]=c;used.add(c)
     events[i]['baseline_color']=remap[old]
  elif typ=='Roll':
   seen={};used=collections.defaultdict(set)
   for i in range(s['start'],s['end']):
    identity=(key(i),path(i))
    if identity not in seen:
     c=next(v for v in range(256) if v not in used[key(i)])
     seen[identity]=c;used[key(i)].add(c)
    events[i]['baseline_color']=seen[identity]
  else:
   assert typ=='Par'
   left={v for i in range(s['start'],s['mid']) for v in key(i)[:2]}
   right={v for i in range(s['mid'],s['end']) for v in key(i)[:2]}
   assert left.isdisjoint(right), 'Unsupported shared-endpoint Par lane allocation'
 walk(tree,[])
 for s in scopes:
  if s['kind']!='Roll':continue
  boundaries=[]
  for sid,arm in s['ancestors']:
   anc=scopes[sid]
   boundary=anc['mid'] if anc['kind'] in ('Route','Par') and arm==0 else anc['end']
   if boundary>s['end']:boundaries.append(boundary)
  s['continuation_end']=min(boundaries+[len(events)])
 return events,scopes,dict(count)
def solve(tree):
 events,scopes,count=lower(tree);edges=[];added=[]
 rolls=[s for s in scopes if s['kind']=='Roll']
 for j,b in enumerate(events):
  used=set()
  for i,a in enumerate(events[:j]):
   if a['key']!=b['key'] or a['key'][0]==a['key'][1]:continue
   base=a['baseline_color']!=b['baseline_color']
   reentry=any(s['start']<=i<s['end']<=j<s['continuation_end'] for s in rolls)
   if base or reentry:
    edges.append([i,j]);used.add(a['proposed_color'])
    if reentry and not base:added.append([i,j])
  free=[c for c in range(256) if c not in used]
  if not free:raise RuntimeError(f'Allocator exhausted at occurrence {j}; not proof of uncolorability')
  b['proposed_color']=free[0]
 assert all(events[a]['proposed_color']!=events[b]['proposed_color'] for a,b in edges)
 perkey=[]
 for key in sorted({tuple(e['key']) for e in events}):
  part=[e for e in events if tuple(e['key'])==key]
  perkey.append({'key':key,'events':len(part),'baseline_colors':len({e['baseline_color'] for e in part}),'proposed_colors':len({e['proposed_color'] for e in part}),'max_proposed_color':max(e['proposed_color'] for e in part)})
 return {'counts':count,'events':events,'scopes':scopes,'edges':edges,'new_edges':added,'per_key':perkey,'max_colors_any_key':max(x['proposed_colors'] for x in perkey)}
paths={
 'tls':('src/roles/protocol_tls_phases.rs','TlsFlow',{'C':24,'T':25}),
 'key_rx':('src/roles/protocol.rs','KeyFlow',{'C':16,'K':17}),
 'key_tx':('src/roles/protocol.rs','KeyFlow',{'C':18,'K':19}),
 'recovery':('src/roles/protocol_recovery.rs','RecoveryFlow',{'C':26,'O':27}),
 'stream':('src/roles/protocol_stream.rs','StreamFlow',{'C':28,'O':29}),
 'path':('src/roles/protocol_path.rs','PathFlow',{'C':30,'O':31}),
 'early':('src/roles/protocol_early.rs','EarlyFlow',{'C':32,'O':33}),
}
def par(a,b):return ('Par',a,b)
def send(a,b):return ('Send',a,b)
def seq(a,b):return ('Seq',a,b)
def route(a,b):return ('Route',a,b)
def roll(a):return ('Roll',a)
trees={k:ast(n,aliases(ROOT/p),r) for k,(p,n,r) in paths.items()}
trees['minimal']=seq(send(0,1),seq(send(1,0),seq(roll(route(send(0,1),send(0,1))),route(seq(seq(send(1,0),send(0,1)),roll(route(send(0,1),send(0,1)))),seq(send(1,0),send(0,1))))))
base=par(par(par(trees['key_rx'],trees['key_tx']),trees['tls']),par(trees['recovery'],par(trees['stream'],trees['path'])))
trees['tls_key_par']=par(trees['tls'],trees['key_rx'])
trees['connection_base']=base
trees['connection_early']=par(base,trees['early'])
results={name:solve(tree) for name,tree in trees.items()}
assert results['tls']['counts']['Send']==333
assert results['tls_key_par']['counts']['Send']==354
# Exact ten-event projection inventory from the unchanged diagnostic Rust copy.
assert [e['baseline_color'] for e in results['minimal']['events']]==[0,0,0,1,0,0,0,1,1,2]
assert [e['proposed_color'] for e in results['minimal']['events']]==[0,0,0,1,0,2,2,3,1,4]
source_paths={p for p,n,r in paths.values()}|{'reference-tls/src/bin/support/connection_roles.rs'}
output={'claim':'Read-only Python transcription of current frame allocation and conservative final greedy proposal; no compiler equivalence, minimal-colorability or runtime pass claimed','source_hashes':{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in sorted(source_paths)},'graphs':results}
(HERE/'color-capacity-model.json').write_text(json.dumps(output,indent=2)+'\n')
for name,r in results.items():print(name, r['counts'], 'edges',len(r['edges']),'new_edges',len(r['new_edges']),'max_colors',r['max_colors_any_key'])
