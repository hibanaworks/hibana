#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FAILED=0

extract_public_api() {
  local source="$1"
  awk '
    function trim(s) {
      sub(/^[[:space:]]+/, "", s)
      sub(/[[:space:]]+$/, "", s)
      return s
    }

    function normalize_surface_item(s) {
      s = trim(s)
      if (s == "pub use crate::epf::verifier::Header;") {
        return "pub use Header;"
      }
      if (s == "pub use crate::epf::vm::Slot;") {
        return "pub use Slot;"
      }
      if (s == "pub use crate::control::types::{Many, One};" ||
          s == "pub use crate::control::types::{One, Many};") {
        return "pub use {One, Many};"
      }
      return s
    }

    function flush() {
      if (buf != "") {
        print normalize_surface_item(buf)
        buf = ""
      }
      in_use = 0
      in_decl = 0
    }

    BEGIN {
      in_use = 0
      in_decl = 0
      buf = ""
    }

    {
      if (in_use) {
        buf = buf " " trim($0)
        if ($0 ~ /;/) {
          flush()
        }
        next
      }

      if (in_decl) {
        buf = buf " " trim($0)
        if ($0 ~ /[;{][[:space:]]*$/) {
          flush()
        }
        next
      }

      if ($0 ~ /^[[:space:]]*pub use /) {
        buf = trim($0)
        if ($0 ~ /;/) {
          flush()
        } else {
          in_use = 1
        }
        next
      }

      if ($0 ~ /^[[:space:]]*pub type /) {
        buf = trim($0)
        if ($0 ~ /;/) {
          flush()
        } else {
          in_use = 1
        }
        next
      }

      if ($0 ~ /^[[:space:]]*pub ((const|async|unsafe)[[:space:]]+)*fn / ||
          $0 ~ /^[[:space:]]*pub mod / ||
          $0 ~ /^[[:space:]]*pub struct / ||
          $0 ~ /^[[:space:]]*pub enum / ||
          $0 ~ /^[[:space:]]*pub union / ||
          $0 ~ /^[[:space:]]*pub trait / ||
          $0 ~ /^[[:space:]]*pub (const|static) /) {
        buf = trim($0)
        if ($0 ~ /[;{][[:space:]]*$/) {
          flush()
        } else {
          in_decl = 1
        }
        next
      }

      if ($0 ~ /^[[:space:]]*pub [A-Za-z0-9_]+:/) {
        print trim($0)
      }
    }

    END {
      if (in_use || in_decl) {
        exit 2
      }
    }
  ' "${source}"
}

extract_g_surface_api() {
  local source="$1"
  awk '
    function trim(s) {
      sub(/^[[:space:]]+/, "", s)
      sub(/[[:space:]]+$/, "", s)
      return s
    }

    function emit_use_item(item) {
      item = trim(item)
      if (item == "") {
        return
      }
      if (item ~ /^crate::global::advanced$/) {
        print "pub use advanced;"
        return
      }
      if (item ~ /(^|::)Program$/) {
        print "pub use Program;"
        return
      }
      sub(/^crate::global::/, "", item)
      print "pub use " item ";"
    }

    BEGIN {
      in_use = 0
      buf = ""
    }

    {
      if (!in_use && $0 !~ /^[[:space:]]*pub use /) {
        next
      }

      if (in_use) {
        buf = buf " " trim($0)
      } else {
        buf = trim($0)
        in_use = 1
      }

      if ($0 !~ /;/) {
        next
      }

      line = buf
      buf = ""
      in_use = 0

      sub(/^pub use /, "", line)
      sub(/;[[:space:]]*$/, "", line)

      if (line ~ /\{/) {
        prefix = line
        sub(/\{.*/, "", prefix)
        items = line
        sub(/^[^{]*\{/, "", items)
        sub(/\}[[:space:]]*$/, "", items)
        n = split(items, parts, ",")
        for (i = 1; i <= n; i++) {
          item = trim(parts[i])
          if (item != "") {
            emit_use_item(prefix item)
          }
        }
      } else {
        emit_use_item(line)
      }
    }

    END {
      if (in_use) {
        exit 2
      }
    }
  ' "${source}"
}

check_surface() {
  local label="$1"
  local source="$2"
  local allowlist="$3"
  local actual

  if [[ ! -f "${source}" ]]; then
    echo "missing source: ${source}" >&2
    FAILED=1
    return
  fi

  if [[ ! -f "${allowlist}" ]]; then
    echo "missing allowlist: ${allowlist}" >&2
    FAILED=1
    return
  fi

  actual="$(mktemp)"
  extract_public_api "${source}" > "${actual}"
  if ! diff -u "${allowlist}" "${actual}"; then
    echo "public API allowlist mismatch: ${label} (${source})" >&2
    FAILED=1
  fi
  rm -f "${actual}"
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n "${pattern}" "$@"; then
    echo "forbidden public API surface detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "forbidden public API surface detected: ${label}" >&2
    FAILED=1
  fi
}

check_surface \
  "crate root" \
  "${ROOT_DIR}/src/lib.rs" \
  "${ROOT_DIR}/.github/allowlists/lib-public-api.txt"

check_g_surface() {
  local source="$1"
  local allowlist="$2"
  local actual

  actual="$(mktemp)"
  extract_g_surface_api "${source}" > "${actual}"
  if ! diff -u "${allowlist}" "${actual}"; then
    echo "public API allowlist mismatch: g surface (${source})" >&2
    FAILED=1
  fi
  rm -f "${actual}"
}

check_g_surface \
  "${ROOT_DIR}/src/g.rs" \
  "${ROOT_DIR}/.github/allowlists/g-public-api.txt"

check_surface \
  "endpoint surface" \
  "${ROOT_DIR}/src/endpoint.rs" \
  "${ROOT_DIR}/.github/allowlists/endpoint-public-api.txt"

check_surface \
  "substrate surface" \
  "${ROOT_DIR}/src/substrate.rs" \
  "${ROOT_DIR}/.github/allowlists/substrate-public-api.txt"

# Internal coordinate accessors must not leak through the public API.
check_absent "\\bpub\\s+fn\\s+eff_index\\s*\\(" \
  "public eff_index accessor" \
  "${ROOT_DIR}/src"

check_absent "\\bpub\\s+fn\\s+scope_id\\s*\\(" \
  "public scope_id accessor" \
  "${ROOT_DIR}/src"

check_absent "\\bpub\\s+fn\\s+scope_trace\\s*\\(" \
  "public scope_trace accessor" \
  "${ROOT_DIR}/src"

# `PhaseCursor` and `BranchMeta` are internal coordinates and must stay non-public.
check_absent "\\bpub\\s+use\\s+[^;]*\\bPhaseCursor\\b" \
  "PhaseCursor public re-export" \
  "${ROOT_DIR}/src/global.rs" "${ROOT_DIR}/src/lib.rs"

check_absent "\\bpub\\s+struct\\s+BranchMeta\\b" \
  "BranchMeta public struct" \
  "${ROOT_DIR}/src/endpoint/cursor.rs"

check_absent "\\bpub\\s+scope_id\\s*:" \
  "BranchMeta public scope_id field" \
  "${ROOT_DIR}/src/endpoint/cursor.rs"

check_absent "\\bpub\\s+eff_index\\s*:" \
  "BranchMeta public eff_index field" \
  "${ROOT_DIR}/src/endpoint/cursor.rs"

check_absent "\\bpub\\s+fn\\s+(instance|has_fin|lane)\\s*\\(" \
  "RouteBranch binding/lane accessor" \
  "${ROOT_DIR}/src/endpoint/cursor.rs"

check_absent "\\bpub\\s+(const\\s+)?fn\\s+(session_id|mint_config|lane|rendezvous_id|phase_index|lane_cursors|lane_cursor)\\s*\\(" \
  "CursorEndpoint coordinate accessor" \
  "${ROOT_DIR}/src/endpoint/cursor.rs"

check_absent "\\bpub\\s+(const\\s+)?fn\\s+(scope_regions|scope_atlas_view|scope_markers|control_markers|mint_config)\\s*\\(" \
  "RoleProgram metadata accessor" \
  "${ROOT_DIR}/src/global/role_program.rs"

check_absent "\\bpub\\s+const\\s+fn\\s+(route_chain|par_chain)\\s*\\(" \
  "public choreography builder helper" \
  "${ROOT_DIR}/src/global.rs" "${ROOT_DIR}/src/global/program.rs"

check_absent "\\bproject_ref\\b" \
  "legacy deep projection helper" \
  "${ROOT_DIR}/src" "${ROOT_DIR}/../hibana-quic/src"

check_absent "\\bpub\\s+struct\\s+LivenessPolicy\\b" \
  "public runtime liveness policy knob" \
  "${ROOT_DIR}/src/runtime/config.rs"

check_absent "\\bpub\\s+fn\\s+with_liveness_policy\\s*\\(" \
  "public runtime liveness-policy builder" \
  "${ROOT_DIR}/src/runtime/config.rs"

check_absent "\\bpub\\s+fn\\s+liveness_policy\\s*\\(" \
  "public runtime liveness-policy accessor" \
  "${ROOT_DIR}/src/runtime/config.rs"

check_absent "\\bpub\\s+fn\\s+enable_global_tap\\s*\\(" \
  "public runtime global-tap knob" \
  "${ROOT_DIR}/src/runtime/config.rs"

check_absent_multiline "pub\\s+fn\\s+run_code_session[^\\{]*\\brv_id\\s*:" \
  "multi-argument management code-session helper" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "^pub use crate::epf::vm::Slot;$" \
  "root Slot alias" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "^pub use crate::control::types::\\{Many, One\\};$" \
  "root One/Many alias" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "crate::epf::vm::Slot" \
  "raw Slot owner in public docs/spec" \
  "${ROOT_DIR}/README.md" \
  "${ROOT_DIR}/../api-sketch.md" \
  "${ROOT_DIR}/.github/allowlists/substrate-public-api.txt"

check_absent "crate::epf::verifier::Header" \
  "raw Header owner in public docs/spec" \
  "${ROOT_DIR}/README.md" \
  "${ROOT_DIR}/../api-sketch.md" \
  "${ROOT_DIR}/.github/allowlists/substrate-public-api.txt"

check_absent "pub use crate::control::types::\\{(Many, One|One, Many)\\};" \
  "raw One/Many owner in public docs/spec" \
  "${ROOT_DIR}/README.md" \
  "${ROOT_DIR}/../api-sketch.md" \
  "${ROOT_DIR}/.github/allowlists/substrate-public-api.txt"

check_absent "hibana::substrate::\\{One, Many\\}" \
  "root One/Many alias in public docs/spec" \
  "${ROOT_DIR}/README.md" \
  "${ROOT_DIR}/../api-sketch.md"

check_absent "pub slot:" \
  "legacy code-session slot field" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "pub code:" \
  "legacy code-session code field" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "pub fuel_max:" \
  "legacy code-session fuel field" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "pub mem_len:" \
  "legacy code-session mem_len field" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "pub command:" \
  "legacy code-session command field" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "Result<\\s*crate::control::cluster::core::DynamicResolution,\\s*\\(\\)>" \
  "unit-error resolver callback surface" \
  "${ROOT_DIR}/src/substrate.rs"

check_absent "Result<\\s*DynamicResolution,\\s*\\(\\)>" \
  "unit-error resolver callback docs" \
  "${ROOT_DIR}/src/control/cluster/core.rs" \
  "${ROOT_DIR}/../api-sketch.md" \
  "${ROOT_DIR}/README.md"

check_absent_multiline "pub\\s+((const|async|unsafe)\\s+)*fn\\s+[A-Za-z0-9_]+[^\\{]*\\b[A-Za-z_][A-Za-z0-9_]*\\s*:\\s*(bool|&str|String)\\b" \
  "bool/stringly public function argument" \
  "${ROOT_DIR}/src/lib.rs" \
  "${ROOT_DIR}/src/g.rs" \
  "${ROOT_DIR}/src/endpoint.rs" \
  "${ROOT_DIR}/src/substrate.rs" \
  "${ROOT_DIR}/src/binding.rs" \
  "${ROOT_DIR}/src/transport.rs" \
  "${ROOT_DIR}/src/transport/context.rs" \
  "${ROOT_DIR}/src/transport/wire.rs" \
  "${ROOT_DIR}/src/control/cap/mint.rs" \
  "${ROOT_DIR}/src/control/cluster/core.rs" \
  "${ROOT_DIR}/src/runtime/mgmt.rs" \
  "${ROOT_DIR}/src/runtime/config.rs"

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "public API allowlist check passed"
