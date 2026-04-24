#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

python3 - <<'PY'
import pathlib
import re
import sys

root = pathlib.Path.cwd()
endpoint = (root / "src/endpoint.rs").read_text()
flow = (root / "src/endpoint/flow.rs").read_text()


def fail(message: str) -> None:
    print(f"message monomorphization hygiene violation: {message}", file=sys.stderr)
    sys.exit(1)


def block_after(source: str, anchor: str) -> str:
    start = source.find(anchor)
    if start < 0:
        fail(f"missing block anchor: {anchor}")
    brace = source.find("{", start)
    if brace < 0:
        fail(f"missing opening brace after: {anchor}")
    depth = 0
    for idx in range(brace, len(source)):
        ch = source[idx]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return source[start : idx + 1]
    fail(f"unterminated block: {anchor}")


raw_owners = {
    "RawRecvFuture": ("src/endpoint.rs", endpoint, ".poll_recv("),
    "RawDecodeFuture": ("src/endpoint.rs", endpoint, ".poll_decode("),
    "RawOfferFuture": ("src/endpoint.rs", endpoint, ".poll_offer("),
    "RawSendFuture": ("src/endpoint/flow.rs", flow, ".poll_send("),
}

for name, (_path, source, call) in raw_owners.items():
    if f"struct {name}" not in source:
        fail(f"{name} owner missing")
    block = block_after(source, f"impl<'e, 'r, const ROLE: u8> {name}")
    if call not in block:
        fail(f"{name} must own {call}")

for name, (_path, source, call) in raw_owners.items():
    outside = source.replace(block_after(source, f"impl<'e, 'r, const ROLE: u8> {name}"), "")
    if call in outside:
        fail(f"{call} escaped {name}; poll loop would monomorphize outside raw owner")

send_decl = re.search(r"struct\s+SendFuture\s*<([^>]*)>", flow)
if not send_decl:
    fail("SendFuture declaration missing")
if re.search(r"\b(M|A)\b", send_decl.group(1)):
    fail("SendFuture must not carry message or send-argument type parameters")

if re.search(r"impl<[^>]*(M|A)[^>]*>\s+Future\s+for\s+SendFuture", flow, re.S):
    fail("SendFuture Future impl must stay message-independent")

for future_name, raw_name in [
    ("RecvFuture", "RawRecvFuture"),
    ("DecodeFuture", "RawDecodeFuture"),
]:
    future_block = block_after(endpoint, f"impl<'e, 'r, const ROLE: u8, M> Future for {future_name}")
    if "this.raw.poll_raw(" not in future_block:
        fail(f"{future_name} must delegate progress to {raw_name}::poll_raw")
    for forbidden in ["poll_recv(", "poll_decode(", "poll_offer(", "poll_send("]:
        if forbidden in future_block:
            fail(f"{future_name} Future impl must not call {forbidden} directly")

send_future_block = block_after(flow, "impl<'e, 'r, const ROLE: u8> Future for SendFuture")
if "this.raw.poll_raw(" not in send_future_block:
    fail("SendFuture must delegate progress to RawSendFuture::poll_raw")
for forbidden in ["poll_recv(", "poll_decode(", "poll_offer(", "poll_send("]:
    if forbidden in send_future_block:
        fail(f"SendFuture Future impl must not call {forbidden} directly")

if "message_type_variation_does_not_change_future_layout" not in endpoint:
    fail("endpoint future layout must be tested across multiple message payload shapes")
if "send_future_layout_is_message_independent" not in flow:
    fail("send future layout independence test missing")

print("message monomorphization hygiene check passed")
PY
