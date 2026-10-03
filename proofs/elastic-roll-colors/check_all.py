#!/usr/bin/env python3
"""Verify preserved evidence, then replay Lean and Z3 without historical caches.

Requires Lean 4.30.0 and Python z3-solver (recorded version 5.1.0).
Fresh compiled modules and fresh logs exist only in a temporary directory.
"""
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sha = lambda data: hashlib.sha256(data).hexdigest()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--lean', default=os.environ.get('LEAN', 'lean'))
parser.add_argument('--quic-source', type=Path, help='Optional checkout for live historical QUIC source hashes')
parser.add_argument('--skip-compiler-cost', action='store_true', help='Only replay elastic color/no-roll evidence')
args = parser.parse_args()

# Immutable historical records are checked before any theorem replay.
manifest = json.loads((HERE / 'preserved-artifacts.json').read_text())
for rel, entry in manifest['files'].items():
    raw = (HERE / rel).read_bytes()
    assert sha(raw) == entry['sha256'], rel
    if 'uncompressed_sha256' in entry:
        unpacked = gzip.decompress(raw)
        assert sha(unpacked) == entry['uncompressed_sha256'], rel
        assert len(unpacked) == entry['uncompressed_bytes'], rel
for record, key in [('pre-edit-proof-gate.json', 'proof_hashes'), ('no-roll-pre-edit.json', 'files')]:
    for rel, expected in json.loads((HERE / record).read_text())[key].items():
        assert sha((HERE / rel).read_bytes()) == expected, (record, rel)
original = (HERE / 'historical/check_roll_membership_gate.py').read_text()
portable = (HERE / 'check_roll_membership_portable.py').read_text()
expected = json.loads((HERE / 'query-equivalence.json').read_text())
check = original[original.index('def check('):original.index('\ndef digest(')]
core = original[original.index('# Parse observed metadata'):original.index('\nsave(True)')]
assert sha(original.encode()) == expected['original_checker_sha256']
assert sha(check.encode()) == expected['original_check_function_sha256']
assert sha(core.encode()) == expected['query_core_sha256']
assert portable.count(check) == 1 and portable.count(core) == 1
print('PASS immutable pre-edit records, compressed inputs, and byte-identical 31-query core', flush=True)

source_manifest = json.loads((HERE / 'source-manifest.json').read_text())
test_followup = json.loads((HERE / 'test-hygiene-followup.json').read_text())
test_source = 'tests/security_report_regressions/reentry_colors.rs'
assert test_followup['source'] == test_source
original_fixture = (HERE / test_followup['snapshot']).read_bytes()
assert sha(original_fixture) == test_followup['original_sha256']
assert source_manifest['implementation_snapshot'][test_source] == test_followup['original_sha256']
assert original_fixture.count(b'        drop(pending);\n') == 1
for rel, expected_hash in source_manifest['implementation_snapshot'].items():
    current_source = (REPO / rel).read_bytes()
    if rel == test_source:
        assert sha(current_source) == test_followup['current_sha256']
        assert current_source == original_fixture.replace(b'        drop(pending);\n', b'')
    else:
        assert sha(current_source) == expected_hash, ('implementation snapshot', rel)
print('PASS exact elastic allocator implementation and regression source identity', flush=True)
for rel, expected_hash in source_manifest['lean_dependencies'].items():
    assert sha((REPO / rel).read_bytes()) == expected_hash, rel
    historical = next(record for record in json.loads(
        (HERE / 'source-correspondence-final.json').read_text()
    )['lean_cache_source_identity'] if record['source'].endswith('/' + rel))
    assert historical['source_sha256'] == expected_hash, rel
print('PASS exact Lean dependency source identity', flush=True)

if not args.skip_compiler_cost:
    for directory in ['compiler-participant-mask', 'route-path-refinement']:
        root = HERE.parent / directory
        preserved = json.loads((root / 'preserved-artifacts.json').read_text())
        for rel, expected_hash in preserved['files'].items():
            assert sha((root / rel).read_bytes()) == expected_hash, (directory, rel)
    qualified = json.loads((HERE.parent / 'route-path-refinement/qualified-source-manifest.json').read_text())
    assert len(qualified['files']) == 7
    correspondence_root = HERE.parent / 'route-path-refinement'
    amendments = json.loads((correspondence_root / 'fixture-correspondence.json').read_text())['files']
    assert len(amendments) == 3
    def rust_tokens(source):
        return [token for token in re.findall(
            r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|[0-9]+|[^\s]',
            source, re.S) if not token.startswith(('//', '/*'))]
    for rel, expected_hash in qualified['files'].items():
        if rel not in amendments:
            assert sha((REPO / rel).read_bytes()) == expected_hash, rel
            continue
        amendment = amendments[rel]
        current_path = amendment['current_path']
        assert ('/tests/' in rel or rel.endswith('/tests.rs')) and (
            '/tests/' in current_path or current_path.endswith('/tests.rs') or current_path.startswith('tests/')), rel
        original = (correspondence_root / amendment['original']).read_bytes()
        assert sha(original) == expected_hash, ('immutable qualified fixture', rel)
        current = (REPO / current_path).read_bytes()
        assert sha(current) == amendment['sha256'], ('current fixture', current_path)
        expected = original.decode()
        kind = amendment['transformation']
        if kind == 'oracle':
            expected = expected.replace('legacy_', 'reference_')
        elif kind == 'import':
            expected = expected.replace('"legacy_participant_validation.rs"',
                '"../../../../../../tests/verification_oracles/participant_validation.rs"')
            expected = expected.replace('legacy', 'reference')
        elif kind == 'stack':
            wrapper = 'std::thread::Builder::new()\n        .stack_size(4 * 1024 * 1024)\n        .spawn(|| {'
            ending = '        })\n        .unwrap()\n        .join()\n        .unwrap();'
            assert expected.count(wrapper) == 1 and expected.count(ending) == 1
            expected = expected.replace(wrapper, '').replace(ending, '')
            expected = expected.replace('compact_boundary_and_large_private_fallback_preserve_relation',
                'compact_and_wide_input_boundaries_preserve_the_exact_relation')
            expected = re.sub(r'\bfallback\b', 'wide', expected)
        else:
            raise AssertionError(('unknown fixture transformation', kind))
        assert rust_tokens(current.decode()) == rust_tokens(expected), ('fixture assertions or semantics changed', rel)
    assert set(amendments).issubset(qualified['files'])
    print('PASS exact compiler-cost artifacts and qualified seven-source manifest', flush=True)
    print('PASS fixture source correspondence; all assertions and production bytes preserved', flush=True)
    # The original seven-source manifest remains immutable. Three test files
    # have a separately checked hygiene-only mapping to this current tree.
    subprocess.run([sys.executable, str(HERE.parent / 'core-followup/check_sources.py')],
                   check=True)
    print('PASS exact compiler-cost artifacts, historical source identity, and current followup mapping', flush=True)

version = subprocess.check_output([args.lean, '--version'], text=True).strip()
assert 'version 4.30.0' in version, version
print(version, flush=True)

def run(command, cwd, env=None):
    completed = subprocess.run(command, cwd=cwd, env=env, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, timeout=300)
    print(completed.stdout, end='', flush=True)
    assert completed.returncode == 0, (command, completed.returncode)
    if command[0] == args.lean:
        assert 'sorryAx' not in completed.stdout and 'error:' not in completed.stdout, command

with tempfile.TemporaryDirectory(prefix='hibana-elastic-proof-') as temp:
    build = Path(temp)
    env = dict(os.environ, LEAN_PATH=str(build))
    # Compile dependency closure from checked-in source, never a cached donor.
    for module in ['Syntax', 'GlobalSyntax', 'EventGraph', 'Commit',
                   'OperationAdmission', 'Generation', 'GlobalSemantics']:
        out = build / 'Hibana' / (module + '.olean')
        out.parent.mkdir(parents=True, exist_ok=True)
        run([args.lean, '-o', str(out), 'Hibana/' + module + '.lean'], REPO / 'proofs/lean', env)
    for module in ['LeanColorGate', 'LeanMinimalTrace', 'LeanRollMembership',
                   'LeanNestedReverse', 'NoRollFastPath']:
        run([args.lean, '-o', str(build / (module + '.olean')), module + '.lean'], HERE, env)
    command = [sys.executable, str(HERE / 'check_roll_membership_portable.py')]
    if args.quic_source:
        command += ['--quic-source', str(args.quic_source.resolve())]
    run(command, HERE)
    run([sys.executable, str(HERE / 'check_no_roll.py')], HERE)
    if not args.skip_compiler_cost:
        for directory, lean_files, scripts in [
            ('compiler-participant-mask', ['ParticipantMask'], ['check_participant_mask.py']),
            ('route-path-refinement', ['RoutePathRefinement', 'ProcessedCardinality'],
             ['prove_unbounded_loop.py', 'check_route_path_refinement.py']),
            ('passive-child-window', ['PassiveChildWindow'], ['check_window.py']),
            ('projection-conflict-reuse', ['ConflictReuse'], ['check_conflict_reuse.py']),
        ]:
            root = HERE.parent / directory
            # The original unbounded script writes JSON. Run exact copies in temp.
            replay = build / directory
            replay.mkdir()
            for name in lean_files:
                run([args.lean, str(root / (name + '.lean'))], root, env)
            for script in scripts:
                shutil.copyfile(root / script, replay / script)
                run([sys.executable, str(replay / script)], replay)
print('PASS portable proof replay; Covers/SameClassUnique is conditional, not a universal Rust theorem')
