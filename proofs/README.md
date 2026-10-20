# Verification

Hibana uses Lean models, SMT checks, bounded Rust verification, and executable
tests to check protocol projection and execution. The models state assumptions
about codecs, transports and participant behavior. They do not verify arbitrary
Rust programs or arbitrary implementations of `Transport`.

## Layout

| Directory | Contents |
| --- | --- |
| [lean](lean/README.md) | Protocol, ownership, runtime and transport models in one Lean package |
| [lean/claims](lean/claims) | The theorem inventories checked by the Lean gate |
| [checks/projection](checks/README.md#projection) | Source ownership, branch knowledge, dependencies and frame allocation |
| [checks/image](checks/README.md#protocol-images) | Packed descriptor fields, lookup and bounds |
| [checks/runtime](checks/README.md#local-execution) | Operation admission, choice, parallel work and repetition |
| [kani](kani/README.md) | Reviewed inventory and entry point for production Kani harnesses |
| [carriers/unix](carriers/unix/README.md) | A concrete Unix datagram transport and conformance tests |

The [check index](checks/README.md) links each group to the implementation it
models. Shared definitions live in `lean/Hibana`. Individual checks import
those definitions rather than keeping copies. Test fixtures are inputs to
reproducible checks, not reports of a previous successful run.

## What is established

The protocol models prove session fidelity, affine ownership and progress
under the assumptions in their theorem statements. The occurrence model
handles finite unfoldings with choices, parallel work, independent repetition
and Endpoint transfers. Its ownership and progress results use the same
source, carrier and custody history.

Transport assumptions include peer and direction binding, FIFO delivery,
nonduplication, replay exclusion, separation of session runs and receive access
after an Endpoint changes holder. A deadline or I/O error does not prove that a
frame was undelivered, and cannot restore a transferred Endpoint.

Rust/model conformance tests compare permitted and rejected operations over
bounded schedules, including separate transport acceptance and sender
completion, close and abort. Mutation checks establish that the comparisons
can detect incorrect ownership, unwanted serialization and lost dependencies.
This is executable correspondence testing, not a complete Rust refinement proof.

Kani checks bounded properties of production functions. Miri checks aliasing,
pointer provenance and destruction in executed tests. Native tests exercise
actual Endpoints, codecs, borrowed buffers and separate-process communication.
The Unix and TCP fixtures do not establish authentication or forwarding
correctness for other deployments.

## Run the checks

Use the latest stable Rust for ordinary builds. Lean is pinned by
[`lean-toolchain`](lean/lean-toolchain). Miri and Kani use the compilers required
by the versions pinned in `.github/`.

From the repository root:

```sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash .github/scripts/check_lean_proofs.sh
bash .github/scripts/check_kani.sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash .github/scripts/check_miri.sh
```

Run these commands sequentially. The guard limits the whole child process tree
to 3,000,000,000 bytes. The Kani entry point already invokes it.

The [CI workflow](../.github/workflows/quality-gates.yml) also runs the individual
checks, transport tests, embedded builds, size measurements and instruction
budgets. Generated Lean inputs and logs go to build or temporary directories.
The checked theorem inventories reject missing or extra claims and forbidden
axioms. Model theorems and kernel-only generated claims permit only `propext`
and `Quot.sound`. Finite regression examples and explicitly classified
acceptance certificates use `native_decide`; they also trust Lean's compiler
and runtime. The compiled schedule oracle is an executable test. The
[Lean audit description](lean/README.md#run-and-audit) identifies these boundaries.
