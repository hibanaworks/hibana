#!/usr/bin/env python3
"""Proof-only qualification of the external final-source allocator.

Manually reviewed Python transcriptions and Z3 obligations, not execution of
Lean/Rust or a universal compiler refinement proof. Never edits source inputs.
The historical checker is imported unchanged and its fresh --repo replay must
already exist. New evidence must use a fresh output path.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, replace
from datetime import datetime, timezone
import gzip
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import re
import subprocess
import sys

import z3

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
HEAD = "731f910a2c8e30844598cd056ab077522333d83c"
PINS = {
    "src/global/const_dsl/allocation/frame_labels/reentry_domains.rs":
        "da90ce98dec6fc89b1bfc4e281436aa84be3435e5eaf5031a001b70d771fccf8",
    "src/global/const_dsl/allocation/frame_labels.rs":
        "219063748f98b22522909c6d083228f1ffddc7bc8701784d858cce2f72f700e2",
    "src/global/const_dsl/scope.rs":
        "a62642e819e5d4fceb6aa2a596ef30336902d4873926cd58956aefe82dfee64f",
    "src/g/source.rs":
        "79279da8bd3904350757cc01ac039c6d6fc88bacbbcc2e61bb19abe40ab26aae",
    "proofs/lean/Hibana/DescriptorImage.lean":
        "bfc5e91f0125654456dac7ddc2bf5eea4c1628f3f859905207da66824e76e10e",
    "proofs/lean/Hibana/DescriptorRefinement.lean":
        "a36d949014f29273f937261a28596c7af5f9e28f2f4ab44cbd6e7116b07c1f6c",
    "proofs/lean/Hibana/Syntax.lean":
        "4604c81cbbcffbbbe5113a81c1e2272333e57dc9e535555f14b05160a23ce419",
    "proofs/lean/Hibana/GlobalSyntax.lean":
        "4ffee9eb9d987bfd11a20388d611c5008e0b991067bff30f0d7b756de3c25338",
}
PRIOR_CHECKER_SHA = "334b47f2bc5f0fac25e439a07fe584f3bff8c213c6b0036486ad46b0dbd8c4bb"
PRIOR_DESCRIPTOR_SHA = "9c239bf4a3c0e428a18b2b902c2680dca51c48ed90a51fa5a516021f453d0127"


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


@dataclass(frozen=True)
class RawMarker:
    offset: int
    scope: int
    tag: int

    def roll_enter(self):
        return self.scope // 8192 == 1 and self.tag % 4 == 0


def closing(marker, other):
    return other.scope == marker.scope and other.tag % 4 == 2


def external_owners(rows, markers):
    """DescriptorImage.elasticFrameOwner: full-list find?, missing exit ignored."""
    result = []
    for index in range(len(rows)):
        best = None
        for marker in markers:
            if marker.roll_enter() and marker.offset <= index:
                exit_marker = next((m for m in markers if closing(marker, m)), None)
                if exit_marker is not None and index < exit_marker.offset:
                    span = max(0, exit_marker.offset - marker.offset)  # Lean Nat subtraction
                    owner = marker.scope % 8192 + 1
                    if best is None or span < best[0] or (span == best[0] and best[1] < owner):
                        best = (span, owner)
        result.append(0 if best is None else best[1])
    return tuple(result)


def former_owners(rows, markers):
    """Former rollFrameOwner/scopeSegmentEnd: suffix find?, missing exit -> length."""
    result = []
    for event in range(len(rows)):
        candidates = []
        for position, marker in enumerate(markers):
            stop = next((m.offset for m in markers[position + 1:] if closing(marker, m)), len(rows))
            if marker.roll_enter() and marker.offset <= event < stop:
                candidates.append((stop - marker.offset, -(marker.scope % 8192 + 1)))
        result.append(-min(candidates)[1] if candidates else 0)
    return tuple(result)


def well_formed_matching_exits(markers):
    """Explicit bridge premise, checked for every Roll enter, not inferred from bytes."""
    return all(len(matches := [j for j, m in enumerate(markers) if closing(marker, m)]) == 1
               and matches[0] > i
               for i, marker in enumerate(markers) if marker.roll_enter())


def raw_markers(intervals):
    result = []
    for m in intervals:
        scope = {"Route": 0, "Roll": 8192, "Parallel": 16384, "Par": 16384}[m.kind] + m.ordinal
        if m.is_enter:
            result.extend([RawMarker(m.start, scope, 0), RawMarker(m.end, scope, 2)])
        else:
            result.append(RawMarker(m.end, scope, 2))
    return tuple(result)


def allocate(rows, markers, owners, reverse_prior=False):
    """External append/filterMap; switch only changes former reverse/filter-map order."""
    if not any(m.roll_enter() for m in markers):
        return tuple(rows)
    prior = []
    result = []
    for atom, owner in zip(rows, owners):
        used = [color for old, old_owner, color in prior
                if (old.sender, old.receiver, old.lane) == (atom.sender, atom.receiver, atom.lane)
                and (old.frame_label != atom.frame_label or old_owner != owner)]
        color = atom.frame_label if atom.sender == atom.receiver else next(
            (c for c in range(256) if c not in used), 256)
        result.append(replace(atom, frame_label=color))
        entry = (atom, owner, color)
        prior = [entry] + prior if reverse_prior else prior + [entry]
    return tuple(result)


def compare_valid(prior, rows, intervals):
    markers = raw_markers(intervals)
    assert well_formed_matching_exits(markers)
    # Pins the interval<->raw bridge explicitly: matching exit offset is stored segment_end.
    expected_owners = prior.rust_owners(rows, intervals)
    ext = external_owners(rows, markers)
    old = former_owners(rows, markers)
    assert ext == old == expected_owners
    result = allocate(rows, markers, ext)
    assert result == allocate(rows, markers, old, reverse_prior=True)
    expected, failed = prior.compare(rows, intervals)
    assert result == expected
    return result, failed


def run_z3(check):
    # Unbounded matching-index abstraction, with all matching exits unique and later.
    n, enter, unique, full, suffix, j = z3.Ints("marker_count enter unique_exit full_first suffix_first j")
    hits = z3.Array("matches_closing_scope_and_tag", z3.IntSort(), z3.BoolSort())
    offsets = z3.Array("marker_offsets", z3.IntSort(), z3.IntSort())
    domain = [n > 0, enter >= 0, enter < unique, unique < n]
    unique_hit = z3.ForAll([j], z3.Implies(z3.And(j >= 0, j < n),
                                        z3.Select(hits, j) == (j == unique)))
    full_first = [full >= 0, full < n, z3.Select(hits, full),
                  z3.ForAll([j], z3.Implies(z3.And(j >= 0, j < full), z3.Not(z3.Select(hits, j))))]
    suffix_first = [suffix > enter, suffix < n, z3.Select(hits, suffix),
                    z3.ForAll([j], z3.Implies(z3.And(j > enter, j < suffix), z3.Not(z3.Select(hits, j))))]
    bridge = domain + [unique_hit] + full_first + suffix_first
    check("unique_exit_after_enter_premises_sat", bridge, z3.sat, [n, enter, unique, full, suffix])
    check("full_list_and_suffix_find_same_exit_under_premise", bridge + [full != suffix], z3.unsat)
    check("matching_segment_end_offsets_equal_under_premise", bridge + [
        z3.Select(offsets, full) != z3.Select(offsets, suffix)], z3.unsat)
    # This checks existence too, not just equality of two assumed successful finds.
    check("unique_exit_is_in_full_and_suffix_search_domains", domain + [unique_hit,
        z3.Or(z3.Not(z3.Select(hits, unique)), unique <= enter, unique >= n)], z3.unsat)
    check("earlier_duplicate_exit_breaks_bridge_negative_control", [n == 3, enter == 1,
        full == 0, suffix == 2, z3.Select(hits, 0), z3.Not(z3.Select(hits, 1)),
        z3.Select(hits, 2)] + full_first + suffix_first + [full != suffix], z3.sat,
        [enter, full, suffix])
    # Absent close is different even with a syntactically valid Roll enter.
    atom_count, event, start, ordinal = z3.Ints("atom_count event start ordinal")
    missing = [atom_count > 0, start >= 0, start <= event, event < atom_count,
               ordinal >= 0, ordinal < 8192]
    check("missing_exit_external_zero_former_fallback_owner_negative_control",
          missing + [ordinal + 1 != 0], z3.sat, [atom_count, event, ordinal])

    # Arbitrary head plus abstract tail membership is the induction step for
    # filterMap membership. Commutativity gives reverse/append equivalence.
    candidate, color = z3.Ints("candidate_color entry_color")
    passes, tail_forward, tail_reverse = z3.Bools("entry_conflicts tail_forward tail_reverse")
    head_member = z3.And(passes, color == candidate)
    forward_member = z3.Or(head_member, tail_forward)
    reverse_member = z3.Or(tail_reverse, head_member)
    check("prior_color_membership_induction_premises_sat", [tail_forward == tail_reverse,
          passes, color == candidate, z3.Not(tail_forward)], z3.sat, [color, candidate])
    check("append_filterMap_reverse_filter_map_membership_step", [tail_forward == tail_reverse,
          forward_member != reverse_member], z3.unsat)
    check("empty_prior_membership_base", [z3.BoolVal(False) != z3.BoolVal(False)], z3.unsat)
    check("drop_matching_assignment_negative_control", [passes, color == candidate,
          z3.Not(tail_forward), forward_member != tail_forward], z3.sat, [color, candidate])
    # Exact finite symbolic list construction separately exercises duplicates,
    # arbitrary natural colors (including sentinel) and every prefix length 0..8.
    for length in range(9):
        colors = [z3.Int(f"prefix_{length}_color_{i}") for i in range(length)]
        filters = [z3.Bool(f"prefix_{length}_filter_{i}") for i in range(length)]
        chronological = z3.Or(*[z3.And(p, c == candidate) for p, c in zip(filters, colors)])
        reversed_prior = z3.Or(*[z3.And(filters[i], colors[i] == candidate)
                                for i in reversed(range(length))])
        check(f"symbolic_prior_set_equivalence_length_{length}",
              [chronological != reversed_prior], z3.unsat)
    # First available depends only on set membership, not used-list order.
    used_forward = [z3.Bool(f"chronological_used_{c}") for c in range(256)]
    used_reverse = [z3.Bool(f"reverse_used_{c}") for c in range(256)]
    def choose(used):
        value = z3.IntVal(256)
        for c in reversed(range(256)):
            value = z3.If(used[c], value, c)
        return value
    selected_forward, selected_reverse = choose(used_forward), choose(used_reverse)
    same_sets = [a == b for a, b in zip(used_forward, used_reverse)]
    check("equal_used_color_sets_give_same_first_color", same_sets + [
        selected_forward != selected_reverse], z3.unsat)
    check("equal_full_sets_retain_256_sentinel", same_sets + used_forward + [
        selected_forward != 256], z3.unsat)
    check("equal_sets_last_available_byte_nonvacuity", same_sets + used_forward[:255] + [
        z3.Not(used_forward[255]), selected_forward == 255], z3.sat, [selected_forward])

    # Admission is role-local. The participating premise is never dropped.
    sender, receiver, role, schema, decoded = z3.Ints("sender receiver role schema decoded")
    participates = z3.Or(z3.And(role == sender, role != receiver),
                        z3.And(role == receiver, role != sender),
                        z3.And(role == sender, role == receiver, schema == 0))
    byte = [decoded >= 0, decoded < 256]
    retained_equality = z3.Implies(participates, decoded == 256)
    natural = [sender >= 0, receiver >= 0, role >= 0, schema >= 0]
    check("participating_nonself_role_retains_exhausted_row", natural + [sender != receiver,
          z3.Or(role == sender, role == receiver), z3.Not(participates)], z3.unsat)
    check("retained_invalid_row_rejects_exact_byte_equality", natural + byte + [
          participates, retained_equality], z3.unsat)
    check("unrelated_role_omits_invalid_row_negative_control", natural + byte + [
          role != sender, role != receiver, retained_equality], z3.sat, [sender, receiver, role, decoded])
    check("remove_decoder_byte_bound_negative_control", natural + [participates,
          retained_equality], z3.sat, [sender, receiver, role, schema, decoded])
    check("successful_lowering_retained_byte_nonvacuity", natural + byte + [
          sender != receiver, role == sender, decoded == 255], z3.sat, [decoded])
    all_colors = z3.Array("total_reference_colors", z3.IntSort(), z3.IntSort())
    row_count, bad, k = z3.Ints("row_count overflow_index k")
    success = z3.ForAll([k], z3.Implies(z3.And(k >= 0, k < row_count),
                          z3.And(z3.Select(all_colors, k) >= 0, z3.Select(all_colors, k) < 256)))
    check("successful_lowering_excludes_any_global_overflow", [row_count > 0, bad >= 0,
          bad < row_count, success, z3.Select(all_colors, bad) == 256], z3.unsat)
    check("role_local_empty_projection_does_not_imply_global_success", [row_count == 1,
          z3.Select(all_colors, 0) == 256, role != sender, role != receiver,
          z3.Not(participates), z3.Not(success)], z3.sat, [row_count, role, sender, receiver])


def definition(source, name):
    match = re.search(r"^def " + re.escape(name) + r"(?=\s|\()", source, re.M)
    assert match, name
    rest = source[match.start():]
    following = re.search(r"\n(?:def |structure |theorem |private def |/--)" , rest[4:])
    return rest if following is None else rest[:following.start() + 4]


def check_placement(image, global_syntax, rust):
    program = definition(image, "canonicalProgramSource")
    event = definition(image, "Choreo.canonicalFrameLabel")
    role = definition(image, "Choreo.canonicalRoleFrameLabels")
    control = definition(image, "canonicalControlSource")
    compiled = definition(global_syntax, "Choreo.compiledOccurrences")
    assert program.count("separateElasticFrameDomains ") == 1
    assert "atoms := separateElasticFrameDomains control.markers\n      (compiled.occurrences.map CompiledOccurrence.programAtomBody)" in program
    assert "let control := canonicalControlSource choreo" in program
    assert "let compiled := choreo.compiledOccurrences" in program
    assert "canonicalWireAtoms" not in image
    assert "(canonicalProgramSource choreo).atoms[index]?" in event
    assert "(canonicalProgramSource choreo).atoms.filterMap" in role
    assert all("separateElasticFrame" not in text for text in [event, role, control, compiled])
    assert rust.count("separate_roll_frame_domains(&mut lowering.eff);") == 1
    assert rust.index("lowering.emit(&Steps::SOURCE_NODE") < rust.index("separate_roll_frame_domains(&mut lowering.eff);")
    assert rust.index("type tree and lowered source disagree") < rust.index("separate_roll_frame_domains(&mut lowering.eff);")


def run_source_audit(repo):
    image = (repo / "proofs/lean/Hibana/DescriptorImage.lean").read_text()
    refinement = (repo / "proofs/lean/Hibana/DescriptorRefinement.lean").read_text()
    syntax = (repo / "proofs/lean/Hibana/GlobalSyntax.lean").read_text()
    rust = (repo / "src/g/source.rs").read_text()
    owner = definition(image, "elasticFrameOwner")
    assert "match markers.find?" in owner and "| none => best" in owner
    assert "markers.drop" not in owner and "atomCount" not in owner
    assert "let used := prior.filterMap" in definition(image, "separateElasticFrameDomainsFrom")
    assert "(prior ++ [{ original := atom, owner, color }])" in image
    assert "(List.range 256).find?" in definition(image, "firstElasticFrameColor")
    assert "| none => 256" in definition(image, "firstElasticFrameColor")
    assert "if byte < 256 then some byte else none" in definition(image, "readByte?")
    assert "certificate.image.decodeEventFrameLabels? =\n      some (certificate.choreo.canonicalRoleFrameLabels certificate.image.role)" in refinement
    assert "theorem accepted_descriptor_frame_labels_bind_compiled_coloring" in refinement
    check_placement(image, syntax, rust)
    mutants = {
        "extra_phase_in_per_event_accessor": image.replace(
            "(canonicalProgramSource choreo).atoms[index]?",
            "(separateElasticFrameDomains [] (canonicalProgramSource choreo).atoms)[index]?"),
        "double_phase_in_full_source": image.replace(
            "atoms := separateElasticFrameDomains control.markers\n      (compiled.occurrences.map CompiledOccurrence.programAtomBody)",
            "atoms := separateElasticFrameDomains control.markers\n      (separateElasticFrameDomains control.markers\n      (compiled.occurrences.map CompiledOccurrence.programAtomBody))"),
    }
    controls = []
    for name, mutant in mutants.items():
        assert mutant != image
        try:
            check_placement(mutant, syntax, rust)
        except AssertionError:
            controls.append({"name": name, "rejected": True})
        else:
            raise AssertionError(f"source-placement mutant accepted: {name}")
    return {"external_phase": "exactly once in canonicalProgramSource, after compiledOccurrences and canonicalControlSource",
            "accessors": "both direct observers of final canonicalProgramSource.atoms",
            "rust_phase": "exactly once after full emit and structural count checks",
            "method": "pinned full-source identity plus exact declaration checks; not general call-graph verification",
            "negative_controls": controls}


def run_finite(prior, repo):
    R, M = prior.Row, prior.Marker
    base = R(0, 1, 0, 0)
    fixtures = {
        "ordinary_257_no_roll": ([replace(base, frame_label=i % 256) for i in range(257)], []),
        "separate_rolls": ([base] * 4, [M(i, i + 1, i) for i in range(4)]),
        "nested_and_coextensive": ([base] * 3, [M(1, 2, 2), M(1, 2, 1), M(0, 3, 0)]),
        "reverse_nested": ([base] * 4, [M(1, 3, 1), M(0, 4, 0)]),
        "domains_and_original_labels": ([base, replace(base, frame_label=1), R(2, 1, 0, 0),
            R(0, 2, 0, 0), R(0, 1, 1, 0)], [M(0, 5, 0)]),
        "self_sends_preserve_labels": ([R(0, 0, 0, 255), base, R(0, 0, 0, 19), base],
            [M(0, 2, 0), M(2, 4, 1)]),
        "half_open_boundary_and_nonroll_markers": ([base] * 5, [M(1, 3, 1),
            M(0, 5, 2, "Route"), M(3, 3, 3), M(0, 5, 4, "Roll", False)]),
        "full_256_palette": ([base] * 256, [M(i, i + 1, i) for i in range(256)]),
        "257th_reports_first_failure": ([base] * 257, [M(i, i + 1, i) for i in range(257)]),
        "total_reference_continues_after_invalid": ([base] * 257 + [R(1, 2, 0, 0)],
            [M(i, i + 1, i) for i in range(258)]),
    }
    records = []
    for name, (rows, intervals) in fixtures.items():
        result, failed = compare_valid(prior, rows, intervals)
        records.append({"name": name, "rows": len(rows), "raw_markers": len(raw_markers(intervals)),
                        "first_failure": failed, "colors": [r.frame_label for r in result]})
    variants = [base, replace(base, frame_label=1), replace(base, frame_label=255),
                R(0, 1, 1, 0), R(0, 2, 0, 0), R(2, 1, 0, 0), R(0, 0, 0, 255)]
    exhaustive = 0
    layouts = {}
    for n in range(5):
        nlayouts = {"ordinary": [], "one_roll": [M(0, n, 0)],
                    "coextensive": [M(0, n, 0), M(0, n, 1)]}
        if n >= 2:
            nlayouts.update(nested=[M(0, n, 0), M(1, n, 1)], disjoint=[M(0, 1, 0), M(1, n, 1)])
        if n >= 3:
            nlayouts["nested_coextensive"] = [M(0, n, 0), M(1, n - 1, 1), M(1, n - 1, 2)]
        for name, intervals in nlayouts.items():
            count = 0
            for rows in itertools.product(variants, repeat=n):
                for order in itertools.permutations(intervals):
                    compare_valid(prior, rows, order)
                    count += 1
            layouts[f"{n}:{name}"] = count
            exhaustive += count
    historical_path = repo / "proofs/elastic-roll-colors/roll-membership-capacity-model.json.gz"
    historical = json.loads(gzip.decompress(historical_path.read_bytes()))
    historical_records = []
    for name, graph in historical["graphs"].items():
        rows = [R(*e["key"], e["baseline_color"]) for e in graph["events"]]
        intervals = [M(s["start"], s["end"], s["id"], s["kind"]) for s in graph["scopes"]]
        result, failed = compare_valid(prior, rows, intervals)
        assert failed is None
        assert [r.frame_label for r in result] == [e["proposed_color"] for e in graph["events"]]
        historical_records.append({"name": name, "rows": len(rows), "matched": True})
    # Run both raw-marker transcriptions, retaining actual divergent outputs.
    malformed = {
        "missing_exit": ([base] * 3, [RawMarker(1, 8192, 0)]),
        "duplicate_exit_before_enter": ([base] * 3,
            [RawMarker(0, 8192, 2), RawMarker(0, 8192, 0), RawMarker(3, 8192, 2)]),
        "only_exit_before_enter": ([base] * 3,
            [RawMarker(0, 8192, 2), RawMarker(0, 8192, 0)]),
        "orphan_exit_without_enter": ([replace(base, frame_label=7)] * 2,
            [RawMarker(2, 8192, 2)]),
        "duplicate_later_exits_choose_first": ([base] * 3,
            [RawMarker(0, 8192, 0), RawMarker(1, 8192, 2), RawMarker(3, 8192, 2)]),
    }
    controls = []
    for name, (rows, markers) in malformed.items():
        ext, old = external_owners(rows, markers), former_owners(rows, markers)
        if name in ("missing_exit", "duplicate_exit_before_enter", "only_exit_before_enter"):
            assert not well_formed_matching_exits(markers) and ext != old
        elif name == "orphan_exit_without_enter":
            assert ext == old == (0, 0)  # vacuous enter premise; ordinary no-Roll identity
            assert allocate(rows, markers, ext) == tuple(rows)
        else:
            assert not well_formed_matching_exits(markers) and ext == old == (1, 0, 0)
        controls.append({"name": name, "raw_markers": [m.__dict__ for m in markers],
                         "bridge_premise": well_formed_matching_exits(markers),
                         "external_owners": ext, "former_owners": old,
                         "external_colors": [r.frame_label for r in allocate(rows, markers, ext)],
                         "former_colors": [r.frame_label for r in allocate(rows, markers, old, True)]})
    # Permutations show order matters to the first-exit search on malformed data.
    markers = malformed["duplicate_later_exits_choose_first"][1]
    assert external_owners([base] * 3, [markers[0], markers[2], markers[1]]) == (1, 1, 1)
    filtered = [2, 7, 2, 9]
    assert filtered != list(reversed(filtered)) and set(filtered) == set(reversed(filtered))
    # Direct per-role filter/byte checks on an actual overflow fixture.
    overflow, failed = compare_valid(prior, *fixtures["257th_reports_first_failure"])
    assert failed == 256
    def role_labels(role):
        return [r.frame_label for r in overflow if role in (r.sender, r.receiver)]
    assert 256 in role_labels(0) and 256 in role_labels(1) and role_labels(2) == []
    assert all(0 <= color < 256 for color in role_labels(2))
    assert not all(0 <= color < 256 for color in role_labels(0))
    return {"named_fixtures": records, "exhaustive_cases": exhaustive,
            "exhaustive_layout_counts": layouts, "historical_models": historical_records,
            "raw_marker_controls": controls,
            "list_order_control": {"append_filterMap": filtered, "reverse_filter_map": list(reversed(filtered)),
                                   "lists_differ_sets_equal": True},
            "capacity_scope": {"global_first_failure": failed, "participants_retain_256": [0, 1],
                               "unrelated_role_2_labels": role_labels(2),
                               "positive_correspondence_requires_successful_rust_lowering": True}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--prior", type=Path, required=True)
    parser.add_argument("--prior-replay", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert not args.output.exists(), "Refusing to overwrite proof evidence; choose a fresh output"
    repo, prior_path = args.repo.resolve(), args.prior.resolve()
    source_hashes = {p: sha(repo / p) for p in PINS}
    assert source_hashes == PINS, "External pinned source changed"
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip() == HEAD
    assert sha(prior_path) == PRIOR_CHECKER_SHA
    prior_descriptor = prior_path.parents[2] / "proofs/lean/Hibana/DescriptorImage.lean"
    assert sha(prior_descriptor) == PRIOR_DESCRIPTOR_SHA
    replay = json.loads(args.prior_replay.read_text())
    assert replay["passed"] and replay["repository"] == str(repo) and replay["head"] == HEAD
    assert replay["checker_sha256"] == PRIOR_CHECKER_SHA and len(replay["z3_checks"]) == 40
    assert all(c["expected"] == c["actual"] for c in replay["z3_checks"])
    assert all(sha(repo / p) == value for p, value in replay["source_sha256_before"].items())
    spec = importlib.util.spec_from_file_location("unchanged_prior_wire_gate", prior_path)
    prior = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = prior
    spec.loader.exec_module(prior)
    report = {
        "passed": False, "started_utc": datetime.now(timezone.utc).isoformat(),
        "command": [sys.executable, *sys.argv], "repository": str(repo), "head": HEAD,
        "checker_sha256": sha(__file__), "z3_version": z3.get_version_string(),
        "source_sha256_before": source_hashes, "prior_checker": str(prior_path),
        "prior_checker_sha256": PRIOR_CHECKER_SHA, "prior_descriptor_sha256": PRIOR_DESCRIPTOR_SHA,
        "prior_40_query_replay": str(args.prior_replay.resolve()),
        "prior_40_query_replay_sha256": sha(args.prior_replay),
        "assumptions": [
            "Every Roll enter has one unique matching exit in the full marker list, strictly after the enter; its offset equals Rust segment_end.",
            "Canonical source intervals are valid half-open intervals, laminar with unique source ordinals 0..8191; coextensive inner wrappers have later ordinals.",
            "Input frame labels are bytes, input row order is the final complete structural-lowering order, and prior original labels are frozen.",
            "Positive emitted Rust descriptor correspondence assumes successful Rust lowering (no exhausted row anywhere).",
            "Negative per-role admission rejection requires that the role filter retains an invalid 256 row.",
            "Concrete runtime Covers/SameClassUnique remains an external premise, not established by this gate.",
        ],
        "claim_limits": [
            "This is proof-only Python/Z3 and finite source-transcription evidence, not Lean/Rust execution or universal language refinement.",
            "Unique exits after enters and Rust segment_end correspondence are explicit bridge premises, not proved for all compiler executions here.",
            "Arbitrary malformed raw-marker equivalence is false: external full-list find and absent-close handling differ from the former suffix/fallback helper.",
            "Append/reverse prior list membership agrees; list order itself need not agree.",
            "Full-source placement is checked against exact pinned declarations, not a generic compiler call-graph theorem.",
            "Per-role exact admission does not establish global capacity rejection for arbitrary synthetic certificates.",
            "Fixed greedy-prefix exhaustion does not claim optimal graph coloring.",
        ], "z3_checks": [],
    }
    def check(name, constraints, expected, variables=()):
        solver = z3.Solver()
        solver.set(timeout=30000)
        solver.add(*constraints)
        actual = solver.check()
        record = {"name": name, "expected": str(expected), "actual": str(actual)}
        if actual == z3.sat:
            model = solver.model()
            record["witness"] = {str(v) if len(str(v)) < 120 else f"expression_{i}":
                                 str(model.eval(v, model_completion=True)) for i, v in enumerate(variables)}
        if actual == z3.unknown:
            record["reason"] = solver.reason_unknown()
        report["z3_checks"].append(record)
        print(f"{name}: {actual} (expected {expected})", flush=True)
        assert actual == expected, record
    try:
        report["source_audit"] = run_source_audit(repo)
        run_z3(check)
        report["finite_equivalence"] = run_finite(prior, repo)
        report["source_sha256_after"] = {p: sha(repo / p) for p in PINS}
        assert report["source_sha256_after"] == source_hashes
        assert sha(prior_path) == PRIOR_CHECKER_SHA and sha(prior_descriptor) == PRIOR_DESCRIPTOR_SHA
        report["passed"] = True
    except BaseException as error:
        report["error"] = repr(error)
        raise
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    finite = report["finite_equivalence"]
    print(f"PASS {len(report['z3_checks'])} external-specific Z3 obligations/controls; "
          f"{finite['exhaustive_cases']} bounded cases; {len(finite['named_fixtures'])} fixtures; "
          f"{len(finite['historical_models'])} historical models; "
          f"{len(finite['raw_marker_controls'])} raw-marker controls", flush=True)
    print("LIMIT: explicit well-formed-marker and successful-lowering premises; role-local capacity admission", flush=True)


if __name__ == "__main__":
    main()
