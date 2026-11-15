# Lane allocation

The source compiler assigns lanes before publishing local programs. Parallel
operations with overlapping possible actors keep distinct lanes. Receives from
different logical senders share a lane only when the source establishes their
causal order, including repetition and changes of Endpoint holder.

`allocation/receive_lanes.rs` first finds a valid move for one conflicting
event, then applies that color to other compatible occurrences of the same
logical direction and old lane. For each candidate, the existing source scope
index supplies every enclosing parallel; unrelated scopes are never rescanned.
Each newly adjacent receive direction is still checked. Temporary candidate membership and direction
lookup storage exist only during compilation. A receiver/color chain index is
built once at each immutable assignment cut and shared by its matching trials.
Its keys, heads and links use the source arena capacity; it adds no runtime
state. Successful remapping ends those queries before the next index is built.
Native tests cover every 16-bit wire key, source order, local-step exclusion and
a completely occupied colliding table, including absent lookups. Matching walks
the existing adjacency bits in ascending order rather than testing absent edges.
Homogeneous receiver/lane groups skip predecessor searches only when their
existing direction index proves that every receive has the same logical sender.
Colored participant trees fold matching leaves into ancestors, leaving absent
leaves empty. Native tests compare these facts and every tested interval with
independent full source scans, including reuse of the same scratch workspace.

Matching vertices include only lane classes present in the selected range.
Each successful cross-boundary check covers every occurrence of that class;
its result is reused while other matching edges are refined. Source lanes
remain unchanged until the whole map has passed. Both compatibility tables
are bounded by the 256-value wire lane domain and are compiler scratch.
Parallel participant-set intersections are materialized once as adjacency bits
before matching. Alternating paths mask visited colors while scanning those
bits, preserving ascending matching order without repeating set intersections.
Native tests compare participant-derived and explicit adjacency on all 512
three-by-three graphs and check masked neighbors at the first and last bytes
against independent enumeration. The original 516-event parallel-capacity
test remains subject to `forbid(long_running_const_eval)`.

After ordinary receive coloring, repeated passive choices need either an
actual same-direction FIFO fence or a causal handoff to the next sender.
Compiler scratch may merge compatible direction classes, but every candidate
retains parallel separation and is rechecked against both obligations before
publication. Failed proposals are restored and accepted partial repairs must
strictly advance the first source conflict. No new runtime state is added.

`GlobalAllocationCorrectness.lean` in the common Lean package proves remapping
for an arbitrary selected set: injectivity inside the set and exclusion of
incompatible outside colors prevent new conflicts. The shared `Fields.lean`
proves exact four-word role intersections and non-aliasing packed row addresses,
including a negative control for a truncated partial byte.
The same Lean module shows that compact vertices preserve class injectivity,
and that whole-class confirmations preserve every cross-boundary obligation.
The same module proves that a complete ancestor index preserves exactly the
all-scopes exclusion predicate. Index completeness remains an implementation
obligation, not an assumed Rust refinement theorem. Native tests compare 128
nested scope combinations, all eight source positions and three proposed colors
against the independent all-interval specification, in addition to delegated
physical-participant and source-index tests. Kani
checks symbolic endpoint identities and workspace reuse; Miri executes the
candidate and borrowed-workspace tests. These checks do not constitute a full
Rust refinement proof.

Run `bash .github/scripts/check_implementation_proofs.sh` for the supplemental
models and `bash .github/scripts/check_lean_proofs.sh` for the common Lean package.
