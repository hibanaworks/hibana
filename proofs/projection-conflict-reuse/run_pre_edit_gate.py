from pathlib import Path
import json, hashlib, subprocess, datetime
here = Path(__file__).resolve().parent
root = here.parents[1]
source = root / 'hibana-projection-conflict-reuse'
paths = ['src/global/role_program/image_impl/projection.rs', 'src/global/role_program/image_impl/blob_image.rs', 'src/global/role_program/image_impl/projection/dependency.rs', 'src/global/const_dsl/eff_list.rs', 'src/global/const_dsl/source_arena.rs', 'src/global/typestate/facts.rs']
for p in paths[:2]:
    assert (source / p).read_bytes() == (here / ('baseline-' + Path(p).name)).read_bytes(), 'Expected exact pre-edit source'
hashes = {p: hashlib.sha256((source / p).read_bytes()).hexdigest() for p in paths}
r = {'baseline': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=source, text=True).strip(), 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_sha256_before': hashes, 'production_edit_applied': False, 'proof_runs': []}
for name, cmd in [('lean', [str(root / 'tools/lean/lean-4.30.0-linux/bin/lean'), str(here / 'ConflictReuse.lean')]), ('z3', ['/tmp/hibana-quic-proof-venv/bin/python', str(here / 'check_conflict_reuse.py')])]:
    result = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (here / (name + '.log')).write_text(result.stdout)
    print(name, result.returncode, result.stdout)
    assert result.returncode == 0
    assert 'sorryAx' not in result.stdout
    r['proof_runs'].append({'name': name, 'command': cmd, 'exit_code': result.returncode, 'log_sha256': hashlib.sha256(result.stdout.encode()).hexdigest()})
assert hashes == {p: hashlib.sha256((source / p).read_bytes()).hexdigest() for p in paths}
r.update(gate='PASS before production edit', completed_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), proof_sha256={p: hashlib.sha256((here / p).read_bytes()).hexdigest() for p in ['ConflictReuse.lean', 'check_conflict_reuse.py', 'SOURCE_BRIDGE.md', 'run_pre_edit_gate.py']})
(here / 'pre-edit-gate.json').write_text(json.dumps(r, indent=2) + '\n')
