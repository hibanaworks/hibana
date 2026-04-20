#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh" thumbv6m-none-eabi

cargo +"${TOOLCHAIN}" check --all-targets -p hibana
cargo +"${TOOLCHAIN}" check --no-default-features --lib -p hibana
# The latest stable lane owns trybuild diagnostics. Keep 1.95 focused on
# build/runtime compatibility and invariant suites that are not compiler-text-sensitive.
cargo +"${TOOLCHAIN}" test -p hibana --lib --features std
cargo +"${TOOLCHAIN}" test -p hibana --test policy_replay --features std
cargo +"${TOOLCHAIN}" test -p hibana --test public_surface_guards --features std
cargo +"${TOOLCHAIN}" test -p hibana --test substrate_surface --features std
cargo +"${TOOLCHAIN}" test -p hibana --test docs_surface --features std
cargo +"${TOOLCHAIN}" check --no-default-features --target thumbv6m-none-eabi -p hibana

echo "stable ${TOOLCHAIN} gate passed"
