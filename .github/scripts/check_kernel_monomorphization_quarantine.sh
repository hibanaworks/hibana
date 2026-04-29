#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

bash ./.github/scripts/check_message_monomorphization_hygiene.sh

python3 - <<'PY'
import pathlib
import re
import sys

root = pathlib.Path.cwd()


def read(path: str) -> str:
    return (root / path).read_text()


def fail(message: str) -> None:
    print(f"kernel monomorphization quarantine violation: {message}", file=sys.stderr)
    sys.exit(1)


core = read("src/endpoint/kernel/core.rs")
endpoint = read("src/endpoint.rs")
flow = read("src/endpoint/flow.rs")

for required in [
    "pub(crate) struct MsgRuntimeCore",
    "pub(crate) struct SendRuntimeDesc",
    "pub(crate) struct RecvRuntimeDesc",
    "pub(crate) struct DecodeRuntimeDesc",
    "pub(crate) trait RecvKernelEndpoint",
    "pub(crate) trait DecodeKernelEndpoint",
    "pub(crate) trait SendKernelEndpoint",
    "pub(crate) fn kernel_recv",
    "pub(crate) fn kernel_decode",
    "pub(crate) fn kernel_send",
]:
    if required not in core:
        fail(f"missing non-generic kernel owner: {required}")

for name, trait_name in [
    ("kernel_recv", "RecvKernelEndpoint"),
    ("kernel_decode", "DecodeKernelEndpoint"),
    ("kernel_send", "SendKernelEndpoint"),
]:
    if f"#[inline(never)]\npub(crate) fn {name}" not in core:
        fail(f"{name} must remain an out-of-line shared runtime kernel symbol")
    if not re.search(rf"pub\(crate\)\s+fn\s+{name}\s*<\s*'r\s*>\s*\(\s*endpoint:\s*&mut dyn {trait_name}<'r>", core):
        fail(f"{name} must be a transport/substrate-erased kernel entry point")

for required in [
    "fn prepare_recv_kernel_descriptor(",
    "fn poll_recv_kernel_payload_source(",
    "fn finish_recv_kernel_payload(",
    "fn prepare_decode_kernel_transport_wait(",
    "fn poll_decode_kernel_transport_payload(",
    "fn finish_decode_kernel(",
    "fn poll_send_init_kernel(",
    "fn poll_send_pending_kernel(",
    "fn finish_send_after_transport_kernel(",
]:
    if required not in core:
        fail(f"runtime kernels must own state-machine loops and call endpoint primitive ops: {required}")

if "endpoint.poll_recv_kernel(" in core or "endpoint.poll_decode_kernel(" in core or "endpoint.poll_send_kernel(" in core:
    fail("kernel entry points must not delegate whole poll loops back to generic endpoint impls")

poll_send_state = re.search(r"pub\(crate\)\s+fn\s+poll_send_state[\s\S]*?\n    \}\n\}", core)
if poll_send_state and "kernel_send(self, state, cx)" not in poll_send_state.group(0):
    fail("generic poll_send_state must delegate to non-generic kernel_send")

recv = read("src/endpoint/kernel/recv.rs")
decode = read("src/endpoint/kernel/decode.rs")
poll_recv_state = re.search(r"pub\(crate\)\s+fn\s+poll_recv_state[\s\S]*?\n    \}\n\n    fn prepare_recv_descriptor", recv)
if not poll_recv_state or not all(
    required in poll_recv_state.group(0)
    for required in [
        "super::core::kernel_recv(",
        "logical_label",
        "expects_control",
        "accepts_empty_payload",
        "state",
        "cx",
    ]
):
    fail("generic poll_recv_state must delegate to non-generic kernel_recv with control-kind evidence")
if "super::core::kernel_decode(self, desc, state, cx)" not in decode:
    fail("generic poll_decode_state must delegate to non-generic kernel_decode")

if re.search(r"\b(struct|pub\\(crate\\) struct)\s+(RecvDesc|DecodeDesc|SendDesc)\b", core + read("src/endpoint/kernel/recv.rs") + read("src/endpoint/kernel/decode.rs")):
    fail("old per-operation message descriptor names must not return")

if "MsgRuntimeDesc" in core + endpoint + flow:
    fail("MsgRuntimeDesc hid operation-specific descriptor meaning")

if re.search(r"send_descriptor_.*unused", flow):
    fail("send runtime descriptors must not carry dummy validate/synthetic callbacks")

for path, source in [
    ("src/endpoint.rs", endpoint),
    ("src/endpoint/flow.rs", flow),
]:
    if "RuntimeDesc" not in source:
        fail(f"{path} must build message-erased runtime descriptors")

if "send_flow_and_runtime_descriptor_size_gates_hold" not in endpoint:
    fail("send flow and runtime descriptor size gates must guard the descriptor split")

print("kernel monomorphization quarantine check passed")
PY

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
rustup component add llvm-tools-preview --toolchain "${TOOLCHAIN}" >/dev/null

SYSROOT="$(rustup run "${TOOLCHAIN}" rustc --print sysroot)"
HOST="$(rustup run "${TOOLCHAIN}" rustc -vV | sed -n 's|host: ||p')"
LLVM_NM="${SYSROOT}/lib/rustlib/${HOST}/bin/llvm-nm"
if [[ ! -x "${LLVM_NM}" ]]; then
  echo "kernel monomorphization quarantine violation: llvm-nm unavailable for ${TOOLCHAIN}" >&2
  exit 1
fi

SYMBOL_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-kernel-target.XXXXXX")"
SYMBOLS_FILE="$(mktemp "${TMPDIR:-/tmp}/hibana-kernel-symbols.XXXXXX")"
cleanup_symbol_proof() {
  rm -f "${SYMBOLS_FILE}"
  rm -rf "${SYMBOL_TARGET_DIR}"
}
trap cleanup_symbol_proof EXIT

CARGO_TARGET_DIR="${SYMBOL_TARGET_DIR}" cargo +"${TOOLCHAIN}" build -p hibana --features std --lib >/dev/null
shopt -s nullglob
HIBANA_RLIBS=("${SYMBOL_TARGET_DIR}"/debug/deps/libhibana-*.rlib)
shopt -u nullglob
if [[ "${#HIBANA_RLIBS[@]}" != "1" ]]; then
  echo "kernel monomorphization quarantine violation: expected exactly one fresh hibana rlib, found ${#HIBANA_RLIBS[@]}" >&2
  printf '%s\n' "${HIBANA_RLIBS[@]}" >&2
  exit 1
fi

"${LLVM_NM}" --demangle "${HIBANA_RLIBS[0]}" 2>/dev/null \
  | rg '\bT hibana::endpoint::kernel::core::kernel_(recv|decode|send)::h[[:xdigit:]]+' \
  >"${SYMBOLS_FILE}"

for kernel in recv decode send; do
  count="$(rg "kernel_${kernel}::h" "${SYMBOLS_FILE}" | wc -l | tr -d ' ')"
  if [[ "${count}" != "1" ]]; then
    echo "kernel monomorphization quarantine violation: kernel_${kernel} symbol count is ${count}, expected 1" >&2
    cat "${SYMBOLS_FILE}" >&2
    exit 1
  fi
done

echo "kernel symbol proof passed"
