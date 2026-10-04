"""Proof-harness source identity bridge; never part of the device/runtime.

Permit only the two exact diagnostic-only additions to qualified Rust files.
Do not overwrite old source hashes or silently ignore arbitrary module edits.
"""
from pathlib import Path
import hashlib
import json
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sha = lambda data: hashlib.sha256(data).hexdigest()
FOLLOWUP = json.loads((HERE / 'source-correspondence.json').read_text())['files']
ADDITIONS = {
    'src/global/const_dsl.rs': (
        b'pub(crate) use self::receive_lane_causality::validate_receive_lane_causality;\n',
        b'pub(crate) use self::receive_lane_causality::{\n'
        b'    receive_lane_conflict, validate_receive_lane_causality,\n};\n',
    ),
    'src/global/compiled/lowering/seal.rs': (
        b'    validate_route_projection_guarantees(summary, eff_list)\n}\n',
        b'    validate_route_projection_guarantees(summary, eff_list)\n}\n'
        b'\nmod diagnostic;\npub(crate) use diagnostic::projection_diagnostic;\n',
    ),
}
assert FOLLOWUP.keys() == ADDITIONS.keys()


def qualified_source(repo, relative):
    current = (repo / relative).read_bytes()
    if relative not in FOLLOWUP:
        return current
    entry = FOLLOWUP[relative]
    original = (ROOT / entry['snapshot']).read_bytes()
    assert sha(original) == entry['qualified_sha256'], ('qualified snapshot', relative)
    assert sha(current) == entry['current_sha256'], ('diagnostic followup', relative)
    before, after = ADDITIONS[relative]
    assert original.count(before) == 1, ('unique addition site', relative)
    assert current == original.replace(before, after), ('production bytes changed', relative)
    return original


def check():
    # Actual qualified gate bytes must be preserved, and altered Rust must
    # remain rejected. These fixtures never modify the live checkout.
    for relative in FOLLOWUP:
        original = qualified_source(ROOT, relative)
        assert sha(original) == FOLLOWUP[relative]['qualified_sha256']
        with tempfile.TemporaryDirectory(prefix='hibana-diagnostic-identity-') as temp:
            repo = Path(temp)
            source = repo / relative
            source.parent.mkdir(parents=True)
            current = (ROOT / relative).read_bytes()
            for mutation in [current + b'const UNQUALIFIED: bool = true;\n', original]:
                source.write_bytes(mutation)
                try:
                    qualified_source(repo, relative)
                except AssertionError:
                    continue
                raise AssertionError(('unqualified production change accepted', relative))
    print('PASS exact diagnostic source correspondence: 2 qualified files, 4 rejected mutations')


if __name__ == '__main__':
    check()
