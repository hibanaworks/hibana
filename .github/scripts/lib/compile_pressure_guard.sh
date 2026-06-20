compile_pressure_guard_limit_kib() {
  local max_mib="${HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB:-}"
  if [[ -z "${max_mib}" ]]; then
    local budget_path
    local helper
    local budget_label
    budget_path="${HIBANA_COMPILE_PRESSURE_BUDGETS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/measurement_snapshots/hibana-compile-pressure-budget.tsv}"
    helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/compile_pressure_budget.py"
    budget_label="${HIBANA_COMPILE_PRESSURE_LABEL:-}"
    if [[ -n "${budget_label}" ]]; then
      max_mib="$(python3 "${helper}" limit "${budget_path}" "${budget_label}" rss_mib)"
    else
      max_mib="$(python3 "${helper}" max-rss "${budget_path}")"
    fi
  fi
  if ! [[ "${max_mib}" =~ ^[1-9][0-9]*$ ]]; then
    echo "compile pressure guard violation: invalid RSS limit MiB: ${max_mib}" >&2
    return 2
  fi
  echo "$((max_mib * 1024))"
}

compile_pressure_guard_limit_seconds() {
  local max_seconds="${HIBANA_COMPILE_PRESSURE_MAX_SECONDS:-}"
  if [[ -z "${max_seconds}" ]]; then
    local budget_label="${HIBANA_COMPILE_PRESSURE_LABEL:-}"
    if [[ -z "${budget_label}" ]]; then
      echo 0
      return 0
    fi
    local budget_path
    local helper
    budget_path="${HIBANA_COMPILE_PRESSURE_BUDGETS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/measurement_snapshots/hibana-compile-pressure-budget.tsv}"
    helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/compile_pressure_budget.py"
    max_seconds="$(python3 "${helper}" limit "${budget_path}" "${budget_label}" seconds)"
  fi
  if ! [[ "${max_seconds}" =~ ^[0-9]+$ ]]; then
    echo "compile pressure guard violation: invalid time limit seconds: ${max_seconds}" >&2
    return 2
  fi
  echo "${max_seconds}"
}

compile_pressure_guard_offender() {
  local root_pid="$1"
  local max_kib="$2"
  local crate_name="${HIBANA_COMPILE_PRESSURE_CRATE_NAME:-}"
  local process_rows
  if ! process_rows="$(ps -axo pid=,ppid=,rss=,command=)"; then
    echo "compile pressure guard violation: failed to read process table" >&2
    return 2
  fi
  local scan
  local status
  set +e
  scan="$(PROCESS_ROWS="${process_rows}" python3 - "${root_pid}" "${max_kib}" "${crate_name}" <<'PY'
import collections
import os
import sys

try:
    root = int(sys.argv[1])
    max_kib = int(sys.argv[2])
    crate_name = sys.argv[3]
    children: dict[int, list[int]] = collections.defaultdict(list)
    rows: dict[int, tuple[int, str]] = {}
    for line in os.environ["PROCESS_ROWS"].splitlines():
        parts = line.split(None, 3)
        if len(parts) != 4:
            continue
        pid, ppid, rss, command = parts
        pid_i = int(pid)
        ppid_i = int(ppid)
        children[ppid_i].append(pid_i)
        rows[pid_i] = (int(rss), command.strip())

    stack = [root]
    descendants = {root}
    while stack:
        current = stack.pop()
        for child in children.get(current, ()):
            if child not in descendants:
                descendants.add(child)
                stack.append(child)

    total_rss = 0
    peak: tuple[int, int, str] | None = None
    def rust_tool_name(command: str) -> str | None:
        tokens = command.split()
        if not tokens:
            return None
        name = os.path.basename(tokens[0])
        if name in {"cargo", "rustc", "rustdoc"}:
            return name
        if name == "rustup":
            for token in tokens[1:]:
                tool = os.path.basename(token)
                if tool in {"cargo", "rustc", "rustdoc"}:
                    return "rustup"
        return None

    def crate_arg_matches(command: str) -> bool:
        if not crate_name:
            return True
        tokens = command.split()
        for idx, token in enumerate(tokens):
            if token == "--crate-name" and idx + 1 < len(tokens):
                return tokens[idx + 1] == crate_name
            if token.startswith("--crate-name="):
                return token.split("=", 1)[1] == crate_name
        return False

    matched_process = False
    for pid in sorted(descendants):
        rss, command = rows.get(pid, (0, ""))
        tool_name = rust_tool_name(command)
        if tool_name is not None:
            if crate_name and not crate_arg_matches(command):
                continue
            matched_process = True
            total_rss += rss
            if peak is None or rss > peak[1]:
                peak = (pid, rss, tool_name)
            if rss > max_kib:
                print(
                    f"process pid={pid} rss_mib={(rss + 1023) // 1024} "
                    f"total_rss_mib={(total_rss + 1023) // 1024} comm={tool_name} matched=1"
                )
                sys.exit(0)
    if total_rss > max_kib:
        if peak is None:
            peak_text = "peak=none"
        else:
            pid, rss, comm = peak
            peak_text = f"peak_pid={pid} peak_rss_mib={(rss + 1023) // 1024} peak_comm={comm}"
        print(f"aggregate total_rss_mib={(total_rss + 1023) // 1024} {peak_text} matched=1")
        sys.exit(0)
    matched = 1 if matched_process else 0
    print(f"ok total_rss_mib={(total_rss + 1023) // 1024} matched={matched}")
    sys.exit(7)
except Exception as exc:
    print(f"compile pressure guard scan failed: {exc}", file=sys.stderr)
    sys.exit(2)
PY
)"
  status="$?"
  set -e
  if [[ "${status}" -eq 0 ]]; then
    printf '%s\n' "${scan}"
    return 0
  fi
  if [[ "${status}" -eq 7 ]]; then
    printf '%s\n' "${scan}"
    return 1
  fi
  printf '%s\n' "${scan}" >&2
  return 2
}

compile_pressure_guard_descendants() {
  local root_pid="$1"
  local process_rows
  if ! process_rows="$(ps -axo pid=,ppid=)"; then
    echo "compile pressure guard violation: failed to read process tree" >&2
    return 2
  fi
  PROCESS_ROWS="${process_rows}" python3 - "${root_pid}" <<'PY'
import collections
import os
import sys

root = int(sys.argv[1])
children: dict[int, list[int]] = collections.defaultdict(list)
for line in os.environ["PROCESS_ROWS"].splitlines():
    parts = line.split()
    if len(parts) != 2:
        continue
    pid, ppid = (int(parts[0]), int(parts[1]))
    children[ppid].append(pid)

stack = [root]
descendants = []
seen = set()
while stack:
    current = stack.pop()
    for child in children.get(current, ()):
        if child not in seen:
            seen.add(child)
            descendants.append(child)
            stack.append(child)

for pid in reversed(descendants):
    print(pid)
PY
}

compile_pressure_guard_stop_tree() {
  local root_pid="$1"
  local pids
  if ! pids="$(compile_pressure_guard_descendants "${root_pid}")"; then
    echo "compile pressure guard violation: failed to enumerate process tree for ${root_pid}" >&2
    set +e
    kill "${root_pid}" 2>/dev/null
    set -e
    return 2
  fi
  if [[ -n "${pids}" ]]; then
    # shellcheck disable=SC2086
    set +e
    kill ${pids} 2>/dev/null
    set -e
  fi
  set +e
  kill "${root_pid}" 2>/dev/null
  set -e
}

run_with_compile_pressure_guard() {
  local label="$1"
  shift
  local max_kib
  local max_seconds
  local offender
  local observed_mib
  local max_observed_mib
  local start_seconds
  local now_seconds
  local active_observed_seconds
  local active_window_start_seconds
  local elapsed_seconds
  local status
  local child
  max_kib="$(compile_pressure_guard_limit_kib)"
  max_seconds="$(compile_pressure_guard_limit_seconds)"
  max_observed_mib=0
  start_seconds="$(date +%s)"
  active_observed_seconds=0
  active_window_start_seconds=""
  "$@" &
  child="$!"
  while kill -0 "${child}" 2>/dev/null; do
    now_seconds="$(date +%s)"
    set +e
    offender="$(compile_pressure_guard_offender "${child}" "${max_kib}")"
    status="$?"
    set -e
    if [[ "${status}" -gt 1 ]]; then
      echo "compile pressure guard violation: ${label} RSS scan failed" >&2
      if ! compile_pressure_guard_stop_tree "${child}"; then
        return 2
      fi
      set +e
      wait "${child}" 2>/dev/null
      set -e
      return "${status}"
    fi
    if [[ "${offender}" =~ total_rss_mib=([0-9]+) ]]; then
      observed_mib="${BASH_REMATCH[1]}"
      if (( observed_mib > max_observed_mib )); then
        max_observed_mib="${observed_mib}"
      fi
    fi
    if [[ -n "${HIBANA_COMPILE_PRESSURE_CRATE_NAME:-}" ]]; then
      if [[ "${offender}" =~ matched=1 ]]; then
        if [[ -z "${active_window_start_seconds}" ]]; then
          active_window_start_seconds="${now_seconds}"
        fi
      elif [[ -n "${active_window_start_seconds}" ]]; then
        active_observed_seconds="$((active_observed_seconds + now_seconds - active_window_start_seconds))"
        active_window_start_seconds=""
      fi
      elapsed_seconds="${active_observed_seconds}"
      if [[ -n "${active_window_start_seconds}" ]]; then
        elapsed_seconds="$((elapsed_seconds + now_seconds - active_window_start_seconds))"
      fi
    else
      elapsed_seconds="$((now_seconds - start_seconds))"
    fi
    if [[ "${status}" -eq 0 && -n "${offender}" ]]; then
      echo "compile pressure guard violation: ${label} exceeded $((max_kib / 1024))MiB RSS: ${offender}" >&2
      if ! compile_pressure_guard_stop_tree "${child}"; then
        return 2
      fi
      set +e
      wait "${child}" 2>/dev/null
      set -e
      return 137
    fi
    if (( max_seconds > 0 && elapsed_seconds > max_seconds )); then
      echo "compile pressure guard violation: ${label} exceeded ${max_seconds}s elapsed time" >&2
      if ! compile_pressure_guard_stop_tree "${child}"; then
        return 2
      fi
      set +e
      wait "${child}" 2>/dev/null
      set -e
      return 124
    fi
    sleep "${HIBANA_COMPILE_PRESSURE_POLL_SECONDS:-1}"
  done
  set +e
  wait "${child}"
  status="$?"
  set -e
  now_seconds="$(date +%s)"
  if [[ -n "${HIBANA_COMPILE_PRESSURE_CRATE_NAME:-}" ]]; then
    elapsed_seconds="${active_observed_seconds}"
    if [[ -n "${active_window_start_seconds}" ]]; then
      elapsed_seconds="$((elapsed_seconds + now_seconds - active_window_start_seconds))"
    fi
  else
    elapsed_seconds="$((now_seconds - start_seconds))"
  fi
  echo "compile pressure observed: ${label} elapsed=${elapsed_seconds}s seconds_budget=${max_seconds}s max_rss=${max_observed_mib}MiB rss_budget=$((max_kib / 1024))MiB"
  return "${status}"
}
