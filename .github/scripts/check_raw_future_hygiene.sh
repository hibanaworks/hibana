#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_required() {
  local pattern="$1"
  local label="$2"
  shift 2
  if ! rg -n -F "${pattern}" "$@" >/dev/null; then
    echo "raw future hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "raw future hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

ENDPOINT_RAW_FILES=(src/endpoint/futures.rs src/endpoint/branch.rs)

check_required "struct RawRecvFuture" "RawRecvFuture owner missing" "${ENDPOINT_RAW_FILES[@]}"
check_required "struct RawDecodeFuture" "RawDecodeFuture owner missing" "${ENDPOINT_RAW_FILES[@]}"
check_required "struct RawOfferFuture" "RawOfferFuture owner missing" "${ENDPOINT_RAW_FILES[@]}"
check_required "struct RawSendFuture" "RawSendFuture owner missing" src/endpoint/flow.rs

check_required "raw: RawRecvFuture<'e, 'r, ROLE>" "RecvFuture must wrap raw recv owner" "${ENDPOINT_RAW_FILES[@]}"
check_required "raw: RawDecodeFuture<'e, 'r, ROLE>" "DecodeFuture must wrap raw decode owner" "${ENDPOINT_RAW_FILES[@]}"
check_required "raw: RawOfferFuture<'e, 'r, ROLE>" "OfferFuture must wrap raw offer owner" "${ENDPOINT_RAW_FILES[@]}"
check_required "raw: RawSendFuture<'a, 'e, 'r, ROLE>" "SendFuture must wrap raw send owner" src/endpoint/flow.rs

check_required "fn poll_raw(" "endpoint raw futures must own poll_raw" "${ENDPOINT_RAW_FILES[@]}"
check_required "fn poll_raw(" "send raw future must own poll_raw" src/endpoint/flow.rs
check_required "payload: &'a M::Payload" "send must accept only the projected payload reference" src/endpoint/flow.rs
check_required "kernel::RawSendPayload::from_typed::<M::Payload>(payload)" "send must own a present typed payload without public absence" src/endpoint/flow.rs
check_required "endpoint.poll_send(cx, self.payload.take())" "send payload borrow must be passed from the future, not staged in endpoint state" src/endpoint/flow.rs
check_absent "set_public_send_payload" "send payload borrow must not be staged in endpoint resident state" src/endpoint src/endpoint/kernel
check_absent \
  "ErasedSendInput|mod[[:space:]]+sealed|Into<Option<&'a M::Payload>>|\\.send\\(None\\)" \
  "Flow::send must not retain optional payload or private-bound argument adapters" \
  src/endpoint/flow.rs

check_absent \
  "struct[[:space:]]+SendFuture[^{;]*<[^>{;]*(M|A)[^>{;]*>" \
  "SendFuture must not be parameterized by message or send-argument type" \
  src/endpoint/flow.rs

check_absent \
  "pub[[:space:]]+(struct|trait)[[:space:]]+(SendValue|ErasedSendInput)\\b" \
  "send argument resolver must not become a public concept" \
  src/endpoint/flow.rs

check_absent \
  "pub[[:space:]]+(trait|struct|enum)[[:space:]]+(FlowSendArg|SendOutcomeKind|CapFlow|FlowInner)\\b" \
  "flow send internals must not become public concepts" \
  src/endpoint src/lib.rs src/g.rs src/runtime.rs

check_absent \
  "impl<'[^']*,[[:space:]]*'[^']*,[[:space:]]*const ROLE: u8,[^>]*(M|A)[^>]*>[[:space:]]+Future[[:space:]]+for[[:space:]]+SendFuture" \
  "send future poll body must stay message-independent" \
  src/endpoint/flow.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "raw future hygiene check passed"
