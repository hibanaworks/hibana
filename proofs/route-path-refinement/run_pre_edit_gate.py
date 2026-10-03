"""Run both proof layers against exact baseline production inputs, before editing."""
from pathlib import Path
import subprocess, hashlib, datetime, json, time
root = Path(__file__).resolve().parent
repo = root.parents[1]
paths = ['src/global/const_dsl/event_relations.rs', 'src/global/const_dsl/allocation/frame_labels/roll.rs', 'src/global/const_dsl/scope_ranges/route.rs', 'src/global/const_dsl/eff_list.rs']
base = 'a6339772d4bf2c905f491e0284d3bef36e79bb6f'
hash_bytes = lambda data: hashlib.sha256(data).hexdigest()
expected = {p: hash_bytes(subprocess.check_output(['git', 'show', base + ':' + p], cwd=repo)) for p in paths}
actual = lambda: {p: hash_bytes((repo/p).read_bytes()) for p in paths}
assert actual() == expected, 'Gate must run before route-cache production edits'
result = {'baseline': base, 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_sha256_before': actual(), 'limits_increased': False, 'production_edits': False}
lean = '/workspace/scratch/0915e8fbff81/tools/lean/lean-4.30.0-linux/bin/lean'
python = '/tmp/hibana-quic-proof-venv/bin/python'
commands = {'lean-relation': [lean, str(root/'RoutePathRefinement.lean')], 'lean-cardinality': [lean, str(root/'ProcessedCardinality.lean')], 'z3-unbounded': [python, '-u', str(root/'prove_unbounded_loop.py')], 'z3-concrete': [python, '-u', str(root/'check_route_path_refinement.py')]}
for name, cmd in commands.items():
 start = time.monotonic()
 log = root/(name + '.log')
 with log.open('w') as out:
  p = subprocess.run(cmd, cwd=repo, stdout=out, stderr=subprocess.STDOUT, timeout=180)
 result[name] = {'command': cmd, 'exit_code': p.returncode, 'elapsed_seconds': time.monotonic()-start, 'log_sha256': hash_bytes(log.read_bytes())}
 assert p.returncode == 0, (name, p.returncode)
 if name.startswith('lean'):
  assert 'sorryAx' not in log.read_text() and 'error:' not in log.read_text()
 print(name, result[name], flush=True)
assert actual() == expected, 'Production changed during gate'
result.update(source_sha256_after=actual(), proof_sha256={p.name:hash_bytes(p.read_bytes()) for p in root.iterdir() if p.suffix in ['.lean', '.py']}, completed_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
(root/'pre_edit_gate.json').write_text(json.dumps(result, indent=2)+'\n')
print('PRE-EDIT LEAN + Z3 GATE PASSED', result['completed_utc'], flush=True)
