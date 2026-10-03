"""Source-derived loop counts; not a rustc profile or wall-clock measurement."""
from pathlib import Path
import collections, hashlib, json, re, sys
root=Path(__file__).resolve().parents[2]
parser=root/'artifacts/hibana-compiler-cost-proposals-20261002/cost_model.py'
parser_text=parser.read_text()
exec(parser_text[parser_text.index('def arguments'):parser_text.index('def stats')])
source=root/'hibana-quic-core-fixed'
paths=['src/roles/protocol_tls_phases.rs','src/roles/protocol.rs']
tls=ast('TlsFlow',aliases(source/paths[0]),{'C':24,'T':25})
key=ast('KeyFlow',aliases(source/paths[1]),{'C':16,'K':17})
def count(tree):
 events=[]; markers=[]; counts=collections.Counter(); ordinal=0
 def insert(m):
  i=len(markers);markers.append(m)
  while i>0:
   previous=markers[i-1]
   precedence=m[1]=='enter' and previous[0]==m[0] and (previous[1]=='split' or previous[1]=='enter' and previous[3]>m[3])
   if previous[0]>m[0] or precedence: markers[i]=previous;i-=1
   else:break
  markers[i]=m
 def walk(n):
  nonlocal ordinal
  kind=n[0];counts[kind]+=1
  if kind=='Send':events.append(n[1:]);return
  if kind=='Seq':walk(n[1]);walk(n[2]);return
  sid=ordinal;ordinal+=1;start=len(events);walk(n[1]);mid=len(events)
  if kind!='Roll':walk(n[2])
  end=len(events)
  if kind=='Route':
   for m in [(start,'enter',kind,sid),(mid,'exit',kind,sid),(mid,'enter',kind,sid),(end,'exit',kind,sid)]:insert(m)
  elif kind=='Par':
   for m in [(start,'enter',kind,sid),(mid,'split',kind,sid),(end,'exit',kind,sid)]:insert(m)
  elif kind=='Roll':
   for m in [(start,'enter',kind,sid),(end,'exit',kind,sid)]:insert(m)
  else:raise AssertionError(kind)
 walk(tree)
 roles=sorted({r for event in events for r in event});rows=[]
 for role in roles:
  calls=reads=0
  for i,event in enumerate(events):
   if role not in event:continue
   calls+=1
   for m in markers:
    reads+=1
    if m[0]>i:break
  rows.append(dict(role=role,duplicate_scan_calls_removed=calls,outer_marker_loop_reads_removed=reads))
 return dict(structure=dict(counts),markers=len(markers),roles=rows,
   duplicate_scan_calls_removed=sum(r['duplicate_scan_calls_removed'] for r in rows),
   outer_marker_loop_reads_removed=sum(r['outer_marker_loop_reads_removed'] for r in rows))
result=dict(evidence='source-derived operation model, not rustc instruction count or measured savings',
 parser_sha256=hashlib.sha256(parser.read_bytes()).hexdigest(),
 source_sha256={p:hashlib.sha256((source/p).read_bytes()).hexdigest() for p in paths},
 tls=count(tls),tls_key_par=count(('Par',tls,key)))
assert result['tls_key_par']['structure']['Send']==354
assert result['tls_key_par']['markers']==607
print(json.dumps(result,indent=2))
