#!/usr/bin/env python3
"""Check immutable qualification records separately from current source identity."""
from pathlib import Path
import hashlib
import json
import re
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE.parent / 'projection-diagnostics'))
from source_identity import qualified_source
sha = lambda data: hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_text())


def check_hash(path, expected):
    assert sha(path.read_bytes()) == expected, str(path)


preserved = read_json(HERE / 'integration-preserved.json')
for path, expected_hash in preserved['files'].items():
    check_hash(REPO / path, expected_hash)
print('PASS immutable local and external pre-integration correspondence records')


mapping = read_json(REPO / 'proofs/compiler-participant-mask/test-hygiene-followup.json')
qualified_path = REPO / mapping['qualified_manifest']
check_hash(qualified_path, mapping['qualified_manifest_sha256'])
qualified = read_json(qualified_path)
assert len(qualified['files']) == 7
assert mapping['files'].keys() == qualified['files'].keys()
old_oracle = 'src/global/compiled/lowering/seal/tests/legacy_participant_validation.rs'
new_oracle = 'tests/verification_oracles/participant_validation.rs'
participant = 'src/global/compiled/lowering/seal/tests/participant_mask.rs'
boundary = 'src/global/const_dsl/event_relations/tests.rs'
changed = {old_oracle, participant, boundary}
for source, qualified_hash in qualified['files'].items():
    entry = mapping['files'][source]
    assert entry['qualified_sha256'] == qualified_hash, source
    check_hash(REPO / entry['historical_snapshot'], qualified_hash)
    assert entry['current_path'] == (new_oracle if source == old_oracle else source)
    assert sha(qualified_source(REPO, entry['current_path'])) == entry['current_sha256'], source
    if source not in changed:
        assert entry['current_sha256'] == qualified_hash, source
assert not (REPO / old_oracle).exists()


def old_text(source):
    return (REPO / mapping['files'][source]['historical_snapshot']).read_text()


def tokens(text):
    return re.sub(r'\s+', '', text)


expected = old_text(old_oracle)
expected = expected.replace('legacy_', 'reference_')
assert expected == (REPO / new_oracle).read_text()
expected = old_text(participant).replace('legacy_participant_validation.rs', '../../../../../../tests/verification_oracles/participant_validation.rs')
expected = expected.replace('mod legacy;', 'mod reference;').replace('legacy::legacy_', 'reference::reference_')
assert tokens(expected) == tokens((REPO / participant).read_text())
expected = old_text(boundary)
prefix = '    std::thread::Builder::new()\n        .stack_size(4 * 1024 * 1024)\n        .spawn(|| {\n'
suffix = '        })\n        .unwrap()\n        .join()\n        .unwrap();'
assert expected.count(prefix) == expected.count(suffix) == 1
expected = expected.replace(prefix, '').replace(suffix, '')
expected = expected.replace('compact_boundary_and_large_private_fallback_preserve_relation',
                            'compact_and_wide_input_boundaries_preserve_the_exact_relation')
expected = re.sub(r'\bfallback\b', 'wide', expected)
assert tokens(expected) == tokens((REPO / boundary).read_text())
print('PASS original seven-source qualification and exact test-hygiene-only mapping')

# The old distributable manifest stays independently checkable after updating
# its wrapper and README. No original pre-edit records are reissued.
historical_manifest = read_json(HERE / 'historical/elastic-package-manifest.json')
fixture_followup = read_json(REPO / 'proofs/elastic-roll-colors/test-hygiene-followup.json')
for path, expected_hash in historical_manifest['files'].items():
    if path in ['proofs/elastic-roll-colors/check_all.py', 'proofs/elastic-roll-colors/README.md']:
        actual = HERE / 'historical' / ('elastic-' + Path(path).name)
    elif path == fixture_followup['source']:
        actual = REPO / 'proofs/elastic-roll-colors' / fixture_followup['snapshot']
    else:
        actual = REPO / path
    check_hash(actual, expected_hash)
print('PASS original distributable manifest with archived wrapper and README')

for directory, proof_key, source_key in [
    ('passive-child-window', 'proof_hashes', 'files'),
    ('projection-conflict-reuse', 'proof_sha256', 'source_sha256'),
]:
    root = REPO / 'proofs' / directory
    preserved = read_json(root / 'preserved-artifacts.json')
    for path, expected_hash in preserved['files'].items():
        check_hash(root / path, expected_hash)
    candidate = read_json(root / 'candidate-manifest.json')
    for path, expected_hash in candidate[source_key].items():
        check_hash(REPO / path, expected_hash)
    for path, expected_hash in candidate.get('artifact_sha256', {}).items():
        check_hash(root / path, expected_hash)
    gate = read_json(root / 'pre-edit-gate.json')
    assert gate['gate'] == 'PASS before production edit'
    assert gate['production_edit_applied'] is False
    for path, expected_hash in gate[proof_key].items():
        check_hash(root / path, expected_hash)
    for run in gate['proof_runs']:
        assert run.get('returncode', run.get('exit_code')) == 0
        check_hash(root / (run['name'] + '.log'), run['log_sha256'])
    if directory == 'passive-child-window':
        check_hash(root / 'passive-child-window.patch', candidate['patch_sha256'])
        for path, expected_hash in gate['pre_edit_source_hashes'].items():
            actual = (HERE / 'historical/passive-child-route-before.rs'
                      if path == 'src/global/const_dsl/scope_ranges/route.rs' else REPO / path)
            check_hash(actual, expected_hash)
    else:
        for path, expected_hash in gate['source_sha256_before'].items():
            baseline = root / ('baseline-' + Path(path).name)
            check_hash(baseline if baseline.exists() else REPO / path, expected_hash)
        # Replay the archived reference verifier with its sole checkout-location
        # binding adapted to this repository; its source bytes remain unchanged.
        verifier = root / 'verify_reference.py'
        code = verifier.read_text()
        old_binding = "repo=here.parents[1]/'hibana-projection-conflict-reuse'"
        assert code.count(old_binding) == 1
        code = code.replace(old_binding, 'repo=here.parents[1]')
        exec(compile(code, str(verifier), 'exec'), {'__file__': str(verifier)})
    print('PASS preserved ' + directory + ' pre-edit evidence and exact current candidate sources')

current = read_json(HERE / 'current-source-manifest.json')
paths = list((REPO / 'src').rglob('*.rs')) + list((REPO / 'tests/verification_oracles').rglob('*.rs')) + [
    REPO / 'Cargo.toml', REPO / 'Cargo.lock', REPO / '.github/repo-tests/Cargo.toml']
assert set(current['files']) == {str(path.relative_to(REPO)) for path in paths}
for path, expected_hash in current['files'].items():
    check_hash(REPO / path, expected_hash)
canonical = ''.join(value + '  ' + key + '\n' for key, value in sorted(current['files'].items()))
assert sha(canonical.encode()) == current['tree_sha256']
print('PASS combined source identity: ' + current['tree_sha256'])
