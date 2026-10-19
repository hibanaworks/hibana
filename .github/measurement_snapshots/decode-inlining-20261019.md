# Current decoder measurements

The Linux x86-64 Callgrind profiles for f8c32d44 attribute about 53% of
exclusive instructions to RoleImageRef::local_step_node and
PackedLocalEventRow::to_node. Add inline hints to those same functions so the
compiler can optimize their immutable metadata at callers. Their bodies,
validation, actor admission, image format and runtime fields remain unchanged.
This is the same kernel, without a legacy execution path or a feature switch.

Rust 1.99.1 on aarch64-apple-darwin measured these best/median ns per endpoint
operation. Each case has 128 timed operations, three warmups and 30 samples;
initialization and tail draining are excluded.

| Case | Tail | Before | With inline hints |
| --- | ---: | ---: | ---: |
| seq | 8 | 9651.7 / 10422.2 | 9065.1 / 10125.3 |
| par | 8 | 4625.6 / 4933.9 | 4430.7 / 4828.8 |
| seq | 256 | 14308.9 / 14541.3 | 12604.5 / 13276.7 |
| par | 256 | 7728.2 / 7927.4 | 6775.4 / 7309.6 |

These wall-clock samples motivate retaining the hints, but are not proof of
historical-main parity. Linux instruction counts use the separate measured
regression gate. No historical implementation or old compiler was rerun.

The first current thumbv6m release measurement was .text 104188, .rodata 15453,
.data 0 and .bss 0: total 119641 bytes. The old per-section .rodata allowance
15341 rejected it even though total flash was within 169965. Move 112 bytes of
allowance from .text to .rodata: new maxima 154512 and 15453. Their sum remains
169965. Do not infer the entire section difference was caused solely by the
hints; this is a measurement of the current source and compiler.

There is no increase to total flash, slab, resident storage, modeled SRAM or
the provisional 5120-byte stack budget. The complete measurement gate must
still check the final build; the first rejected log is retained separately.

CI 39217605385 also passed the Linux instruction gate. The larger fresh total
per case was 134967179 (seq/8), 249383269 (seq/256), 77593284 (par/8), and
136723836 (par/256), over 512 endpoint operations each. These are 3.3–4.0%
below the pre-inlining totals at the same compiler and target. The two fresh
runs differ by less than 0.001%. Tighten each instruction limit to its new
maximum plus 1%, rounded up, so the previous decoder's cost would fail it.

The next measurement exposed a stale collector limit: the actual host
SendFuture is 48 bytes, and both existing library size tests already permit six
words for the owned/scoped operation target. The collector still allowed five
words. Make its bound agree with those tests and reject one byte above six
words in the parser's boundary test. This does not change a future field or
increase a runtime storage/stack budget. All other future bounds are unchanged.
