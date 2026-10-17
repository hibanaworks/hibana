# Provisional runtime stack budget

The runtime stack budget is **5 KiB (5,120 bytes)** for each measured workload,
including attachment. The user set this provisional budget so that Endpoint
delegation can be completed before further stack tuning. It does not establish
that the implementation is fully optimized or qualified on Pico hardware.

The SRAM budgets use the same stack allowance. Each is the sum of its unchanged
session storage, resident prefix and peak live slab budgets, plus 5,120 bytes of
stack and the existing 64-byte headroom. The largest modeled SRAM budget is
10,491 bytes. Slab, resident storage and flash limits are unchanged. The gates
still reject measurements over their budgets and check aggregate headroom.

Core remains `no_std`, without heap allocation or another progress store.
Remove clear waste, but do not change ownership, ordering or concurrency to
match an old measurement. Do not rerun old implementations as a prerequisite
for completing delegation. Final target qualification remains required.

## Current measurements

Release measurements use stable `rustc 1.99.1 (58cf8f9c3 2026-10-13)` on
`aarch64-apple-darwin`. The snapshot's `toolchain` field records its original
budget provenance, not the compiler used here.

| Workload | Localside stack | Peak including attachment |
| --- | ---: | ---: |
| fanout | 2,783 | 2,783 |
| linear | 2,671 | 2,767 |
| route | 2,863 | 2,863 |

The current cleanup avoids constructing complete send metadata when admission
only needs the endpoint payload role, and checks the optional moved view
directly. Validation and callback order remain unchanged. This removes 48 bytes
from the linear localside measurement. It is a local cleanup, not a claim that
all further optimization is exhausted.

The measurement command is:

```sh
source .github/scripts/repo_rustflags.sh
hibana_enable_repo_tests_cfg
cargo +stable test --locked --release --lib -p hibana \
  runtime::tests::large_choreography_runtime_peak_metrics \
  -- --ignored --nocapture --test-threads=1
```

Host measurements depend on ABI and compiler and do not prove deployed Pico
stack use. The separate verification-process limit remains 4 GB, with the
descendant-process guard stopping at 3,000,000,000 bytes. The full local
validation of this runtime cleanup peaked at 1,651,785,728 bytes.
