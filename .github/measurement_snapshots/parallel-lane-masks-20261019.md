# Immutable parallel lane masks

Projection appends one compressed mask for each participating logical view
and lexical parallel arm to the immutable lane-data column. A row has six
header bytes and one to 32 bitmap bytes. The ordinary non-parallel image has
no mask rows. The constructor independently reconstructs every row from source
markers and local operations; omitting the table does not bypass validation.

A parallel borrow holds a two-byte offset to that data. Its source region is
eight bytes instead of six. This offset also survives deferred Drop; it does
not borrow storage inside a child that may already have been destroyed.
There is no mutable lane cache, owner registry, lock, allocation or new wire
field. Lane membership reads a bounded byte and bit instead of scanning all
local events. Other runtime scans remain separate work.

The actual host sizes remain Endpoint 16, ParallelEndpoint 24 and operation
handle 16 bytes. These sizes do not establish that every resident allocation
is unchanged; the complete measurement gate checks SRAM and stack separately.

Rust 1.99.1, thumbv6m-none-eabi, no default features, release produced .text
106588, .rodata 15953, .data 0 and .bss 0: total 122541 bytes. The first
measurement failed the old .rodata section limit of 15453. Transfer 500 bytes
of allowance from .text to .rodata, making their maxima 154012 and 15953.
The total flash limit remains 169965. No slab, resident, SRAM or stack budget
is raised. The rejected measurement is retained as validation evidence.

Host timings use the same current kernel before and after this change, 128
timed operations per sample, three warmups and five samples. Initialization,
borrow construction and tail draining are excluded. The results vary too
much to establish an overall speedup. Linux Callgrind counts and the existing
instruction limits remain the performance gate; their limits are not raised.
