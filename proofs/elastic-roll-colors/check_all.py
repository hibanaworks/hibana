#!/usr/bin/env python3
"""Replay Lean and Z3 models without historical caches or source identity gates.

Requires Lean 4.30.0 and Python z3-solver (recorded version 5.1.0).
Fresh compiled modules and fresh logs exist only in a temporary directory.
"""
from pathlib import Path
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--lean', default=os.environ.get('LEAN', 'lean'))
parser.add_argument('--skip-compiler-cost', action='store_true', help='Only replay elastic color/no-roll evidence')
args = parser.parse_args()

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
