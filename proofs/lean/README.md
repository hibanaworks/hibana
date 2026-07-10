# Hibana Lean proofs

This host-only package checks a normalized Hibana choreography model with Lean
Core and Std. It is outside the Cargo workspace and does not enter Pico builds,
runtime memory, or flash artifacts.

The kernel-checked boundary covers:

- role projection for `send`, `seq`, `par`, `route`, and `roll`;
- send/receive duality, self-send locality, uninvolved-role erasure, and label
  preservation for the normalized send projection;
- structural validity of projected events, roll scopes, and route scopes;
- rejection of trace commits that are absent from the enabled frontier;
- exact dynamic `resolve` authority by route site and resolver id, including
  explicit left/right selection and terminal rejection;
- visibility of unresolved dynamic-route candidates without commit authority;
- single-use resolver authority until an enclosing `roll` reset;
- fail-closed rejection of direct unresolved commits, wrong resolver ids,
  resolver reuse without reset, and any continuation after resolver rejection;
- unique route-arm authority in every commit state;
- exact commit and resolver successor deltas from one prepared base state;
- atomic roll and route-arm reentry resets, including inside-clear and
  outside-preservation properties.

The gate exports real production-cursor frontiers from Rust and checks them as
concrete Lean proofs. The generated corpus contains 13 artifacts and 55 frames.
It exercises all roles of the base choreography, both intrinsic route arms,
send/receive projections, nested and repeated roll restart, resolved left/right
arms, nested resolver sites, alternating resolved roll reentry, and resolver
rejection.

This is not a source-to-source proof of arbitrary Rust. The normalized model
separates the production cursor's candidate frontier from commit authority, so
a dynamic route can expose candidate labels while remaining uncommittable until
its resolver transition succeeds. Aeneas, Verus, Mathlib, custom axioms,
`Classical.choice`, `sorry`, and `admit` are not part of this boundary. The
axiom audit permits only the `propext` and `Quot.sound` dependencies introduced
by the checked Core/Std proofs.

Run the same fail-closed gate used by CI:

```sh
bash .github/scripts/check_lean_proofs.sh
```
