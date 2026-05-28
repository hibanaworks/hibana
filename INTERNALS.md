# Hibana Internals

This file is maintainer-facing. Application authors should use `hibana::g` and
`Endpoint`; protocol implementors should use `hibana::integration`.

## Attach

Attach does not lower a projected role. It reads the resident role image owned
by projection and initializes only endpoint/session state. A role with no
resident descriptor is not attachable.

The resident compiled image is the source of truth. Attach must not rebuild the
role descriptor or program descriptor through an alternate materialization path,
and must not reserve lowering scratch. Immutable queries against the resident
program image are descriptor reads; they are not attach lowering and must not
allocate, clone, or reserve scratch. If stable Rust cannot express a particular
exact-sized static layout, Hibana changes the resident image representation; it
does not keep attach-time lowering logic.

## Frontier

Runtime frontier entries are compact headers. They may remember live lane,
scope, frontier, summary, and selection bits, but they must not cache frame-label
metadata, arm-materialization tables, route dispatch rows, or observed-state
summaries from the descriptor. Those facts are read from the resident descriptor
or recomputed from live evidence at the wait site.
