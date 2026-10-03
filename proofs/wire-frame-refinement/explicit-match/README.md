# Explicit option-match followup

Commit `7762836741fc6d6c73f03c63980bc56e5c67e0fa` passed the Lean proof gate but
the semantic-surface gate rejected two `.getD` calls in production Lean source.
The existing guard and its test were preserved unchanged.

Before editing production definitions, `OptionMatchEquivalence.lean` proved
four axiom-free equalities, including both replacements under arbitrary
observing contexts. `check_option_match.py` executed eight Z3 obligations and
non-vacuity controls. `pre-edit-gate.json` records the exact two source rewrites,
before/after hashes, proof hashes, commands, and passing logs.

The replacements use exhaustive option matches. The absent owner remains 0;
failed color selection remains the invalid value 256. The source-bridge runner
applies only these two recorded rewrites to the immutable original candidate
before comparing it with current production definitions. The previous manifest
and runner are archived here; original `pre-edit/` and `validation/` files are
unchanged.
