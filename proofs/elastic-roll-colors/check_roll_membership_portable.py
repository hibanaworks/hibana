#!/usr/bin/env python3
"""Portable replay of the exact recorded 31 queries and finite graph checks.

Source hashes in the model are historical inputs, not a fresh source-refinement
proof. Supply --quic-source to separately verify those source files today.
"""
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import re
import z3

HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--quic-source', type=Path, help='Optional checkout matching historical QUIC input hashes')
parser.add_argument('--output', type=Path, help='Write a NEW replay JSON here; never overwrite historical evidence')
args = parser.parse_args()
checks = []
recorded = json.loads((HERE / 'roll-membership-z3.json').read_text())
manifest = json.loads((HERE / 'preserved-artifacts.json').read_text())
inventory = HERE / 'evidence/run-inventory.log'
raw = gzip.decompress((HERE / 'roll-membership-capacity-model.json.gz').read_bytes())
assert hashlib.sha256(raw).hexdigest() == manifest['files']['roll-membership-capacity-model.json.gz']['uncompressed_sha256']
assert hashlib.sha256(inventory.read_bytes()).hexdigest() == recorded['evidence_sha256']['artifacts/current-offer-source-diagnostic/run-inventory.log']
data = json.loads(raw)
source_status = 'recorded identity only; live QUIC source check not requested'
if args.quic_source:
    for rel, expected in data['source_hashes'].items():
        assert hashlib.sha256((args.quic_source / rel).read_bytes()).hexdigest() == expected, rel
    source_status = 'live QUIC source hashes match recorded finite-model inputs'
print('SOURCE CORRESPONDENCE: ' + source_status)

def save(passed):
    if args.output:
        target = args.output.resolve()
        assert not target.exists(), 'Replay output must be a new file'
        target.write_text(json.dumps({
            'passed': passed, 'z3_version': z3.get_version_string(),
            'source_correspondence': source_status,
            'claim_limit': recorded['claim_limit'], 'checks': checks,
        }, indent=2) + '\n')

def check(name, constraints, expected, variables=()):
    solver = z3.Solver()
    solver.set(timeout=30000)
    solver.add(*constraints)
    actual = solver.check()
    record = {"name": name, "expected": str(expected), "actual": str(actual)}
    if actual == z3.sat:
        model = solver.model()
        record["witness"] = {str(v): str(model.eval(v, model_completion=True)) for v in variables}
    if actual == z3.unknown:
        record["reason"] = solver.reason_unknown()
    checks.append(record)
    print(f"{name}: {actual} (expected {expected})", flush=True)
    if actual != expected:
        save(False)
        raise AssertionError(record)


# BEGIN UNCHANGED QUERY CORE
# Parse observed metadata instead of inventing an eligible set.
observed = {}
pattern = r"INVENTORY recv idx=(\d+) enabled=(true|false).*?peer: (\d+), label: (\d+),.*?frame_label: (\d+),.*?lane: (\d+)"
for match in re.finditer(pattern, inventory.read_text()):
    idx, enabled, peer, label, color, lane = match.groups()
    observed[int(idx)] = {"enabled": enabled == "true", "peer": int(peer),
                          "label": int(label), "color": int(color), "lane": int(lane)}
assert set(observed) == {0, 2, 3, 5, 6, 7, 9}
assert observed[2]["label"] == 52 and observed[6]["label"] == 55
old_a, old_b = z3.Ints("old_candidate_a old_candidate_b")
pair_facts = []
for a, av in observed.items():
    for b, bv in observed.items():
        if av["enabled"] and bv["enabled"] and av["peer"] == bv["peer"] and av["lane"] == bv["lane"] and av["color"] == bv["color"]:
            pair_facts.append(z3.And(old_a == a, old_b == b))
observed_collision = [z3.Or(*pair_facts), old_a != old_b]
check("observed_old_wire_collision", observed_collision, z3.sat, [old_a, old_b])
check("exact_Inspect52_Open55_collision", observed_collision + [old_a == 2, old_b == 6], z3.sat, [old_a, old_b])
check("disabled_completed_prefix_is_not_a_witness", observed_collision + [old_a == 0], z3.unsat)
check("different_logical_labels_do_not_prevent_wire_collision", observed_collision + [old_a == 2, old_b == 6, z3.IntVal(observed[2]["label"]) != observed[6]["label"]], z3.sat)
check("old_Inspect_wire_key_accepts_queued_Open_key", [z3.IntVal(observed[2]["peer"]) == observed[6]["peer"], z3.IntVal(observed[2]["lane"]) == observed[6]["lane"], z3.IntVal(observed[2]["color"]) == observed[6]["color"]], z3.sat)

# Abstract graph predicates are symmetric. All premises have a SAT control.
ca, cb, oa, ob, ma, mb = z3.Ints("color_a color_b old_a old_b membership_a membership_b")
same_domain, ea, eb, distinct = z3.Bools("same_domain eligible_a eligible_b distinct_occurrences")
bounds = [ca >= 0, ca < 256, cb >= 0, cb < 256]
edge = z3.And(distinct, same_domain, z3.Or(oa != ob, ma != mb))
valid = z3.Implies(edge, ca != cb)
coverage = z3.Implies(z3.And(distinct, same_domain, ea, eb), edge)
premises = bounds + [distinct, same_domain, ea, eb, coverage, valid]
check("coverage_and_validity_premises_are_satisfiable", premises, z3.sat, [ca, cb, oa, ob, ma, mb])
check("same_wire_distinct_eligible_occurrences_under_coverage", premises + [ca == cb], z3.unsat)
check("remove_coverage_restores_collision_negative_control", bounds + [distinct, same_domain, ea, eb, valid, ca == cb], z3.sat, [ca, cb, oa, ob, ma, mb])
check("preserve_baseline_inequality", bounds + [distinct, same_domain, oa != ob, valid, ca == cb], z3.unsat)
check("different_membership_separation", bounds + [distinct, same_domain, ma != mb, valid, ca == cb], z3.unsat)
check("remove_membership_edge_restores_observed_collision", bounds + [distinct, same_domain, oa == ob, ma != mb, z3.Implies(z3.And(distinct, same_domain, oa != ob), ca != cb), ca == cb], z3.sat)
check("same_membership_same_baseline_can_reuse_color", bounds + [distinct, same_domain, oa == ob, ma == mb, valid, ca == cb], z3.sat)
check("different_domains_can_reuse_color", bounds + [distinct, z3.Not(same_domain), ma != mb, oa != ob, valid, ca == cb], z3.sat)

# Byte palette and overflow: exhaustion refers to this prefix, not chromatic number.
c = z3.Int("next_color")
check("last_byte_color_255_is_available", [c >= 0, c < 256] + [c != k for k in range(255)], z3.sat, [c])
check("all_256_colors_blocked_reports_failure", [c >= 0, c < 256] + [c != k for k in range(256)], z3.unsat)
check("truncated_255_palette_wrongly_exhausts_negative_control", [c >= 0, c < 255] + [c != k for k in range(255)], z3.unsat)
path_colors = z3.Ints("p0 p1 p2 p3")
path_edges = [(0, 1), (1, 3), (2, 3)]
path_constraints = [z3.And(v >= 0, v < 2) for v in path_colors] + [path_colors[a] != path_colors[b] for a, b in path_edges]
check("greedy_exhaustion_does_not_imply_uncolorability", path_constraints, z3.sat, path_colors)
check("same_colorable_graph_bad_greedy_prefix_exhausts", path_constraints + [path_colors[0] == 0, path_colors[1] == 1, path_colors[2] == 0], z3.unsat)

# Independently reconstruct the membership graph from source-derived intervals.
# Keep source-transcription and runtime-correspondence limits explicit.
for name, graph in data["graphs"].items():
    events, scopes = graph["events"], graph["scopes"]
    rolls = [s for s in scopes if s["kind"] == "Roll"]
    members = [{s["id"] for s in rolls if s["start"] <= i < s["end"]} for i in range(len(events))]
    expected_edges = set()
    assigned = []
    for j, b in enumerate(events):
        forbidden = set()
        for i, a in enumerate(events[:j]):
            if a["key"] == b["key"] and a["key"][0] != a["key"][1] and (a["baseline_color"] != b["baseline_color"] or members[i] != members[j]):
                expected_edges.add((i, j))
                forbidden.add(assigned[i])
        free = next((k for k in range(256) if k not in forbidden), None)
        assert free is not None, f"prefix exhausted: {name}/{j}"
        assigned.append(free)
    assert expected_edges == set(map(tuple, graph["edges"])), name
    assert assigned == [e["proposed_color"] for e in events], name
    assert [e["id"] for e in events] == list(range(len(events))), name
    assert all(members[i] == set(e["roll_membership"]) for i, e in enumerate(events)), name
    assert all((members[i] == members[j]) == (a["innermost_roll"] == b["innermost_roll"]) for i, a in enumerate(events) for j, b in enumerate(events)), name
    colors = [z3.Int(f"{name}_{i}") for i in range(len(events))]
    constraints = [v == assigned[i] for i, v in enumerate(colors)]
    constraints += [z3.And(v >= 0, v < 256) for v in colors]
    constraints += [colors[a] != colors[b] for a, b in expected_edges]
    check(f"{name}_concrete_coloring_all_edges", constraints, z3.sat)
    if name == "minimal":
        assert (2, 6) in expected_edges
        check("exact_pair_with_new_graph_cannot_collide", constraints + [colors[2] == colors[6]], z3.unsat)
        check("new_Inspect_wire_key_cannot_accept_queued_Open_key", constraints + [colors[2] == colors[6]], z3.unsat)

# END UNCHANGED QUERY CORE
assert len(checks) == 31
assert [(c['name'], c['expected'], c['actual']) for c in checks] == [
    (c['name'], c['expected'], c['actual']) for c in recorded['checks']
], 'Query order, names, or expected/actual results differ from historical gate'
save(True)
print('PASS exact 31-query replay; universal Rust Covers/SameClassUnique remains unproved')
