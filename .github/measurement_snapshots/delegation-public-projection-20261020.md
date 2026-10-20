# Public Endpoint projection flash measurement

The normal public projection path is enabled on the development branch. The
first full measurement of `29d54a003017342d5bc0f4b1c50f243b6af01ca5`
with Rust 1.99.1, release, thumbv6m-none-eabi and no default features produced
these complete `libhibana.rlib` section totals:

| Section | Bytes |
| --- | ---: |
| .text | 109612 |
| .rodata | 16116 |
| .data | 0 |
| .bss | 0 |
| Total flash | 125728 |

The old `.rodata` allowance of 15953 rejects this result by 163 bytes. Transfer
exactly 163 bytes of allowance from `.text` to `.rodata`, making their maxima
153849 and 16116. Both sections use the same flash; their sum remains 169965.
No total flash, stack, slab, resident storage, SRAM or instruction limit is
increased. The initial rejected measurement is retained in the validation
record. This section allocation is not a claim about runtime performance or
an attribution of every byte change to the projection guard removal.

The rest of the measurement gate must still check the protocol artifacts,
future layouts, runtime stack, SRAM, compile pressure and message-heavy cases.
Passing this section check alone does not complete Endpoint qualification.
