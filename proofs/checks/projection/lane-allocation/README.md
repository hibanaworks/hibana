# Lane allocation

The source compiler assigns lanes before publishing local programs. Parallel
operations with overlapping possible actors keep distinct lanes. Receives from
different logical senders share a lane only when the source establishes their
causal order, including repetition and changes of Endpoint holder.

`allocation/receive_lanes.rs` first finds a valid move for one conflicting
event, then applies that color to other compatible occurrences of the same
logical direction and old lane. Every parallel scope and each newly adjacent
receive direction is checked. Temporary candidate membership and direction
lookup storage exist only during compilation.

`GlobalAllocationCorrectness.lean` in the common Lean package proves remapping
for an arbitrary selected set: injectivity inside the set and exclusion of
incompatible outside colors prevent new conflicts. `RouteAlignment.smt2`
checks those conditions and the one-old-lane batch, with failing-rule controls.
Native tests compare nested parallel exclusions with source intervals. Kani
checks symbolic endpoint identities and workspace reuse; Miri executes the
candidate and borrowed-workspace tests. These checks do not constitute a full
Rust refinement proof.

Run `bash .github/scripts/check_projection_algorithms.sh` for the supplemental
models and `bash .github/scripts/check_lean_proofs.sh` for the common Lean package.
