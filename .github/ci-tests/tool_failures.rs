use std::{path::PathBuf, process::Command};

#[test]
fn compile_pressure_process_table_bypasses_exec_environment_limits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guard = root.join(".github/scripts/lib/compile_pressure_guard.sh");
    let script = r#"
set -euo pipefail
source "$1"
ps() {
  case "$*" in
    '-axo pid=,ppid=,rss=,command=')
      printf '%d 0 1 bash\n' "$$"
      printf '999999 %d 1024 rustc --crate-name hibana ' "$$"
      head -c 300000 /dev/zero | tr '\0' x
      printf '\n'
      ;;
    '-axo pid=,ppid=')
      printf '%d 0\n999999 %d\n' "$$" "$$"
      ;;
    *)
      return 2
      ;;
  esac
}
set +e
scan="$(compile_pressure_guard_offender "$$" 1048576)"
status="$?"
set -e
[[ "${status}" -eq 1 ]]
[[ "${scan}" == "ok total_rss_mib=1 matched=1" ]]
[[ "$(compile_pressure_guard_descendants "$$")" == "999999" ]]
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("compile-pressure-process-table-test")
        .arg(guard)
        .output()
        .expect("run compile pressure process-table regression");
    assert!(
        output.status.success(),
        "compile pressure process-table regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn proof_trust_audits_reject_tool_failures_before_running_proofs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"
set -euo pipefail
root="$1"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-lean-audit.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/bin"
for tool in dirname mkdir; do
  ln -s "$(command -v "$tool")" "$scratch/bin/$tool"
done
export PROOF_AUDIT_MARKER="$scratch/proof-ran"
for tool in lake lean python3; do
  cat > "$scratch/bin/$tool" <<'SH'
#!/bin/bash
if [[ "$*" == 'env lean --version' ]]; then
  printf 'Lean (version 4.30.0)\n'
  exit 0
fi
printf '%s\n' "$*" > "$PROOF_AUDIT_MARKER"
exit 91
SH
  chmod +x "$scratch/bin/$tool"
done
for gate in proofs/rolled-route-ownership/check.sh; do
  for audit in missing error forbidden clean; do
    rm -f "$scratch/bin/rg" "$PROOF_AUDIT_MARKER"
    case "$audit" in
      missing) ;;
      error) printf '#!/bin/bash\nexit 2\n' > "$scratch/bin/rg" ;;
      forbidden) printf '#!/bin/bash\nprintf "forbidden proof fixture\\n"\nexit 0\n' > "$scratch/bin/rg" ;;
      clean) printf '#!/bin/bash\nexit 1\n' > "$scratch/bin/rg" ;;
    esac
    if [[ -f "$scratch/bin/rg" ]]; then chmod +x "$scratch/bin/rg"; fi
    set +e
    PATH="$scratch/bin" "$BASH" "$root/$gate" "$scratch/evidence" > "$scratch/gate.log" 2>&1
    status="$?"
    set -e
    if [[ "$audit" == clean ]]; then
      if [[ "$status" != 91 || ! -f "$PROOF_AUDIT_MARKER" ]]; then
        cat "$scratch/gate.log" >&2
        printf 'clean audit did not reach the proof runner: %s\n' "$gate" >&2
        exit 1
      fi
    elif [[ "$status" == 0 || -f "$PROOF_AUDIT_MARKER" ]]; then
      cat "$scratch/gate.log" >&2
      printf 'proof runner passed a failed proof trust audit: %s %s\n' "$gate" "$audit" >&2
      exit 1
    fi
  done
done
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("lean-proof-trust-audit-test")
        .arg(root)
        .output()
        .expect("run Lean proof-trust-audit fault injection");
    assert!(
        output.status.success(),
        "Lean proof-trust-audit fault injection failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
