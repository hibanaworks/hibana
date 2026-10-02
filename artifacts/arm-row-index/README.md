# Certified metadata performance qualification

These are sanitized qualification outputs for the exact three-stage immutable metadata optimization, based on upstream 3aef31ba015c75ea824b8b41f5603b03f5dd336b. Final changed production bytes match the separately qualified stage3 manifest. The Lean/Z3 gates were executed before each implementation stage; see the three proofs directories for models, source assumptions and original logs.

- Full workspace: 834 passed, 0 failed, 11 existing ignored
- Existing Lean gate and source authority/layout gates passed
- No Miri interpreter or Kani run is claimed
- No additional metadata SRAM, heap use or mutable authority state
- Separate route/liveness correctness defects are not fixed by this branch

## Observed transfer performance

The unchanged 90,112-byte plus empty-file actual-owner self-UDP fixture completed with exact payload, both endpoints Closed, and 279/280 datagrams in every compared version. User CPU was 58.824337 s at baseline, 55.444280 s after stage 1, 9.427799 s after stage 2, and 5.106626 s after stage 3. Stage 3 client body completion was 2.344581 s and Closed 3.987537 s. These are individual controlled measurements, not a throughput guarantee or formal Neqo interoperability result. The complete QUIC actor replacement remains unfinished.

`real-owner-summary.json` contains numeric counters and the frozen executable hash, with no packet payloads or traffic secrets. Actor-span timers overlap individual operation timers and must not be added together.

## Reproducing the public API microbenchmark

`bench-source.rs` is the unchanged measured harness (its original hash is in benchmark-inputs.sha256). It uses four publicly available source files from hibanaworks/hibana-quic commit 004a32459f8013c2be477cc5c876da03589ce059. Check their hashes against that manifest. Place the harness as `projected-api-microbench/bench.rs` alongside the `hibana-quic` checkout, as its relative source imports require. Build the selected Hibana checkout in release mode with Rust 1.95.0 and no custom RUSTFLAGS; compile the harness with rustc edition 2024, opt-level 3, codegen-units 1, panic=abort and --extern hibana pointing at that release library. Run 100 warmup exchanges and three 300-exchange runs using argument `all`, serially for each revision. The recorded benchmark contains no crypto or UDP.

The full runtime/Thumb footprint output is a core/library measurement. It does not establish a complete embedded QUIC/TLS application fit.
