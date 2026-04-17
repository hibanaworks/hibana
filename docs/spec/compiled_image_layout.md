# Compiled Image Layout

Compiled images are blob-backed.

- `CompiledProgramImage` is immutable header plus immutable blob
- `CompiledRoleImage` is immutable header plus immutable blob
- repeated route/frontier/lane shapes are interned

Layout rules:

- persistent bytes are measured separately from runtime transient bytes
- headers carry offsets and resident facts, not pointer-rich section tables
- 32-bit embedded targets are first-class
