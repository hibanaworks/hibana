#!/usr/bin/env python3
"""Replay the canonical wire-frame refinement against current Lean source."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--lean', default=os.environ.get('LEAN', 'lean'))
parser.add_argument('--repo', type=Path, default=HERE.parents[1])
args = parser.parse_args()
REPO = args.repo.resolve()
lean = shutil.which(args.lean)
assert lean, f'Lean executable not found: {args.lean}'
lean = str(Path(lean).absolute())
lake = str(Path(lean).with_name('lake'))
proof_dir = REPO / 'proofs/lean'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def run(command, cwd=REPO, env=None):
    completed = subprocess.run(command, cwd=cwd, env=env, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=300)
    print(completed.stdout, end='', flush=True)
    assert completed.returncode == 0, (command, completed.returncode)
    return completed.stdout

manifest = json.loads((HERE / 'source-manifest.json').read_text())
for name, digest in manifest['preserved'].items():
    assert sha(HERE / name) == digest, ('preserved pre-edit evidence', name)
assert sha(HERE / 'check_correspondence.py') == sha(HERE / 'pre-edit/check_correspondence.py'), \
    'The replay must use the exact pre-edit Z3 and finite-query generator'
binding = manifest['canonical_source_binding']
for name, digest in binding['source_sha256'].items():
    assert sha(REPO / name) == digest, ('qualified canonical source', name)
wrapper = json.loads((HERE / 'external-binding/portable-wrapper.json').read_text())
assert sha(HERE / 'check_external_allocator.py') == wrapper['portable_checker_sha256']
assert sha(HERE / 'OptionMatchEquivalence.lean') == \
    sha(HERE / 'explicit-match/OptionMatchEquivalence.lean'), 'Historical option proof changed'
for name, digest in manifest['unchanged_sources'].items():
    assert sha(REPO / name) == digest, ('unchanged source bridge', name)
print('PASS preserved pre-edit proofs and exact current allocator source bridge', flush=True)

version = run([lean, '--version'])
assert 'version 4.30.0' in version, version
run([lake, 'build'], proof_dir)
base_path = subprocess.check_output([lake, 'env', 'printenv', 'LEAN_PATH'],
    cwd=proof_dir, text=True).strip()
base_path = os.pathsep.join(str((proof_dir / path).resolve()) if not Path(path).is_absolute()
                            else path for path in base_path.split(os.pathsep))
sys.path.insert(0, str(REPO / '.github/scripts'))
from check_lean_theorem_inventory import erase_non_code, theorem_names

with tempfile.TemporaryDirectory(prefix='hibana-wire-frame-proof-') as temporary:
    build = Path(temporary)
    env = dict(os.environ, LEAN_PATH=str(build) + os.pathsep + base_path)
    names = []
    for module, namespace in [('OptionMatchEquivalence', 'Hibana.WireOptionMatchEquivalence'),
                              ('WireFrameRefinement', 'Hibana.WireFrameRefinement'),
                              ('WireSourceBridge', 'Hibana.WireFrameSourceBridge')]:
        source = (HERE / (module + '.lean')).read_text()
        code = erase_non_code(source)
        assert not re.search(r'\b(sorry|admit|axiom|constant|opaque|unsafe|native_decide)\b', code)
        names.extend(namespace + '.' + name for name in sorted(theorem_names(source)))
        output = run([lean, '-o', str(build / (module + '.olean')),
                      str(HERE / (module + '.lean'))], HERE, env)
        assert 'error:' not in output and 'sorryAx' not in output, module
    audit = build / 'Audit.lean'
    audit.write_text('import OptionMatchEquivalence\nimport WireFrameRefinement\nimport WireSourceBridge\n' +
                     '\n'.join('#print axioms ' + name for name in names) + '\n')
    output = run([lean, str(audit)], build, env)
    blocks = re.findall(r"'([^']+)' (does not depend on any axioms|depends on axioms: \[(.*?)\])",
                        output, re.S)
    assert {name for name, _, _ in blocks} == set(names), 'Incomplete axiom audit'
    for name, _, dependencies in blocks:
        actual = {item.strip() for item in dependencies.split(',') if item.strip()}
        assert actual <= {'propext', 'Quot.sound'}, (name, actual)
    run([sys.executable, str(HERE / 'check_correspondence.py'), '--repo', str(REPO),
         '--output', str(build / 'z3-result.json')])
    run([sys.executable, str(HERE / 'explicit-match/check_option_match.py')])
    run([sys.executable, str(HERE / 'check_external_allocator.py'), '--repo', str(REPO),
         '--prior', str(HERE / 'check_correspondence.py'),
         '--prior-replay', str(build / 'z3-result.json'),
         '--prior-descriptor', str(HERE / 'external-binding/before/DescriptorImage.lean'),
         '--output', str(build / 'external-z3-result.json')])
print(f'PASS wire-frame refinement: {len(names)} Lean theorems; unchanged exact admission; '
      'Z3 and finite source correspondence', flush=True)
print('LIMIT: concrete runtime Covers/SameClassUnique remains an explicit premise', flush=True)
