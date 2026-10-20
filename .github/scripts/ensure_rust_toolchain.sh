#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="${TOOLCHAIN:-stable}"

if ! rustup which --toolchain "${TOOLCHAIN}" cargo >/dev/null 2>&1; then
  rustup toolchain install "${TOOLCHAIN}"
fi

for target in "$@"; do
  rustup target add --toolchain "${TOOLCHAIN}" "${target}"
done
