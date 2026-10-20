# Initial issuer attachment measurement

Resource issuance now requires its source-assigned actor to be attached in
the same runtime and session before reserving the physical binding. A missing
actor returns an attach error with the resource and expected actor. Callback
reentry cannot replace that actor with a resource-only allocation. There are
no new runtime fields, wire bytes, allocator dependencies or operation rules.

The current source with this check, Rust 1.99.1, thumbv6m-none-eabi, release
and no default features produces complete `libhibana.rlib` totals of .text
109916, .rodata 16674, .data 0 and .bss 0: total flash 126590 bytes.
The first check rejected .rodata against 16116. Transfer the 558-byte
difference from text allowance, setting maxima 153291 and 16674. Their sum
and the total flash limit remain 169965. Stack, slab, resident storage, SRAM
and instruction limits do not change. Preserve the rejected measurement.

The previous full measurement of `44c902cf` passed the artifact, layout,
stack, SRAM and compile-pressure gates; its maximum measured runtime stack
was 2879 bytes against 5120. That older result is scoped to its own source.
The new source still requires the complete feature CI. Neither this cold
attachment change nor the section allocation proves a hot-path speedup.
