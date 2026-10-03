#!/usr/bin/env python3
"""Pin source/evidence correspondence; deliberately not compiler refinement."""
from pathlib import Path
import hashlib
import json
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
BASE = ROOT / "hibana-rolled-route-perf-integration"
EXPECTED = "a6339772d4bf2c905f491e0284d3bef36e79bb6f"
def sha(data):
    return hashlib.sha256(data).hexdigest()
def git(*args):
    return subprocess.check_output(["git", "-C", str(BASE), *args])
assert git("rev-parse", "HEAD").decode().strip() == EXPECTED
paths = [
    "src/g/source.rs", "src/global/const_dsl/eff_list.rs",
    "src/global/const_dsl/allocation.rs", "src/global/const_dsl/allocation/frame_labels.rs",
    "src/global/const_dsl/allocation/frame_labels/route.rs",
    "src/global/const_dsl/allocation/frame_labels/roll.rs",
    "src/global/const_dsl/event_relations.rs",
    "src/global/const_dsl/scope_ranges.rs", "src/global/const_dsl/scope_ranges/nesting.rs",
    "src/global/const_dsl/endpoint_selectors.rs",
    "src/global/typestate/facts/inbound_key.rs",
    "src/global/typestate/cursor/scope_route.rs",
    "src/global/typestate/cursor/scope_route/roll.rs",
    "src/global/typestate/cursor/scope_route/roll/nesting.rs",
    "src/global/typestate/cursor/scope_route/roll/reentry.rs",
    "src/global/typestate/cursor/scope_route/navigation.rs",
    "src/global/typestate/cursor/scope_route/event_progress.rs",
    "src/global/typestate/cursor/scope_route/row_completion.rs",
    "src/endpoint/kernel/recv.rs",
    "src/endpoint/kernel/offer/select_observed.rs", "src/runtime_core/unique_match.rs",
]
hashes = {}
for rel in paths:
    source = (BASE / rel).read_bytes()
    assert source == git("show", f"{EXPECTED}:{rel}"), rel
    hashes[f"hibana-rolled-route-perf-integration/{rel}"] = sha(source)
    print(f"PASS pinned unchanged {rel}")

# The donor source checks make cache reuse explicit. They cannot independently
# prove the provenance of a previously built .olean file; preserve that limit.
lean_sources = []
required_modules = {"Commit", "EventGraph", "Generation", "GlobalSemantics", "GlobalSyntax", "OperationAdmission", "Syntax"}
for source in sorted((BASE / "proofs/lean/Hibana").rglob("*.lean")):
    rel = source.relative_to(BASE / "proofs/lean")
    donor = ROOT / "hibana/proofs/lean" / rel
    assert source.read_bytes() == donor.read_bytes(), str(rel)
    artifact = ROOT / "hibana/proofs/lean/.lake/build/lib/lean" / rel.with_suffix(".olean")
    if source.stem in required_modules:
        assert artifact.is_file(), str(artifact)
    record = {"source": str(source.relative_to(ROOT)), "source_sha256": sha(source.read_bytes())}
    if artifact.is_file():
        record.update({"cache": str(artifact.relative_to(ROOT)), "cache_sha256": sha(artifact.read_bytes())})
    lean_sources.append(record)
print(f"PASS {len(lean_sources)} Lean source files match reused module donor")

capacity = json.loads((HERE / "roll-membership-capacity-model.json").read_text())
for rel, expected in capacity["source_hashes"].items():
    source = ROOT / "hibana-quic" / rel
    assert sha(source.read_bytes()) == expected, rel
    hashes[str(source.relative_to(ROOT))] = expected
print("PASS QUIC input source hashes")

inventory = (HERE.parent / "run-inventory.log").read_text()
metas = {}
for match in re.finditer(r"INVENTORY (recv|send) idx=(\d+).*?peer: (\d+), label: (\d+),.*?frame_label: (\d+),.*?lane: (\d+)", inventory):
    kind, index, peer, label, color, lane = match.groups()
    metas[int(index)] = {"kind":kind, "peer":int(peer), "label":int(label), "color":int(color), "lane":int(lane)}
assert sorted(metas) == list(range(10))
assert [metas[i]["label"] for i in range(10)] == [40,41,52,42,78,71,55,72,73,74]
for e in capacity["graphs"]["minimal"]["events"]:
    m = metas[e["id"]]
    assert e["baseline_color"] == m["color"]
    assert e["key"] == ([m["peer"],1,m["lane"]] if m["kind"] == "recv" else [1,m["peer"],m["lane"]])
print("PASS all 10 minimal occurrence IDs/domains/baseline colors match Rust inventory")

logs = ["LeanColorGate-recheck.log", "LeanMinimalTrace.log", "LeanMinimalTrace-recheck.log", "LeanRollMembership.log", "LeanNestedReverse.log"]
for name in logs:
    log = (HERE / name).read_text()
    assert "error:" not in log and "sorryAx" not in log, name
    assert "depends on axioms" in log or "does not depend on any axioms" in log, name
    print(f"PASS final Lean log has theorem output and no error/sorry: {name}")
z3_result = json.loads((HERE / "roll-membership-z3.json").read_text())
assert z3_result["passed"] and len(z3_result["checks"]) == 31
assert all(c["expected"] == c["actual"] for c in z3_result["checks"])
print("PASS all 31 Z3 checks")
for p in [HERE.parent / "run.log", HERE.parent / "run-inventory.log",
          ROOT / "artifacts/elastic-roll-color-fix/security-before-expanded.log",
          ROOT / "hibana-roll-color-fix/tests/security_report_regressions/reentry_colors.rs",
          *[HERE / name for name in logs],
          *[p for p in HERE.iterdir() if p.suffix in {".py", ".lean", ".md"}],
          HERE / "roll-membership-z3.json", HERE / "roll-membership-capacity-model.json"]:
    hashes[str(p.relative_to(ROOT))] = sha(p.read_bytes())
output = {"baseline_commit": EXPECTED,
          "claim": "Source identity and exact finite descriptor correspondence, not proof that Rust implements Lean or universal coeligibility coverage",
          "remaining_obligation": "SameClassUnique for all reachable well-formed unchanged-Rust selector states under proposed wire colors",
          "sha256": hashes, "lean_cache_source_identity": lean_sources,
          "cache_limit": "Matching sources do not independently attest how preexisting compiled modules were produced"}
(HERE / "source-correspondence-final.json").write_text(json.dumps(output, indent=2) + "\n")
print("PASS source-correspondence-final.json written; implementation validation remains separate")
