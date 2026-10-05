#!/usr/bin/env python3
"""Pre-edit mathematical gate for the post-structural wire-color refinement.

This is an independent Python/Z3 specification check, not execution or proof of
Rust or Lean. Rust panic is compared with the FIRST invalid-256 reference row;
the total list model may continue, but an invalid result must not be accepted.
No proof here derives concrete runtime Covers/SameClassUnique or graph optimality.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, replace
from datetime import datetime, timezone
import gzip
import itertools
import json
from pathlib import Path
import subprocess
import sys

import z3

HERE = Path(__file__).resolve().parent
DEFAULT_REPO = HERE.parents[1]
PALETTE = 256
INVALID = 256
def utc():
    return datetime.now(timezone.utc).isoformat()


@dataclass(frozen=True)
class Row:
    sender: int
    receiver: int
    lane: int
    frame_label: int


@dataclass(frozen=True)
class Marker:
    start: int
    end: int
    ordinal: int
    kind: str = "Roll"
    is_enter: bool = True


def rust_owners(rows, markers):
    """Direct functional transcription of the current imperative owner loops."""
    owners = [0 for _ in rows]
    for event in range(len(rows)):
        narrowest = (1 << 64) - 1  # finite fixtures use valid usize intervals
        for marker in markers:
            if (marker.is_enter and marker.kind == "Roll"
                    and marker.start <= event and event < marker.end):
                span = marker.end - marker.start
                owner = marker.ordinal + 1
                if span < narrowest or (span == narrowest and owner > owners[event]):
                    narrowest = span
                    owners[event] = owner
    return tuple(owners)


def list_owners(rows, markers):
    """Independent order-independent argmin, rather than Rust's rolling update."""
    def owner(event):
        candidates = [(m.end - m.start, -(m.ordinal + 1)) for m in markers
                      if m.kind == "Roll" and m.is_enter and event in range(m.start, m.end)]
        return -min(candidates)[1] if candidates else 0
    return tuple(map(owner, range(len(rows))))


def rust_functional(rows, markers):
    """The byte-mask Rust algorithm; None failure carries unchanged prior prefix."""
    if not any(m.is_enter and m.kind == "Roll" for m in markers):
        return tuple(rows), None
    original = [r.frame_label for r in rows]
    owners = rust_owners(rows, markers)
    source = list(rows)
    for event in range(len(source)):
        current = source[event]
        if current.sender != current.receiver:
            used = bytearray(32)
            for prior in range(event):
                previous = source[prior]
                if (previous.sender == current.sender
                        and previous.receiver == current.receiver
                        and previous.lane == current.lane
                        and (original[prior] != original[event]
                             or owners[prior] != owners[event])):
                    value = previous.frame_label
                    assert 0 <= value < PALETTE
                    used[value >> 3] |= 1 << (value & 7)
            selected = None
            for value in range(PALETTE):
                if not (used[value >> 3] & (1 << (value & 7))):
                    selected = value
                    break
            if selected is None:
                return tuple(source[:event]), event
            source[event] = replace(current, frame_label=selected)
    return tuple(source), None


def list_reference(rows, markers):
    """Total proposal: immutable row/owner pairs, list scan, sentinel on exhaustion."""
    if not any(m.kind == "Roll" and m.is_enter for m in markers):
        return tuple(rows)
    frozen = tuple(zip(rows, list_owners(rows, markers)))
    assigned = ()
    for current, owner in frozen:
        blocked = [new.frame_label for (old, old_owner), new in zip(frozen, assigned)
                   if (old.sender, old.receiver, old.lane)
                   == (current.sender, current.receiver, current.lane)
                   and (old.frame_label, old_owner) != (current.frame_label, owner)]
        selected = next((c for c in range(PALETTE) if c not in blocked), INVALID)
        assigned += (current if current.sender == current.receiver
                     else replace(current, frame_label=selected),)
    return assigned


def compare(rows, markers):
    assert all(0 <= r.frame_label < PALETTE for r in rows)
    assert all(0 <= m.ordinal <= 0x1FFF and 0 <= m.start <= m.end <= len(rows)
               for m in markers)
    assert rust_owners(rows, markers) == list_owners(rows, markers)
    actual, failed = rust_functional(rows, markers)
    expected = list_reference(rows, markers)
    first_invalid = next((i for i, r in enumerate(expected) if r.frame_label == INVALID), None)
    assert failed == first_invalid, (failed, first_invalid)
    assert actual == (expected if failed is None else expected[:failed])
    assert len(expected) == len(rows)
    assert [(r.sender, r.receiver, r.lane) for r in expected] == [
        (r.sender, r.receiver, r.lane) for r in rows]
    assert all(a == b for a, b in zip(rows, expected) if a.sender == a.receiver)
    if not any(m.kind == "Roll" and m.is_enter for m in markers):
        assert expected == tuple(rows)
    elif failed is None:
        owners = list_owners(rows, markers)
        for j, current in enumerate(rows):
            if current.sender == current.receiver:
                continue
            for i, prior in enumerate(rows[:j]):
                if ((prior.sender, prior.receiver, prior.lane)
                        == (current.sender, current.receiver, current.lane)
                        and (prior.frame_label, owners[i]) != (current.frame_label, owners[j])):
                    assert expected[i].frame_label != expected[j].frame_label
    return expected, failed


def run_z3(check):
    # Arbitrary prior/current rows. Covers/SameClassUnique are explicit premises,
    # never facts inferred from a concrete Rust runtime or a finite fixture.
    ps, pr, pl, cs, cr, cl, oldp, oldc, ownp, ownc, cp, cc = z3.Ints(
        "prior_sender prior_receiver prior_lane current_sender current_receiver current_lane "
        "old_prior old_current owner_prior owner_current color_prior color_current")
    distinct, ep, ec = z3.Bools("distinct_occurrences eligible_prior eligible_current")
    same_domain = z3.And(ps == cs, pr == cr, pl == cl)
    nonself = cs != cr
    class_diff = z3.Or(oldp != oldc, ownp != ownc)
    conflict = z3.And(distinct, same_domain, nonself, class_diff)
    bounded = [cp >= 0, cp < PALETTE, cc >= 0, cc < PALETTE]
    used = z3.Array("blocked", z3.IntSort(), z3.BoolSort())
    scan_fact = z3.Implies(conflict, z3.Select(used, cp))
    free = z3.Not(z3.Select(used, cc))
    step = bounded + [scan_fact, free, distinct, same_domain, nonself]
    check("one_step_premises_sat", step + [oldp != oldc], z3.sat, [cp, cc])
    check("one_step_original_inequality_preserved", step + [oldp != oldc, cp == cc], z3.unsat)
    check("one_step_owner_separation", step + [ownp != ownc, cp == cc], z3.unsat)
    check("missing_block_insertion_negative_control", bounded + [free, conflict, cp == cc],
          z3.sat, [cp, cc])
    check("missing_free_color_guard_negative_control", bounded + [scan_fact, conflict, cp == cc],
          z3.sat, [cp, cc])
    old_only = z3.Implies(z3.And(distinct, same_domain, nonself, oldp != oldc), cp != cc)
    check("omit_owner_edge_negative_control", bounded + [old_only, distinct, same_domain,
          nonself, oldp == oldc, ownp != ownc, cp == cc], z3.sat, [cp, cc])
    check("same_class_color_reuse_is_permitted", step + [oldp == oldc, ownp == ownc, cp == cc],
          z3.sat, [cp, cc])
    check("different_sender_can_reuse", bounded + [scan_fact, free, ps != cs, cp == cc], z3.sat)
    check("different_receiver_can_reuse", bounded + [scan_fact, free, pr != cr, cp == cc], z3.sat)
    check("different_lane_can_reuse", bounded + [scan_fact, free, pl != cl, cp == cc], z3.sat)

    same_class_unique = z3.Implies(z3.And(ep, ec, same_domain, oldp == oldc, ownp == ownc),
                                   z3.Not(distinct))
    covers = z3.Implies(z3.And(distinct, ep, ec, same_domain, nonself), conflict)
    validity = z3.Implies(conflict, cp != cc)
    uniqueness_premises = bounded + [same_class_unique, validity, ep, ec, same_domain, nonself]
    check("same_class_unique_premises_sat", uniqueness_premises + [distinct], z3.sat, [cp, cc])
    check("same_class_unique_implies_covers", [same_class_unique, z3.Not(covers)], z3.unsat)
    check("same_class_reuse_excludes_simultaneously_eligible_distinct", uniqueness_premises
          + [oldp == oldc, ownp == ownc, distinct], z3.unsat)
    check("same_wire_unique_under_same_class_unique", uniqueness_premises + [distinct, cp == cc], z3.unsat)
    check("covers_and_validity_premises_sat", bounded + [covers, validity, distinct, ep, ec,
          same_domain, nonself], z3.sat, [cp, cc])
    check("same_wire_unique_under_covers", bounded + [covers, validity, distinct, ep, ec,
          same_domain, nonself, cp == cc], z3.unsat)
    check("remove_coverage_negative_control", bounded + [validity, distinct, ep, ec,
          same_domain, nonself, oldp == oldc, ownp == ownc, cp == cc], z3.sat, [cp, cc])

    prefix = z3.Array("prior_assignments", z3.IntSort(), z3.IntSort())
    prior_index, current_index = z3.Ints("prior_index current_index")
    updated = z3.Store(prefix, current_index, cc)
    check("one_step_keeps_all_prior_assignments", [prior_index < current_index,
          z3.Select(updated, prior_index) != z3.Select(prefix, prior_index)], z3.unsat)

    # One-step lexicographic minimum covers any prefix-minimal current candidate.
    span, best_span, owner, best_owner = z3.Ints("span best_span owner best_owner")
    eligible = z3.Bool("candidate_contains_row_and_is_roll_enter")
    replace_owner = z3.And(eligible, z3.Or(span < best_span,
                                 z3.And(span == best_span, owner > best_owner)))
    selected_owner = z3.If(replace_owner, owner, best_owner)
    selected_span = z3.If(replace_owner, span, best_span)
    owner_bounds = [span >= 0, best_span >= 0, owner > 0, owner <= 8192,
                    best_owner >= 0, best_owner <= 8192]
    check("narrower_owner_premises_sat", owner_bounds + [eligible, span < best_span,
          owner < best_owner], z3.sat, [span, best_span, owner, best_owner])
    check("narrower_owner_always_wins", owner_bounds + [eligible, span < best_span,
          z3.Or(selected_owner != owner, selected_span != span)], z3.unsat)
    check("equal_span_later_ordinal_premises_sat", owner_bounds + [eligible, span == best_span,
          owner > best_owner], z3.sat, [owner, best_owner])
    check("equal_span_later_ordinal_wins", owner_bounds + [eligible, span == best_span,
          owner > best_owner, selected_owner != owner], z3.unsat)
    check("broader_owner_cannot_override", owner_bounds + [eligible, span > best_span,
          selected_owner != best_owner], z3.unsat)
    check("noncontaining_or_nonroll_candidate_ignored", owner_bounds + [z3.Not(eligible),
          selected_owner != best_owner], z3.unsat)
    tie_omitted = z3.If(z3.And(eligible, span < best_span), owner, best_owner)
    check("omit_equal_span_tie_break_negative_control", owner_bounds + [eligible,
          span == best_span, owner > best_owner, tie_omitted != selected_owner], z3.sat,
          [owner, best_owner])
    check("local_ordinal_plus_one_is_nonzero_u16", [owner >= 0, owner <= 8191,
          z3.Or(owner + 1 <= 0, owner + 1 > 65535)], z3.unsat)

    has_roll = z3.Bool("has_roll")
    baseline, refined = z3.Ints("baseline_label refined_label")
    final = z3.If(has_roll, z3.If(cs == cr, baseline, refined), baseline)
    check("no_roll_identity_premises_sat", [z3.Not(has_roll), baseline == 255], z3.sat, [final])
    check("no_roll_identity", [z3.Not(has_roll), final != baseline], z3.unsat)
    check("self_send_identity_premises_sat", [has_roll, cs == cr, baseline != refined], z3.sat)
    check("self_send_identity", [has_roll, cs == cr, final != baseline], z3.unsat)

    # Exact first-available byte palette encoded independently as a nested ITE.
    palette_used = [z3.Bool(f"used_{c}") for c in range(PALETTE)]
    picked = z3.IntVal(INVALID)
    for color in reversed(range(PALETTE)):
        picked = z3.If(palette_used[color], picked, z3.IntVal(color))
    all_used = z3.And(*palette_used)
    check("palette_premises_sat", [z3.Not(all_used)], z3.sat)
    check("total_choice_bounded_or_invalid_sentinel", [z3.Or(picked < 0, picked > INVALID)], z3.unsat)
    check("sentinel_iff_all_256_blocked", [z3.Xor(picked == INVALID, all_used)], z3.unsat)
    check("chosen_byte_is_available", [z3.Or(*[z3.And(picked == c, palette_used[c])
          for c in range(PALETTE)])], z3.unsat)
    check("chosen_byte_is_least_available", [z3.Or(*[z3.And(picked > c, z3.Not(palette_used[c]))
          for c in range(PALETTE)])], z3.unsat)
    check("last_byte_255_is_available", palette_used[:255] + [z3.Not(palette_used[255]),
          picked == 255], z3.sat, [picked])
    check("all_bytes_blocked_yield_invalid_256", palette_used + [picked != INVALID], z3.unsat)
    truncated = z3.IntVal(INVALID)
    for color in reversed(range(255)):
        truncated = z3.If(palette_used[color], truncated, z3.IntVal(color))
    check("truncated_palette_negative_control", palette_used[:255]
          + [z3.Not(palette_used[255]), picked == 255, truncated == INVALID], z3.sat,
          [picked, truncated])
    # This arbitrary path graph demonstrates why the generic failure contract
    # must describe the fixed prefix, not claim optimal graph coloring.
    p0, p1, p2, p3 = z3.Ints("path0 path1 path2 path3")
    path = [z3.And(p >= 0, p < 2) for p in [p0, p1, p2, p3]]
    path += [p0 != p1, p1 != p3, p2 != p3]
    check("two_colorable_graph_exists", path, z3.sat, [p0, p1, p2, p3])
    check("same_graph_fixed_greedy_prefix_exhausts", path + [p0 == 0, p1 == 1, p2 == 0], z3.unsat)


def run_finite(repo):
    records = []
    base = Row(0, 1, 0, 0)
    fixtures = {
        "ordinary_257_no_roll": ([replace(base, frame_label=i % 256) for i in range(257)], []),
        "separate_rolls": ([base] * 4, [Marker(i, i + 1, i) for i in range(4)]),
        "nested_and_coextensive": ([base] * 3,
                                    [Marker(1, 2, 2), Marker(1, 2, 1), Marker(0, 3, 0)]),
        "domains_and_original_labels": ([base, replace(base, frame_label=1), Row(2, 1, 0, 0),
                                          Row(0, 2, 0, 0), Row(0, 1, 1, 0)], [Marker(0, 5, 0)]),
        "self_sends_preserve_labels": ([Row(0, 0, 0, 255), base, Row(0, 0, 0, 19), base],
                                       [Marker(0, 2, 0), Marker(2, 4, 1)]),
        "half_open_boundary_and_nonroll_markers": ([base] * 5,
            [Marker(1, 3, 1), Marker(0, 5, 2, "Route"), Marker(3, 3, 3),
             Marker(0, 5, 4, "Roll", False)]),
        "full_256_palette": ([base] * 256, [Marker(i, i + 1, i) for i in range(256)]),
        "257th_reports_first_failure": ([base] * 257, [Marker(i, i + 1, i) for i in range(257)]),
        "total_reference_continues_after_invalid": ([base] * 257 + [Row(1, 2, 0, 0)],
            [Marker(i, i + 1, i) for i in range(258)]),
    }
    for name, (rows, markers) in fixtures.items():
        expected, failed = compare(rows, markers)
        records.append({"name": name, "rows": len(rows), "markers": len(markers),
                        "first_failure": failed,
                        "colors": [r.frame_label for r in expected]})
    assert records[-3]["colors"] == list(range(256))
    assert records[-2]["first_failure"] == 256 and records[-2]["colors"][-1] == INVALID
    assert records[-1]["first_failure"] == 256 and records[-1]["colors"][-1] == 0

    # Exhaustive over these 7 row variants and all listed laminar layouts for
    # lengths 0..4, including every marker ordering in each layout.
    variants = [base, replace(base, frame_label=1), replace(base, frame_label=255),
                Row(0, 1, 1, 0), Row(0, 2, 0, 0), Row(2, 1, 0, 0), Row(0, 0, 0, 255)]
    exhaustive = 0
    layouts = {}
    for n in range(5):
        nlayouts = {"ordinary": [], "one_roll": [Marker(0, n, 0)],
                    "coextensive": [Marker(0, n, 0), Marker(0, n, 1)]}
        if n >= 2:
            nlayouts["nested"] = [Marker(0, n, 0), Marker(1, n, 1)]
            nlayouts["disjoint"] = [Marker(0, 1, 0), Marker(1, n, 1)]
        if n >= 3:
            nlayouts["nested_coextensive"] = [Marker(0, n, 0), Marker(1, n - 1, 1),
                                               Marker(1, n - 1, 2)]
        for layout, markers in nlayouts.items():
            permutations = list(itertools.permutations(markers))
            count = 0
            for rows in itertools.product(variants, repeat=n):
                for permuted in permutations:
                    compare(rows, permuted)
                    count += 1
            layouts[f"{n}:{layout}"] = count
            exhaustive += count

    # Reconstruct all recorded finite source models without claiming fresh
    # compiler/runtime coverage. Their IDs are historical preorder ordinals.
    graph_path = repo / "proofs/elastic-roll-colors/roll-membership-capacity-model.json.gz"
    historical = json.loads(gzip.decompress(graph_path.read_bytes()))
    historical_records = []
    for name, graph in historical["graphs"].items():
        rows = [Row(*e["key"], e["baseline_color"]) for e in graph["events"]]
        markers = [Marker(s["start"], s["end"], s["id"], s["kind"]) for s in graph["scopes"]]
        expected, failed = compare(rows, markers)
        assert failed is None, name
        assert [r.frame_label for r in expected] == [e["proposed_color"] for e in graph["events"]], name
        owners = list_owners(rows, markers)
        assert list(owners) == [0 if e["innermost_roll"] is None else e["innermost_roll"] + 1
                                for e in graph["events"]], name
        memberships = [frozenset(s["id"] for s in graph["scopes"]
                       if s["kind"] == "Roll" and s["start"] <= i < s["end"])
                       for i in range(len(rows))]
        for i, a in enumerate(owners):
            for j, b in enumerate(owners):
                assert (a == b) == (memberships[i] == memberships[j]), (name, i, j)
        historical_records.append({"name": name, "rows": len(rows), "markers": len(markers),
                                   "owners_and_colors_match_historical_model": True})

    # Executed mutant algorithms must disagree on concrete witnesses, rather
    # than merely asserting hand-written purported counterexample outputs.
    def mutant(rows, markers, defect):
        assigned = []
        owners = list_owners(rows, markers)
        for j, row in enumerate(rows):
            blocked = []
            for i, old in enumerate(rows[:j]):
                old_color = (assigned[i] if defect == "mutated_baseline" else old.frame_label)
                old_diff = old_color != row.frame_label and defect != "omit_original"
                owner_diff = owners[i] != owners[j] and defect != "omit_owner"
                if ((old.sender, old.receiver, old.lane) == (row.sender, row.receiver, row.lane)
                        and (old_diff or owner_diff)):
                    blocked.append(assigned[i])
            assigned.append(next((c for c in range(PALETTE) if c not in blocked), INVALID))
        return assigned

    controls = []
    rows = [replace(base, frame_label=7), replace(base, frame_label=7)]
    markers = [Marker(0, 2, 0)]
    expected, _ = compare(rows, markers)
    # Reading the previously mutated label as the baseline introduces a false
    # inequality: row 0 changes 7 -> 0, causing incorrect [0,1] instead of [0,0].
    assert [r.frame_label for r in expected] == [0, 0]
    mutated = mutant(rows, markers, "mutated_baseline")
    assert mutated == [0, 1]
    controls.append({"name": "mutated_baseline_instead_of_frozen", "correct": [0, 0],
                     "mutant": mutated, "frozen_old_labels": [7, 7]})
    rows = [base, replace(base, frame_label=1)]
    markers = [Marker(0, 2, 0)]
    expected, _ = compare(rows, markers)
    assert [r.frame_label for r in expected] == [0, 1]
    mutated = mutant(rows, markers, "omit_original")
    assert mutated == [0, 0]
    controls.append({"name": "omit_original_inequality", "correct": [0, 1], "mutant": mutated})
    rows, markers = [base, base], [Marker(0, 1, 0), Marker(1, 2, 1)]
    expected, _ = compare(rows, markers)
    assert [r.frame_label for r in expected] == [0, 1]
    mutated = mutant(rows, markers, "omit_owner")
    assert mutated == [0, 0]
    controls.append({"name": "omit_owner_difference", "correct": [0, 1], "mutant": mutated})
    return {"named_fixtures": records, "exhaustive_cases": exhaustive,
            "exhaustive_row_variants": [r.__dict__ for r in variants],
            "exhaustive_layout_counts": layouts, "historical_models": historical_records,
            "negative_controls": controls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=DEFAULT_REPO)
    parser.add_argument("--output", type=Path, default=HERE / "result.json")
    args = parser.parse_args()
    assert not args.output.exists(), "Refusing to replace proof evidence; choose a new output"
    repo = args.repo.resolve()
    report = {
        "passed": False, "started_utc": utc(), "command": [sys.executable, *sys.argv],
        "repository": str(repo), "head": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
        "z3_version": z3.get_version_string(), "python_version": sys.version,
        "phase": "post-structural lowering, frozen old sender/receiver/lane/frame label and selected owner",
        "assumptions": [
            "Input rows have byte frame labels; scope bounds are valid half-open intervals.",
            "Scope ordinals are unique source preorder ordinals in 0..8191; containing rolls are laminar.",
            "Equal-span nested rolls increase local ordinal with depth; owner 0 means no enclosing roll.",
            "Abstract same-wire uniqueness requires Covers or SameClassUnique for actual eligible nonself rows.",
            "Rust-equivalent acceptance requires no invalid 256 label in the total reference result.",
        ],
        "claim_limits": [
            "No universal Rust correctness, Rust-to-Lean refinement, or concrete runtime coverage proof.",
            "Python algorithms are manually reviewed functional transcriptions, not extracted executable Rust or Lean.",
            "Z3 proves local mathematical obligations; finite enumeration only covers the documented inputs.",
            "Historical source models retain their original transcription and reachability limitations.",
            "Capacity failure is exact for the fixed greedy prefix; no optimal graph-coloring claim.",
            "No production code edits, Cargo run, or Lean compilation is part of this isolated pre-edit gate.",
        ],
        "z3_checks": [],
    }

    def check(name, constraints, expected, variables=()):
        solver = z3.Solver()
        solver.set(timeout=30000)
        solver.add(*constraints)
        actual = solver.check()
        record = {"name": name, "expected": str(expected), "actual": str(actual)}
        if actual == z3.sat:
            model = solver.model()
            # Avoid serializing the full nested palette ITE as a key.
            record["witness"] = {str(v) if len(str(v)) < 200 else f"expression_{i}":
                                 str(model.eval(v, model_completion=True))
                                 for i, v in enumerate(variables)}
        if actual == z3.unknown:
            record["reason"] = solver.reason_unknown()
        report["z3_checks"].append(record)
        print(f"{name}: {actual} (expected {expected})", flush=True)
        assert actual == expected, record

    try:
        run_z3(check)
        report["finite_equivalence"] = run_finite(repo)
        report["passed"] = True
    except BaseException as error:
        report["error"] = repr(error)
        raise
    finally:
        report["finished_utc"] = utc()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    finite = report["finite_equivalence"]
    print(f"PASS {len(report['z3_checks'])} Z3 obligations/controls; "
          f"{finite['exhaustive_cases']} bounded exhaustive equivalence cases; "
          f"{len(finite['named_fixtures'])} named fixtures; "
          f"{len(finite['historical_models'])} historical finite models", flush=True)
    print("LIMIT: conditional mathematical and finite source-transcription evidence only; "
          "not universal Rust/Lean refinement or concrete runtime coverage", flush=True)


if __name__ == "__main__":
    main()
