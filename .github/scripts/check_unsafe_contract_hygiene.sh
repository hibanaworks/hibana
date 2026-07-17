#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

runtime_rs="$(cat src/runtime.rs src/runtime/session_kit.rs)"
port_rs="$(
  cat src/rendezvous/recv_frame_receipt.rs
  cat src/rendezvous/port.rs
  if [[ -d src/rendezvous/port ]]; then
    find src/rendezvous/port -type f -name '*.rs' -print0 | sort -z | xargs -0 cat
  fi
)"
lane_port_rs="$(cat src/endpoint/kernel/lane_port.rs)"
frontier_state_rs="$(cat src/endpoint/kernel/frontier_state.rs)"
eff_rs="$(cat src/eff.rs)"

count_unsafe_blocks() {
  local roots=("$@")
  local matches
  set +e
  matches="$(rg -n 'unsafe\s*\{' "${roots[@]}" \
    -g '*.rs' \
    -g '!src/**/tests.rs' \
    -g '!src/**/*_tests.rs' \
    -g '!src/endpoint/kernel/test_support/**')"
  local status=$?
  set -e
  if [[ "${status}" -gt 1 ]]; then
    echo "unsafe block count failed for: ${roots[*]}" >&2
    exit 1
  fi
  if [[ -z "${matches}" ]]; then
    echo 0
  else
    printf '%s\n' "${matches}" | wc -l | tr -d ' '
  fi
}

assert_unsafe_limit() {
  local label="$1"
  local limit="$2"
  shift 2
  local count
  count="$(count_unsafe_blocks "$@")"
  if (( count > limit )); then
    echo "unsafe block count increased for ${label}: ${count} > ${limit}" >&2
    exit 1
  fi
}

assert_unsafe_limit "production" 377 src
assert_unsafe_limit "frontier scratch owner" 6 src/endpoint/kernel/frontier/scratch.rs
assert_unsafe_limit "endpoint lease owner" 4 \
  src/rendezvous/core/endpoint_leases.rs \
  src/rendezvous/core/storage_layout.rs \
  src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs
assert_unsafe_limit "resolver erased storage owner" 5 src/session/cluster/core/dynamic_resolvers.rs
assert_unsafe_limit "SessionKit init owner" 4 src/runtime/session_kit.rs

for unsafe_owner in \
  src/session/cluster/core.rs \
  src/rendezvous/core.rs \
  src/endpoint/carrier.rs \
  src/endpoint/kernel/frontier.rs \
  src/endpoint/kernel/decision_state.rs \
  src/endpoint/kernel/decision_state/route_arm_history.rs \
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
    "initialization owns exclusive writable storage",
    "offset was checked against the backing allocation",
    "caller supplies exclusive uninitialized storage",
    "pointer comes from pinned owner storage",
    "pointer and length are carved from one backing slice",
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
    "this owner validates the concrete pointer identity and initialized storage before raw access.",
    "the owner tracks the initialized prefix and this slot is inside that initialized range.",
    "the table owner tracks the initialized prefix and checks this slot before reading initialized storage.",
]
comments = []
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
        if "SAFETY:" in line:
            text = line.strip()
            if text.startswith("/*"):
                text = text[2:].strip()
            if text.startswith("//"):
                text = text[2:].strip()
            if text.endswith("*/"):
                text = text[:-2].strip()
            comments.append((text, text_path, idx + 1))
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

from collections import Counter
duplicate_failures = []
for text, count in Counter(text for text, _, _ in comments).items():
    if count >= 3:
        sites = ", ".join(f"{path}:{line}" for item, path, line in comments if item == text)
        duplicate_failures.append(
            f"SAFETY comment duplicated {count} times: {text} [{sites}]"
        )
if duplicate_failures:
    print(
        "unsafe contract hygiene violation: duplicate SAFETY comments must be factored or made owner-specific",
        file=__import__("sys").stderr,
    )
    for item in duplicate_failures:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)

category_terms = {
    "owner": [
        "owner", "owns", "owned", "rendezvous",
        "frontier", "resolver", "sessionkit", "session", "table", "arena",
        "column", "storage", "payload", "free-list", "replacement",
        "destination", "source", "scratch", "callback", "endpoint",
        "carrier", "lane", "cursor", "slab", "bucket", "assoc", "waiter",
        "entry", "slot", "view", "buffer", "port", "receipt", "efflist",
        "descriptor", "image", "program",
    ],
    "init": [
        "init", "initialized", "registered", "written", "staged", "bound",
        "binding", "publishing", "published", "stored", "installed",
        "matching", "empty", "free-list", "state", "frame", "column",
        "section", "slot", "storage", "arena", "exposed", "succeeds",
        "returned", "returning", "computed", "produced", "created", "reset",
        "cleared", "clearing", "starts", "zeroed", "armed", "live", "ready",
        "valid", "validated", "paired", "derived", "issued", "selected",
        "removed", "removing", "compaction", "fragments",
    ],
    "alias": [
        "borrow", "exclusive", "shared", "mutable", "single", "token",
        "not expose", "only live", "only creates", "same", "matching",
        "fresh", "local", "not linked", "copy", "copies", "sole", "one",
        "duration", "reference", "live", "serializes", "transition",
        "mutation", "mutating", "returned", "tied", "drop", "exactly once",
        "belongs", "therefore", "until", "before", "during", "source",
        "destination", "replacement", "current", "callback", "scoped",
        "remains", "consumed", "owns", "owned", "&mut", "read-only",
        "read only", "reads", "read", "writes", "write", "call", "view",
        "slice", "lease", "pointer", "identity", "one-shot", "interior",
        "paired", "span", "segment", "pool", "clearing", "compaction",
        "passed", "returns", "without aliasing", "without creating a borrow",
    ],
    "bounds": [
        "idx", "index", "count", "capacity", "slot", "range", "inside",
        "bounded", "layout", "total_bytes", "route_slots", "lane_slots",
        "entries", "entry", "bytes", "align", "size", "offset", "u16",
        "len", "ptr", "self.storage", "matching", "typed", "assert",
        "asserted", "trampoline", "resolver-id", "validation", "pointer",
        "slice", "u8", "row", "word", "cell", "section", "arena",
        "segment", "span", "start", "end", "field", "fields", "identity",
        "fragments",
    ],
}
category_failures = []
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
        window = "\n".join(lines[max(0, idx - 6) : min(len(lines), idx + 4)]).lower()
        missing_terms = [
            name
            for name, terms in category_terms.items()
            if not any(term in window for term in terms)
        ]
        if missing_terms:
            category_failures.append(
                f"{text_path}:{idx + 1}: unsafe block missing concrete SAFETY terms: {', '.join(missing_terms)}"
            )
if category_failures:
    print(
        "unsafe contract hygiene violation: production unsafe blocks must name owner/init/alias/bounds facts",
        file=__import__("sys").stderr,
    )
    for item in category_failures:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)

focus_paths = [
    Path("src/rendezvous/core/endpoint_leases.rs"),
    Path("src/rendezvous/core/storage_layout.rs"),
    Path("src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs"),
    Path("src/endpoint/kernel/frontier/scratch.rs"),
    Path("src/session/cluster/core/dynamic_resolvers.rs"),
    Path("src/runtime/session_kit.rs"),
]
focused_deny = [
    "initialization owns exclusive writable storage",
    "offset was checked against the backing allocation",
    "caller supplies exclusive uninitialized storage",
    "pointer comes from pinned owner storage",
    "pointer and length are carved from one backing slice",
]
focused_comments = []
focused_failures = []
for path in focus_paths:
    lines = path.read_text().splitlines()
    for idx, line in enumerate(lines):
        if "unsafe {" not in line:
            continue
        window = "\n".join(lines[max(0, idx - 6) : min(len(lines), idx + 4)])
        for phrase in focused_deny:
            if phrase in window:
                focused_failures.append(f"{path}:{idx + 1}: focused owner keeps generic SAFETY phrase: {phrase}")
        lower = window.lower()
        categories = {
            "owner": [
                "owner", "owns", "owned", "rendezvous",
                "frontier", "resolver", "sessionkit", "table", "arena", "column",
                "storage", "payload", "free-list", "replacement", "destination",
                "source", "scratch", "callback",
            ],
            "init": [
                "init", "initialized", "registered", "written", "staged", "bound",
                "binding", "publishing", "published", "stored", "installed",
                "matching", "empty", "free-list", "state", "frame", "column",
                "section", "slot", "storage", "arena", "exposed", "succeeds",
            ],
            "alias": [
                "borrow", "exclusive", "shared", "mutable", "single", "token",
                "not expose", "only live", "only creates", "same", "matching",
                "fresh", "local", "not linked", "copy", "copies", "sole", "one",
                "duration", "reference", "live", "serializes", "transition",
                "mutation", "mutating", "returned", "tied", "drop", "exactly once",
                "belongs", "therefore", "until", "before", "during", "source",
                "destination", "replacement", "current", "callback", "scoped",
                "remains",
            ],
            "bounds": [
                "idx", "index", "count", "capacity", "slot", "range", "inside",
                "bounded", "layout", "total_bytes", "route_slots", "lane_slots",
                "entries", "entry", "bytes", "align", "size", "offset", "u16",
                "len", "ptr", "self.storage", "matching", "typed", "assert",
                "asserted", "trampoline", "resolver-id", "validation",
            ],
        }
        missing_keys = [
            name
            for name, terms in categories.items()
            if not any(term in lower for term in terms)
        ]
        if missing_keys:
            focused_failures.append(
                f"{path}:{idx + 1}: focused unsafe block missing concrete SAFETY terms: {', '.join(missing_keys)}"
            )
    for idx, line in enumerate(lines, 1):
        if "SAFETY:" not in line:
            continue
        text = line.strip()
        if text.startswith("/*"):
            text = text[2:].strip()
        if text.startswith("//"):
            text = text[2:].strip()
        if text.endswith("*/"):
            text = text[:-2].strip()
        focused_comments.append((text, path, idx))
from collections import Counter
counts = Counter(text for text, _, _ in focused_comments)
for text, count in counts.items():
    if count >= 3:
        sites = ", ".join(f"{path}:{idx}" for item, path, idx in focused_comments if item == text)
        focused_failures.append(f"focused SAFETY comment duplicated {count} times: {text} [{sites}]")
if focused_failures:
    print("unsafe contract hygiene violation: focused owner-local unsafe boundary regressed", file=__import__("sys").stderr)
    for item in focused_failures:
        print(item, file=__import__("sys").stderr)
    raise SystemExit(1)

registry_ops = Path("src/session/lease/core/registry_ops.rs")
registry = registry_ops.read_text()
init_call = registry.find("Rendezvous::init_in_slab_auto(id, resources, transport)")
ptr_init = registry.find("NonNull::new_unchecked(rendezvous)")
link = registry.find("rendezvous_ref.link_registry_next(self.head.get());")
publish = registry.find("self.head.set(Some(rendezvous_ptr));")
if min(init_call, ptr_init, link, publish) == -1 or not (init_call < ptr_init < link < publish):
    print(
        "unsafe contract hygiene violation: intrusive rendezvous registry must initialize, witness non-null, link, then publish",
        file=__import__("sys").stderr,
    )
    raise SystemExit(1)
init_window = " ".join(registry[max(0, init_call - 220) : init_call].replace("*", " ").split())
ptr_window = " ".join(registry[max(0, ptr_init - 220) : ptr_init].replace("*", " ").split())
for term, window in [
    ("SAFETY:", init_window),
    ("caller slab", init_window),
    ("SAFETY:", ptr_window),
    ("non-null", ptr_window),
    ("initialized header", ptr_window),
    ("pinned", ptr_window),
]:
    if term not in window:
        print(f"{registry_ops}: missing intrusive registry SAFETY fact: {term}", file=__import__("sys").stderr)
        raise SystemExit(1)
if "RendezvousEntry" in registry or "allocate_external_persistent_sidecar_bytes" in registry:
    print("unsafe contract hygiene violation: detached registry-node residue returned", file=__import__("sys").stderr)
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
  echo "SessionKit construction must expose only the safe resource-bounded storage owner" >&2
  exit 1
fi

if [[ "${eff_rs}" == *"EffKind"* ]] \
  || [[ "${eff_rs}" == *"EffStruct"* ]] \
  || [[ "${eff_rs}" == *"EffData"* ]]; then
  echo "source events must not retain an uninhabited pure/atom tagged representation" >&2
  exit 1
fi

if [[ "${eff_rs}" != *"pub(crate) struct EffAtom"* ]]; then
  echo "source events must retain one direct EffAtom representation" >&2
  exit 1
fi

for required in \
  "received transport frames must be committed, explicitly requeued, or explicitly discarded" \
  "impl Drop for ReceivedFrameCore" \
  "if self.receipt.is_current()" \
  "RecvFrameReceiptPhase::Outstanding" \
  "owner: Option<RecvFrameReceiptOwner>" \
  "None => crate::invariant()" \
  "crate::invariant()" \
  "if self.lane_wire() != port.lane().as_wire()" \
  "if owner.port_key != port_key" \
  "if owner.state != receipt_state" \
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

if [[ "${port_rs}" == *"if self.state.is_null()"* ]] \
  || [[ "${port_rs}" == *"if self.outstanding.replace(true)"* ]]; then
  echo "transport receive-frame authority must not use raw-null or boolean phase fallback" >&2
  exit 1
fi

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
  'SAFETY: the scoped scratch lease keeps `outgoing` stable for this call,' \
  'SAFETY: the pending state records an armed transport poll and this'
do
  if [[ "${lane_port_rs}" != *"${required}"* ]]; then
    echo "lane_port transport raw-handle operations must carry local SAFETY contracts: ${required}" >&2
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
