"""Cold rustc CTFE comparison on source-verified private snapshots.
Builds the production crate with one appended measurement-only private module.
The const assertion executes only the changed route validator after constructing
64 independent routes. This is neither full TLS nor runtime performance.
No limit or lint ceiling is raised. Acquire rust-heavy-build.lock externally.
"""
from pathlib import Path
import hashlib,json,os,shutil,subprocess,time,datetime,statistics
root=Path(__file__).resolve().parent
repo=root.parents[1]
work=repo.parent/'hibana-participant-mask-measurement'
work.mkdir(exist_ok=True)
seal=Path('src/global/compiled/lowering/seal.rs')
baseline=subprocess.check_output(['git','show','a6339772d4bf2c905f491e0284d3bef36e79bb6f:'+str(seal)],cwd=repo)
assert hashlib.sha256(baseline).hexdigest()=='a1e54426c1e5b344b96e1f90f8cd9a41b054f8c8ae1fa597e19f3892b1573016'
benchmark='''use super::*;
use crate::eff::{EffAtom, EventOrigin};
use crate::global::const_dsl::{ScopeId, ReentryMark};
const ROUTES: usize = 64;
const fn source() -> EffList<{ROUTES*6}> {
    let mut source=EffList::new_partitioned(ROUTES*2,ROUTES*4,0);
    let from=if cfg!(measure_max_sparse) {254} else if cfg!(measure_tls_sparse) {24} else {0};
    let mut i=0;
    while i<ROUTES*2 {
        source.push_event_mut(EffAtom {from,to:from+1,label:(i%2+1) as u8,payload_schema:0,origin:EventOrigin::User,lane:0});
        i+=1;
    }
    i=0;
    while i<ROUTES {
        source.push_route_scope_mut(ScopeId::route(i as u16),i*2,i*2+1,i*2+2,ReentryMark::SinglePass);
        i+=1;
    }
    source
}
const SOURCE: EffList<{ROUTES*6}> = source();
const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(&SOURCE);
const _: () = assert!(validate_route_projection_guarantees(&SUMMARY,&SOURCE).is_none());
'''
(root/'compile_cost_benchmark.rs').write_text(benchmark)
snapshots={}
for variant in ['baseline','candidate']:
 dest=work/variant
 shutil.copytree(repo/'src',dest/'src',dirs_exist_ok=True)
 raw=baseline if variant=='baseline' else (repo/seal).read_bytes()
 (dest/seal).write_bytes(raw+b'\n#[path = "'+str(root/'compile_cost_benchmark.rs').encode()+b'"]\nmod compile_cost_benchmark;\n')
 snapshots[variant]={'seal_sha256_before_benchmark_append':hashlib.sha256(raw).hexdigest(), 'source_hashes':{str(p.relative_to(dest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((dest/'src').rglob('*.rs'))}}
# No accidental changes outside the measured seal file.
assert all(v==snapshots['candidate']['source_hashes'][p] for p,v in snapshots['baseline']['source_hashes'].items() if p!=str(seal))
rustc='/tmp/hibana-rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc'
result={'kind':'cold rustc metadata build with forced route-validator CTFE only; not TLS build, not runtime benchmark','started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'rustc':subprocess.check_output([rustc,'--version'],text=True).strip(),'snapshots':snapshots,'runs':[]}
for case in ['dense_two_roles','tls_sparse','max_sparse']:
 for repeat in range(3):
  order=['baseline','candidate'] if repeat%2==0 else ['candidate','baseline']
  for variant in order:
   tag=f'{case}-{repeat}-{variant}'; metrics=root/'validation'/f'{tag}.time'; log=root/'validation'/f'{tag}.log'
   cmd=[rustc,'--edition=2024','--crate-name','hibana','--crate-type=rlib','--emit=metadata','-C','debuginfo=0',str(work/variant/'src/lib.rs'),'-o',str(work/f'{tag}.rmeta')]
   if case!='dense_two_roles':cmd+=['--cfg','measure_'+case]
   start=time.monotonic()
   with log.open('w') as out:
    p=subprocess.Popen(cmd,stdout=out,stderr=subprocess.STDOUT)
    pid,status,usage=os.wait4(p.pid,0)
    p.returncode=os.waitstatus_to_exitcode(status)
   metrics.write_text(f'{time.monotonic()-start:.6f} {usage.ru_utime:.6f} {usage.ru_stime:.6f} {usage.ru_maxrss}\n')
   run={'case':case,'repeat':repeat,'variant':variant,'exit_code':p.returncode,'wall_seconds':time.monotonic()-start,'command':cmd,'metrics':metrics.read_text().strip()}
   result['runs'].append(run)
   print(tag,run['exit_code'],run['metrics'],flush=True)
   (root/'compile_cost_results.json').write_text(json.dumps(result,indent=2)+'\n')
   if p.returncode:raise SystemExit(p.returncode)
result['summary']={case:{variant:{'median_wall_seconds':statistics.median(r['wall_seconds'] for r in result['runs'] if r['case']==case and r['variant']==variant),'median_max_rss_kib':statistics.median(int(r['metrics'].split()[3]) for r in result['runs'] if r['case']==case and r['variant']==variant)} for variant in ['baseline','candidate']} for case in ['dense_two_roles','tls_sparse','max_sparse']}
result['completed_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
(root/'compile_cost_results.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result['summary'],indent=2),flush=True)
