#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

runtime_rs="$(cat src/runtime.rs src/runtime/session_kit.rs)"
port_rs="$(
  cat src/rendezvous/port.rs
  if [[ -d src/rendezvous/port ]]; then
    find src/rendezvous/port -type f -name '*.rs' -print0 | sort -z | xargs -0 cat
  fi
)"
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
  src/session/cluster/core.rs \
  src/rendezvous/core.rs \
  src/endpoint/carrier.rs \
  src/endpoint/kernel/frontier.rs \
  src/endpoint/kernel/decision_state.rs \
  src/endpoint/kernel/frontier_state.rs \
  src/rendezvous/association.rs \
  src/global/const_dsl.rs
do
  if ! grep -q "# Unsafe Owner Contract" "${unsafe_owner}"; then
    echo "unsafe-heavy owner missing module-level unsafe owner contract: ${unsafe_owner}" >&2
    exit 1
  fi
done

while IFS= read -r unsafe_file; do
  set +e
  count="$(rg -o 'unsafe\s*\{' "${unsafe_file}")"
  status=$?
  set -e
  if [[ "${status}" -gt 1 ]]; then
    echo "unsafe block search failed: ${unsafe_file}" >&2
    exit 1
  fi
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
    "excludes alias conflicts at this access path",
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
            generic.append(f"{text_path}:{idx + 1}: unsafe block uses the generic SAFETY comment")

if missing:
    print("unsafe contract hygiene violation: every production unsafe block needs a local SAFETY contract", file=__import__("sys").stderr)
    for item in missing:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)
if generic:
    print("unsafe contract hygiene violation: SAFETY comments must name owner/lifetime/aliasing facts instead of the generic comment", file=__import__("sys").stderr)
    for item in generic:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)

registry_ops = Path("src/session/lease/core/registry_ops.rs")
registry = registry_ops.read_text()
init_call = registry.find("RendezvousEntry::init_from_parts(")
if init_call == -1:
    print("unsafe contract hygiene violation: registry entry initialization disappeared", file=__import__("sys").stderr)
    raise SystemExit(1)
window = " ".join(registry[max(0, init_call - 700) : init_call].replace("*", " ").split())
required_terms = [
    "SAFETY:",
    "rendezvous",
    "Rendezvous::init_in_slab_auto",
    "non-null",
    "slab-pinned",
    "entry_ptr",
    "persistent sidecar",
    "RendezvousEntry",
    "size/align",
    "not published",
    "self.head",
    "initialized list head",
    "published to `self.head` only",
]
missing_terms = [term for term in required_terms if term not in window]
if missing_terms:
    print(
        "unsafe contract hygiene violation: registry entry initialization must carry the concrete entry_ptr/rendezvous/self.head/publish SAFETY contract",
        file=__import__("sys").stderr,
    )
    for term in missing_terms:
        print(f"{registry_ops}: missing registry SAFETY fact: {term}", file=__import__("sys").stderr)
    raise SystemExit(1)
PY

if [[ "${runtime_rs}" == *"pub unsafe fn init_in_place"* ]]; then
  echo "resident init_in_place public surface must be the safe wrapper only" >&2
  exit 1
fi

if [[ "${runtime_rs}" == *"pub fn init_in_place"* ]] \
  || [[ "${runtime_rs}" == *"pub fn init_resident_in_place"* ]]; then
  echo "SessionKit construction must not expose raw MaybeUninit lifecycle APIs" >&2
  exit 1
fi

if [[ "${runtime_rs}" == *"ResidentSessionKit"* ]]; then
  echo "SessionKit construction must not expose a thin resident wrapper" >&2
  exit 1
fi

if [[ "${runtime_rs}" != *"pub struct SessionKitStorage"* ]] \
  || [[ "${runtime_rs}" != *"pub fn init(&mut self) -> &SessionKit"* ]]; then
  echo "SessionKit construction must expose only the safe Pico-class storage owner" >&2
  exit 1
fi

if [[ "${eff_rs}" == *"pub union EffData"* ]] || [[ "${eff_rs}" == *"unsafe { self.atom }"* ]]; then
  echo "effect node storage must not expose safe inactive-union reads" >&2
  exit 1
fi

if [[ "${eff_rs}" != *"EffKind::Pure => crate::invariant()"* ]] \
  || [[ "${eff_rs}" != *"EffKind::Atom => self.data.atom()"* ]]; then
  echo "pure effect atom access must fail fast through the runtime invariant path" >&2
  exit 1
fi

for required in \
  "received transport frames must be committed, explicitly requeued, or explicitly discarded" \
  "impl Drop for ReceivedFrameCore" \
  "if self.receipt.is_current()" \
  "if self.outstanding.replace(true)" \
  "if !self.outstanding.get()" \
  "crate::invariant()" \
  "if self.lane_wire() != port.lane().as_wire()" \
  "if self.port_key != port_key" \
  "if self.state != receipt_state" \
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

check_absent \
  "debug_assert_.*different lane|debug_assert_.*different endpoint port|debug_assert_.*different Rx handle" \
  "transport frame requeue identity checks must fail fast in release builds, not debug_assert only" \
  src/rendezvous/port.rs src/endpoint/kernel/lane_port.rs

check_absent \
  "port_key: u(8|16|32|64|size)|port_identity|\\.addr\\(\\)" \
  "transport frame receipt must not use lossy integer-compressed or exposed Rx identities" \
  src/rendezvous/port.rs src/endpoint/kernel/lane_port.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

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
  'SAFETY: `lane_heads` points at `lane_slots` u16 entries' \
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
