#!/usr/bin/env python3
"""Single heavy build at a time, existing 270s compile window and 2.5GiB RSS cap."""
import fcntl,json,os,pathlib,signal,subprocess,sys,time
ROOT=pathlib.Path(__file__).resolve().parents[2]; HERE=pathlib.Path(__file__).resolve().parent
stage=sys.argv[1]; command=sys.argv[2:]; seconds=15 if stage.endswith('-runtime') else 270
limit=2560*1024; env=os.environ.copy();env.update(PATH='/tmp/hibana-rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:'+env['PATH'],CARGO_HOME='/tmp/hibana-cargo',RUSTUP_HOME='/tmp/hibana-rustup',CARGO_TARGET_DIR=str(ROOT/'hibana-roll-color-target'),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',RUST_BACKTRACE='0',LC_ALL='C')
for key in ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUST_MIN_STACK'):
 assert not env.get(key),('unexpected override',key)
env['RUSTFLAGS']='--cfg hibana_repo_tests'
lock=(ROOT/'rust-heavy-build.lock').open('a');print('Waiting for heavy-build lock',flush=True);fcntl.flock(lock,fcntl.LOCK_EX);print('Acquired heavy-build lock',flush=True)
start=time.monotonic();peak=0;reason=None;samples=0
with (HERE/(stage+'.log')).open('w') as log:
 p=subprocess.Popen(command,cwd=ROOT/'hibana-roll-color-fix',env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
 while p.poll() is None:
  group=[]
  for stat in pathlib.Path('/proc').glob('[0-9]*/stat'):
   try:
    fields=stat.read_text().rsplit(')',1)[1].split()
    if int(fields[2])==p.pid:
     rss=int(next(line for line in (stat.parent/'status').read_text().splitlines() if line.startswith('VmRSS:')).split()[1]);group.append(rss)
   except (FileNotFoundError,ProcessLookupError,PermissionError,StopIteration):pass
  peak=max(peak,sum(group));samples+=1
  if sum(group)>limit:reason='sampled_process_group_rss_limit'
  if time.monotonic()-start>seconds:reason='time_limit'
  if reason:
   os.killpg(p.pid,signal.SIGTERM)
   try:p.wait(2)
   except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
   break
  time.sleep(.1)
result=dict(stage=stage,command=command,elapsed_seconds=round(time.monotonic()-start,3),returncode=p.returncode,stop_reason=reason,peak_sampled_process_group_rss_kib=peak,rss_limit_kib=limit,timeout_seconds=seconds,sample_count=samples,unique_target_dir=env['CARGO_TARGET_DIR'],cwd=str(ROOT/'hibana-roll-color-fix'),compiler_limit_overrides=False,repo_test_cfg=True,normal_test_stack=True,rust_min_stack_override=False)
(HERE/(stage+'.json')).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);sys.exit(1 if reason else p.returncode)
