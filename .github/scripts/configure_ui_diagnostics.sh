#!/usr/bin/env bash

# trybuild stderr is part of the public surface guard.  Use rustc's default
# diagnostic shape instead of inheriting caller-specific terminal widths.
unset CARGO_TERM_WIDTH
