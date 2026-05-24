#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

integration_rs="$(cat src/integration.rs)"
lane_port_rs="$(cat src/endpoint/kernel/runtime/lane_port.rs)"
port_rs="$(cat src/rendezvous/port.rs)"
capability_rs="$(cat src/rendezvous/capability.rs)"
unsafe_occurrences="$(
  rg -o '\bunsafe\b' src \
    -g '*.rs' \
    -g '!src/endpoint/kernel/test_support/**' \
    | wc -l | tr -d ' '
)"
safety_comments="$(
  rg -n 'SAFETY:' src \
    -g '*.rs' \
    -g '!src/endpoint/kernel/test_support/**' \
    | wc -l | tr -d ' '
)"
unsafe_fns="$(
  rg -n '\bunsafe fn\b' src \
    -g '*.rs' \
    -g '!src/endpoint/kernel/test_support/**' \
    | wc -l | tr -d ' '
)"

for unsafe_owner in \
  src/rendezvous/tables.rs \
  src/control/cluster/core.rs \
  src/rendezvous/core.rs \
  src/endpoint/carrier.rs \
  src/endpoint/kernel/runtime/frontier.rs \
  src/endpoint/kernel/runtime/route_state.rs \
  src/endpoint/kernel/runtime/frontier_state.rs \
  src/rendezvous/association.rs \
  src/rendezvous/capability.rs \
  src/endpoint/kernel/runtime/inbox.rs
do
  if ! grep -q "# Unsafe Owner Contract" "${unsafe_owner}"; then
    echo "unsafe-heavy owner missing module-level unsafe owner contract: ${unsafe_owner}" >&2
    exit 1
  fi
done

if ! grep -q "pub unsafe fn init_in_place" src/integration.rs; then
  echo "SessionKit::init_in_place must remain explicitly unsafe" >&2
  exit 1
fi

if ! grep -q "pub struct SessionKitStorage" src/integration.rs \
  || ! grep -q "pub fn init(&mut self) -> ResidentSessionKit" src/integration.rs; then
  echo "host-managed SessionKit construction must expose the safe storage guard" >&2
  exit 1
fi

if ! grep -q "/// # Safety" src/integration.rs; then
  echo "public unsafe resident initialization must document its Safety contract" >&2
  exit 1
fi

for required in \
  "must remain pinned and initialized" \
  "endpoint borrowed from the kit" \
  "SessionKitStorage"
do
  if [[ "${integration_rs}" != *"${required}"* ]]; then
    echo "SessionKit::init_in_place Safety docs missing required invariant: ${required}" >&2
    exit 1
  fi
done

for required in \
  "pub(crate) use crate::rendezvous::port::ReceivedFrame" \
  "frame.assert_matches_port(port);"
do
  if [[ "${lane_port_rs}" != *"${required}"* ]]; then
    echo "ReceivedFrame rollback authority missing required invariant: ${required}" >&2
    exit 1
  fi
done

if [[ "${lane_port_rs}" == *"PortRecvFrameReceipt"* ]]; then
  echo "lane_port must traffic only in ReceivedFrame; raw frame receipts stay private to rendezvous::port" >&2
  exit 1
fi

for required in \
  "struct PortRecvFrameReceipt" \
  "pub(crate) struct ReceivedFrame<'r>" \
  "Option<PortRecvFrameReceipt>" \
  "fn consume_receipt(&mut self)" \
  "fn discard_uncommitted(mut self)" \
  "fn assert_matches_port" \
  "impl Drop for ReceivedFrame" \
  "received transport frames must be committed, explicitly requeued, or explicitly discarded" \
  "received transport frame dropped without explicit commit, requeue, or discard" \
  "received transport frame requeued on a different lane" \
  "transport receive frame polled while previous frame receipt is unresolved" \
  "transport receive frame receipt is no longer current" \
  "received transport frame requeued on a different endpoint port" \
  "received transport frame requeued on a different Rx handle" \
  "issue_recv_frame_receipt"
do
  if [[ "${port_rs}" != *"${required}"* ]]; then
    echo "Port receive receipt state missing required invariant: ${required}" >&2
    exit 1
  fi
done

if [[ "${port_rs}" == *"fn discard_terminal(mut self)"* ]] \
  || [[ "${port_rs}" == *"fn discard_nonsemantic(mut self)"* ]]; then
  echo "ReceivedFrame must keep endpoint terminal/nonsemantic vocabulary out of the port layer" >&2
  exit 1
fi

if [[ "${port_rs}" == *"pub(crate) struct PortRecvFrameReceipt"* ]]; then
  echo "Port receive frame receipt must stay private behind ReceivedFrame" >&2
  exit 1
fi

if [[ "${lane_port_rs}" == *"let _ = port;"* ]]; then
  echo "ReceivedFrame construction must use the producing port receipt, not discard it" >&2
  exit 1
fi

if [[ "${port_rs}" == *$'#[derive(Clone, Copy)]\npub(crate) struct PortRecvFrameReceipt'* ]] \
  || [[ "${port_rs}" == *$'#[derive(Clone)]\npub(crate) struct PortRecvFrameReceipt'* ]] \
  || [[ "${port_rs}" == *$'#[derive(Copy)]\npub(crate) struct PortRecvFrameReceipt'* ]]; then
  echo "Port receive frame receipt must be affine and must not be Clone/Copy" >&2
  exit 1
fi

if [[ "${lane_port_rs}" != *"mut frame: ReceivedFrame<'r>"* ]] \
  || [[ "${lane_port_rs}" != *"frame.consume_receipt();"* ]]; then
  echo "ReceivedFrame requeue must explicitly consume the frame receipt after transport rollback" >&2
  exit 1
fi

if [[ "${port_rs}" == *$'impl Drop for ReceivedFrame<\'_> {\n    fn drop(&mut self) {\n        self.consume_receipt();'* ]]; then
  echo "ReceivedFrame Drop must fail fast on unresolved receipts instead of silently consuming them" >&2
  exit 1
fi

if [[ "${lane_port_rs}" == *"debug_assert_eq!(port.lane().as_wire() as usize, frame.lane_idx())"* ]]; then
  echo "ReceivedFrame requeue must fail fast in release builds, not debug_assert only" >&2
  exit 1
fi

if [[ "${lane_port_rs}" == *"port_key: u32"* ]] \
  || [[ "${lane_port_rs}" == *"port_identity"* ]] \
  || [[ "${lane_port_rs}" == *"addr()"* ]]; then
  echo "ReceivedFrame receipt must not use lossy integer-compressed or exposed Rx identities" >&2
  exit 1
fi

for required in \
  "SAFETY: \`bind_from_storage\` and \`migrate_from_storage\` are the only" \
  "rendezvous-local table owner" \
  "Option<CapEntry>"
do
  if [[ "${capability_rs}" != *"${required}"* ]]; then
    echo "CapTable claim mutation must document slot owner and initialized-entry invariants: ${required}" >&2
    exit 1
  fi
done

if (( unsafe_occurrences > 1023 )); then
  echo "unsafe surface grew: ${unsafe_occurrences} > 1023" >&2
  exit 1
fi

if (( unsafe_fns > 192 )); then
  echo "unsafe fn surface grew: ${unsafe_fns} > 192" >&2
  exit 1
fi

if (( safety_comments < 25 )); then
  echo "unsafe contract comments regressed: ${safety_comments} < 25" >&2
  exit 1
fi

echo "unsafe contract hygiene check passed"
