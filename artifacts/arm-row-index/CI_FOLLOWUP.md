# CI follow-up

Run37037710684 at6679d6ab failed. Its Kani job stopped at the inherited expected-inventory ordering mismatch before proof execution. Its final-form Rust suites passed451+123+16 tests, then boundary hygiene rejected two new nested-test projection spellings and one test name.

The test-only correction uses an explicitly typed RoleProgram<1> local to infer the same project role, and renames the malformed-prefix test. It changes no assertion, fixture topology, production source, validator or gate. Focused execution with the repository's --cfg hibana_repo_tests ran4 arm-row and6 passive-parent tests successfully; the complete boundary-contract/surface-hygiene gate passed. An initial invocation without that cfg did not cover the added tests and is not counted.

The original pre-CI source manifest is retained as source-before-ci-style.sha256; source.sha256 now includes the two verified test-only style updates. All production bytes are unchanged from the proof-qualified stage3.

The separate inventory correction preserves all200 harness names, multiplicities, groups and every other JSON section. Only one group's sequence is restored to the order emitted by the unchanged Kani generator in the failed run. The exact-equality gate and all harness source/assertions stay unchanged. No local Kani rerun is claimed; the next unchanged GitHub gate must verify this metadata and execute the proofs.
