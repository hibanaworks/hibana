"""Rerun the reviewed gate, refusing any mismatch with preimplementation source."""
from pathlib import Path
import subprocess, time, json, hashlib, datetime
root = Path(__file__).resolve().parent
source = root.parents[1]
expected = {
'src/global/compiled/lowering/seal.rs': 'a1e54426c1e5b344b96e1f90f8cd9a41b054f8c8ae1fa597e19f3892b1573016',
'src/global/compiled/lowering/driver/impls/image.rs': '1a5af0704ec7809d57855650e8fbf363b6f040da1a1b14f1435aa5d813666f6d',
'src/global/const_dsl/endpoint_selectors.rs': '7d52066eae327eec0f640d0ab444cf1960ab76cf8a31b14efb859df7227bb85e',
'src/global/const_dsl/eff_list.rs': '19ec1efeab3953aaed72c9c155c1f9713d5fe7047357ea431369f883264e1f63',
}
hashes = lambda: {str(p): hashlib.sha256((source/p).read_bytes()).hexdigest() for p in expected}
assert hashes() == expected, 'Expected exact pre-edit source'
result = {'baseline': subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip(), 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_sha256_before': hashes()}
assert result['baseline'] == 'a6339772d4bf2c905f491e0284d3bef36e79bb6f'
for name, cmd in {
 'lean': ['/workspace/scratch/0915e8fbff81/tools/lean/lean-4.30.0-linux/bin/lean', str(root/'ParticipantMask.lean')],
 'z3': ['/tmp/hibana-quic-proof-venv/bin/python', '-u', str(root/'check_participant_mask.py')],
}.items():
 start=time.monotonic()
 with (root/(name+'.log')).open('w') as log: process=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,cwd=source,timeout=120)
 result[name]={'command':cmd,'exit_code':process.returncode,'elapsed_seconds':time.monotonic()-start}
 assert process.returncode == 0, (name,process.returncode)
 print(name,result[name],flush=True)
assert hashes() == expected, 'Source changed during gate'
result.update(source_sha256_after=hashes(), production_edits=False, completed_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
(root/'pre_edit_gate.json').write_text(json.dumps(result,indent=2)+'\n')
print('PRE-EDIT GATE PASSED',result['completed_utc'],flush=True)
