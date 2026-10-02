"""SMT evidence for the observed same-lane stale-cursor fallback, Hibana 102fc47.

The candidate policy models the production correction and was checked before
that correction was applied. This script is supplemental model evidence.
The actual Rust trace, diagnostic fields, and existing Lean semantic trace proof
are separate evidence; this script does not claim Rust source refinement.
"""
import z3

current, current_lane, observed_lane, head, frame = z3.Ints(
    "current current_lane observed_lane pending_head frame"
)
done = z3.Bool("current_done")
# -1 encodes Option::None; descriptor indices are nonnegative.
old_index = z3.If(current_lane == observed_lane, current, head)
candidate_index = z3.If(z3.And(current_lane == observed_lane, z3.Not(done)), current, head)


def enclosing(index):
    return z3.If(z3.And(observed_lane == 0, index >= 1, index < 4, frame >= 0, frame <= 1), 0,
                 z3.If(z3.And(observed_lane == 1, index >= 5, index < 8, frame >= 0, frame <= 1), 1, -1))


old_scope = enclosing(old_index)
candidate_scope = enclosing(candidate_index)
base = [current >= 0, current_lane >= -1, observed_lane >= 0, head >= -1, frame >= 0]
witness = [current == 4, current_lane == 1, done, observed_lane == 1, head == 5]


def prove(name, premises, negated_property):
    solver = z3.Solver()
    solver.set(timeout=30_000)
    solver.add(*base, *premises)
    assert solver.check() == z3.sat, f"{name}: vacuous premises"
    solver.add(negated_property)
    result = solver.check()
    print(f"{name}: {result} (negated property; premises sat)")
    assert result == z3.unsat, f"{name}: {result}"


for label in [0, 1]:
    solver = z3.Solver()
    solver.add(*base, *witness, frame == label, old_scope == -1, enclosing(head) == 1)
    assert solver.check() == z3.sat
    print(f"existing_guard_counterexample_frame_{label}: sat, current=4(done), lane=1, head=5, old_scope=None, pending_scope=1")
    prove(f"exact_existing_rejection_frame_{label}", witness + [frame == label], old_scope != -1)
    prove(f"candidate_acceptance_frame_{label}", witness + [frame == label], candidate_scope != 1)

prove("completed_cursor_uses_pending_head", [done], candidate_index != head)
prove("live_cursor_behavior_unchanged", [z3.Not(done)], candidate_index != old_index)
prove("foreign_cursor_behavior_unchanged", [current_lane != observed_lane], candidate_index != old_index)
prove("valid_pending_scope_not_hidden", [done, enclosing(head) >= 0], candidate_scope != enclosing(head))
prove("no_pending_scope_is_not_invented", [done, enclosing(head) == -1], candidate_scope != -1)
print("PASS: 9 non-vacuous UNSAT checks and 2 concrete SAT bug witnesses")
