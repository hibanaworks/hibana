#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

integration_rs="$(cat src/integration.rs src/integration/session_kit.rs)"
port_rs="$(
  cat src/rendezvous/port.rs
  if [[ -d src/rendezvous/port ]]; then
    find src/rendezvous/port -type f -name '*.rs' -print0 | sort -z | xargs -0 cat
  fi
)"
capability_rs="$(cat src/rendezvous/capability.rs)"
lane_port_rs="$(cat src/endpoint/kernel/lane_port.rs)"
route_table_rs="$(
  cat src/rendezvous/tables/route_table.rs
  if [[ -d src/rendezvous/tables/route_table ]]; then
    find src/rendezvous/tables/route_table -type f -name '*.rs' -print0 | sort -z | xargs -0 cat
  fi
)"
frontier_state_rs="$(cat src/endpoint/kernel/frontier_state.rs)"
eff_rs="$(cat src/eff.rs)"

for unsafe_owner in \
  src/rendezvous/tables.rs \
  src/control/cluster/core.rs \
  src/rendezvous/core.rs \
  src/endpoint/carrier.rs \
  src/endpoint/kernel/frontier.rs \
  src/endpoint/kernel/decision_state.rs \
  src/endpoint/kernel/frontier_state.rs \
  src/rendezvous/association.rs \
  src/rendezvous/capability.rs \
  src/global/const_dsl.rs \
  src/observe/tests/normalise.rs
do
  if ! grep -q "# Unsafe Owner Contract" "${unsafe_owner}"; then
    echo "unsafe-heavy owner missing module-level unsafe owner contract: ${unsafe_owner}" >&2
    exit 1
  fi
done

while IFS= read -r unsafe_file; do
  count="$(rg -o 'unsafe\s*\{' "${unsafe_file}" 2>/dev/null || true)"
  count="$(printf '%s\n' "${count}" | sed '/^$/d' | wc -l | tr -d ' ')"
  if (( count >= 20 )) && ! grep -q "# Unsafe Owner Contract" "${unsafe_file}"; then
    echo "unsafe-heavy file missing module-level unsafe owner contract: ${unsafe_file}" >&2
    exit 1
  fi
done < <(
  rg -l '\bunsafe\b' src \
    -g '*.rs' \
    -g '!src/**/tests.rs' \
    -g '!src/**/*_tests.rs' \
    -g '!src/endpoint/kernel/test_support/**'
)

python3 - <<'PY'
from pathlib import Path

missing = []
generic = []
generic_phrases = [
    "SAFETY-FIELDS:",
    "SAFETY: the surrounding function establishes the pointer, lifetime, and aliasing preconditions for this raw access.",
    "surrounding owner validates",
    "enclosing owner validates",
    "file-local owner has checked",
    "invoked unsafe API's documented preconditions",
    "raw storage identity",
    "SAFETY: owner/lifetime/aliasing invariants are established by this module's unsafe owner contract for this raw access.",
    "owner: module-local raw storage owner",
    "owner: see the adjacent SAFETY comment",
    "lifetime: caller-provided storage stays pinned",
    "aliasing: the owner contract provides",
    "initialization: the owner contract initializes",
    "initialization: initialized state is established before any read",
    "only exposes references bounded by the resident storage owner or this call",
    "reaches this block through its exclusive mutation path or a read-only snapshot path",
    "establishes the relevant initialized slot/range before this access",
    "bounds the reference to resident storage or this call",
    "excludes incompatible aliases at this access path",
    "has established the initialized slot/range before this access",
    "owns this raw storage access",
    "owns the adjacent raw access",
    "this unsafe expression",
    "owns the raw operands",
    "local owner contract establishes",
    "active owner operation",
    "adjacent owner operation",
    "addressed field",
    "addressed slot",
    "pointer field or table slot",
    "second mutable handle to the same slot",
]
for path in Path("src").rglob("*.rs"):
    text_path = str(path)
    if (
        "/test_support/" in text_path
        or "/tests/" in text_path
        or text_path.endswith("/tests.rs")
        or text_path.endswith("_tests.rs")
    ):
        continue
    lines = path.read_text().splitlines()
    for idx, line in enumerate(lines):
        if "unsafe {" not in line:
            continue
        window = "\n".join(lines[max(0, idx - 6) : min(len(lines), idx + 4)])
        if "SAFETY:" not in window:
            missing.append(f"{text_path}:{idx + 1}: unsafe block missing local SAFETY contract")
            continue
        if any(phrase in window for phrase in generic_phrases):
            generic.append(f"{text_path}:{idx + 1}: unsafe block uses the generic SAFETY escape hatch")

if missing:
    print("unsafe contract hygiene violation: every production unsafe block needs a local SAFETY contract", file=__import__("sys").stderr)
    for item in missing:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)
if generic:
    print("unsafe contract hygiene violation: SAFETY comments must name owner/lifetime/aliasing facts instead of the generic escape hatch", file=__import__("sys").stderr)
    for item in generic:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)
PY

if [[ "${integration_rs}" == *"pub unsafe fn init_in_place"* ]]; then
  echo "resident init_in_place public surface must be the safe wrapper only" >&2
  exit 1
fi

if [[ "${integration_rs}" == *"pub fn init_in_place"* ]] \
  || [[ "${integration_rs}" == *"pub fn init_resident_in_place"* ]]; then
  echo "SessionKit construction must not expose raw MaybeUninit lifecycle APIs" >&2
  exit 1
fi

if [[ "${integration_rs}" == *"ResidentSessionKit"* ]]; then
  echo "SessionKit construction must not expose a thin resident wrapper" >&2
  exit 1
fi

if [[ "${integration_rs}" != *"pub struct SessionKitStorage"* ]] \
  || [[ "${integration_rs}" != *"pub fn init(&mut self) -> &SessionKit"* ]]; then
  echo "SessionKit construction must expose only the safe Pico-class storage owner" >&2
  exit 1
fi

if [[ "${eff_rs}" == *"pub union EffData"* ]] || [[ "${eff_rs}" == *"unsafe { self.atom }"* ]]; then
  echo "effect node storage must not expose safe inactive-union reads" >&2
  exit 1
fi

if [[ "${eff_rs}" != *"pure effect node has no atom data"* ]]; then
  echo "pure effect atom access must fail fast instead of reading untagged storage" >&2
  exit 1
fi

for required in \
  "received transport frames must be committed, explicitly requeued, or explicitly discarded" \
  "received transport frame dropped without explicit commit, requeue, or discard" \
  "received transport frame requeued on a different lane" \
  "transport receive frame polled while previous frame receipt is unresolved" \
  "transport receive frame receipt is no longer current" \
  "received transport frame requeued on a different endpoint port" \
  "received transport frame requeued on a different Rx handle" \
  "fn issue(&self, port_key" \
  "assert_matches_port" \
  "fn requeue_on" \
  "transport.requeue(&mut *rx_ptr)" \
  "discard_uncommitted"
do
  if [[ "${port_rs}" != *"${required}"* ]]; then
    echo "transport receive-frame authority missing required invariant: ${required}" >&2
    exit 1
  fi
done

if [[ "${port_rs}" == *"fn discard_terminal(mut self)"* ]] \
  || [[ "${port_rs}" == *"fn discard_nonsemantic(mut self)"* ]]; then
  echo "transport frame authority must keep endpoint terminal/nonsemantic vocabulary out of the port layer" >&2
  exit 1
fi

if [[ "${port_rs}" == *"pub(crate) fn consume_receipt"* ]]; then
  echo "transport frame receipt resolution must stay private to consuming frame actions" >&2
  exit 1
fi

if rg -n "debug_assert_.*different lane|debug_assert_.*different endpoint port|debug_assert_.*different Rx handle" \
  src/rendezvous/port.rs src/endpoint/kernel/lane_port.rs >/dev/null; then
  echo "transport frame requeue identity checks must fail fast in release builds, not debug_assert only" >&2
  exit 1
fi

if rg -n "port_key: u(8|16|32|64|size)|port_identity|\\.addr\\(\\)" \
  src/rendezvous/port.rs src/endpoint/kernel/lane_port.rs >/dev/null; then
  echo "transport frame receipt must not use lossy integer-compressed or exposed Rx identities" >&2
  exit 1
fi

for required in \
  "SAFETY: \`bind_from_storage\` and \`migrate_from_storage\` are the only" \
  "rendezvous registered-token owner" \
  "Option<CapEntry>" \
  "failed cleanup attempts leave lifecycle clocks still" \
  "release scans in bounds and clears at most the matching nonce"
do
  if [[ "${capability_rs}" != *"${required}"* ]]; then
    echo "CapTable release mutation must document slot owner and initialized-entry invariants: ${required}" >&2
    exit 1
  fi
done

for required in \
  'SAFETY: `Port` owns the Rx handle for this lane.' \
  'SAFETY: `PendingSend` owns the outgoing payload borrow while this send' \
  'SAFETY: a pending send is armed, so `PendingSend` owns the outgoing'
do
  if [[ "${lane_port_rs}" != *"${required}"* ]]; then
    echo "lane_port transport raw-handle operations must carry local SAFETY contracts: ${required}" >&2
    exit 1
  fi
done

for required in \
  'SAFETY: the caller provides exclusive, writable storage for one' \
  'SAFETY: `bind_storage` owns the route-frame backing slice' \
  'SAFETY: `lane_heads` points at `lane_slots` caller-owned u16' \
  'SAFETY: `free_head` is the single u16 free-list head owned by this' \
  'SAFETY: `pending_frame_hint_masks` has one initialized slot per' \
  'SAFETY: the waiter arena contains `lane_slots *'
do
  if [[ "${route_table_rs}" != *"${required}"* ]]; then
    echo "RouteTable raw storage operations must carry local SAFETY contracts: ${required}" >&2
    exit 1
  fi
done

for required in \
  'SAFETY: `FrontierState` is initialized in one caller-owned endpoint' \
  'disjoint backing' \
  'safe frontier methods can observe them'
do
  if [[ "${frontier_state_rs}" != *"${required}"* ]]; then
    echo "FrontierState raw storage initialization must carry local SAFETY contracts: ${required}" >&2
    exit 1
  fi
done

echo "unsafe contract hygiene check passed"
