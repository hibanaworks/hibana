#!/usr/bin/env bash

# Normalize local and CI test execution so failures reproduce before push.
#
# trybuild stderr is part of the public surface guard.  Use rustc's default
# diagnostic shape instead of inheriting caller-specific terminal widths.
unset CARGO_TERM_WIDTH

# Rust's libtest runs tests on worker threads.  Keep the worker stack explicit
# so local final-form gates exercise the same budget as CI instead of passing
# only because a host happens to provide a larger default.
export RUST_MIN_STACK="${RUST_MIN_STACK:-2097152}"
