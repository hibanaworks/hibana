#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# Manager load/activate/revert primitives must not leak as public methods.
if rg -n "pub fn (load_begin|load_begin_raw|load_chunk|load_chunk_raw|load_commit|load_commit_raw|activate|schedule_activate|on_decision_boundary|revert|set_policy_mode|set_policy_mode_staged)\\b" src/runtime; then
  echo "mgmt boundary violation: manager mutators must not be public" >&2
  exit 1
fi

# Management must stay on the ordinary-prefix surface, not regrow session/helper hubs.
if rg -n "pub mod session\\b" src/substrate.rs; then
  echo "mgmt boundary violation: substrate::mgmt::session must not return" >&2
  exit 1
fi

if rg -n "pub (async fn |fn )(enter_controller|enter_cluster|enter_stream_controller|enter_stream_cluster|drive_controller|drive_cluster|drive_stream_cluster|drive_stream_controller)\\b" \
  src/substrate.rs \
  src/runtime/mgmt.rs \
  src/runtime/mgmt/request_reply.rs \
  src/runtime/mgmt/observe_stream.rs \
  src/runtime/mgmt/test_support.rs; then
  echo "mgmt boundary violation: management helper family must not return" >&2
  exit 1
fi

if rg -n "pub use crate::runtime::mgmt::TapBatch;|\\bTapBatch\\b" src/substrate.rs; then
  echo "mgmt boundary violation: public mgmt surface must stay on TapEvent only" >&2
  exit 1
fi

echo "mgmt boundary check passed"
