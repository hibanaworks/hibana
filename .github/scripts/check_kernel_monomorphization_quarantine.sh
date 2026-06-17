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


def read_tree(path: str) -> str:
    base = root / path
    if base.is_file():
        return base.read_text()
    return "\n".join(
        child.read_text() for child in sorted(base.rglob("*.rs")) if child.is_file()
    )


def fail(message: str) -> None:
    print(f"kernel monomorphization quarantine violation: {message}", file=sys.stderr)
    sys.exit(1)


def extract_rust_function(source: str, name: str) -> str:
    match = re.search(rf"\bfn\s+{re.escape(name)}\s*\(", source)
    if not match:
        return ""
    open_idx = source.find("{", match.end())
    if open_idx < 0:
        return ""
    depth = 0
    for idx in range(open_idx, len(source)):
        char = source[idx]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[match.start() : idx + 1]
    return ""


core = read("src/endpoint/kernel/core.rs")
runtime_types = read("src/endpoint/kernel/core/runtime_types.rs")
public_ops = read("src/endpoint/kernel/public_ops.rs")
public_poll = read("src/endpoint/kernel/public_poll.rs")
public_runtime = public_ops + "\n" + public_poll
endpoint = "\n".join(
    read(path)
    for path in [
        "src/endpoint.rs",
        "src/endpoint/public_types.rs",
        "src/endpoint/futures.rs",
        "src/endpoint/ops.rs",
        "src/endpoint/branch.rs",
        "src/endpoint/error.rs",
        "src/endpoint/tests.rs",
    ]
)
send = read("src/endpoint/send.rs")

for required in [
    "pub(crate) struct MsgCore",
    "pub(crate) struct SendRuntimeDesc",
    "pub(crate) struct RecvRuntimeDesc",
    "pub(crate) struct DecodeRuntimeDesc",
]:
    if required not in runtime_types:
        fail(f"missing non-generic runtime descriptor owner: {required}")

for required in [
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
        fail(f"{name} must be a transport/runtime-erased kernel entry point")

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
if poll_send_state and "kernel_send(self, state, payload, cx)" not in poll_send_state.group(0):
    fail("generic poll_send_state must delegate to non-generic kernel_send")

recv = read("src/endpoint/kernel/recv.rs")
decode = read("src/endpoint/kernel/decode.rs") + "\n" + read_tree("src/endpoint/kernel/decode")
poll_public_recv = extract_rust_function(public_runtime, "poll_public_recv")
if not poll_public_recv or not all(
    required in poll_public_recv
    for required in [
        "kernel_recv(",
        "logical_label",
        "validate",
        "recv_state",
        "cx",
    ]
):
    fail("generic poll_public_recv must delegate to non-generic kernel_recv with label and validation evidence")
if "kernel_decode(self, descriptor, &mut decode_state, cx)" not in public_runtime:
    fail("generic poll_public_decode must delegate to non-generic kernel_decode")

if re.search(
    r"\b(struct|pub\\(crate\\) struct)\s+(RecvDesc|DecodeDesc|SendDesc)\b",
    core + runtime_types + read("src/endpoint/kernel/recv.rs") + decode,
):
    fail("forbidden per-operation message descriptor names must not return")

if "MsgRuntimeDesc" in core + runtime_types + endpoint + send:
    fail("MsgRuntimeDesc hid operation-specific descriptor meaning")

if re.search(r"send_descriptor_.*" "un" "used", send):
    fail("send runtime descriptors must not carry inert validate/zero_payload callbacks")

for path, source in [
    ("src/endpoint.rs", endpoint),
    ("src/endpoint/send.rs", send),
]:
    if "RuntimeDesc" not in source:
        fail(f"{path} must build message-erased runtime descriptors")

if "send_future_and_runtime_descriptor_size_gates_hold" not in endpoint:
    fail("send future and runtime descriptor size gates must guard the descriptor split")

print("kernel monomorphization quarantine check passed")
PY

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
rustup component add llvm-tools-preview --toolchain "${TOOLCHAIN}" >/dev/null

SYSROOT="$(rustup run "${TOOLCHAIN}" rustc --print sysroot)"
HOST="$(rustup run "${TOOLCHAIN}" rustc -vV | sed -n 's|host: ||p')"
LLVM_NM="${SYSROOT}/lib/rustlib/${HOST}/bin/llvm-nm"
if [[ ! -x "${LLVM_NM}" ]]; then
  echo "kernel monomorphization quarantine violation: llvm-nm missing for ${TOOLCHAIN}" >&2
  exit 1
fi

SYMBOL_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-kernel-target.XXXXXX")"
SYMBOLS_FILE="$(mktemp "${TMPDIR:-/tmp}/hibana-kernel-symbols.XXXXXX")"
cleanup_symbol_proof() {
  rm -f "${SYMBOLS_FILE}"
  rm -rf "${SYMBOL_TARGET_DIR}"
}
trap cleanup_symbol_proof EXIT

CARGO_TARGET_DIR="${SYMBOL_TARGET_DIR}" cargo +"${TOOLCHAIN}" build -p hibana --lib >/dev/null
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
