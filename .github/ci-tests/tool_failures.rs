use std::{path::PathBuf, process::Command};

#[test]
fn compile_pressure_process_table_bypasses_exec_environment_limits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guard = root.join(".github/scripts/lib/compile_pressure_guard.sh");
    let script = r#"
set -euo pipefail
source "$1"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
export PRESSURE_ROOT_PID="$$"
cat > "$scratch/ps" <<'PS'
#!/bin/bash
case "$*" in
  '-axo pid=,ppid=,rss=,command=')
    printf '%d 0 1 bash\n' "$PRESSURE_ROOT_PID"
    printf '999999 %d 1024 rustc --crate-name hibana ' "$PRESSURE_ROOT_PID"
    head -c 300000 /dev/zero | tr '\0' x
    printf '\n'
    ;;
  '-axo pid=,ppid=')
    printf '%d 0\n999999 %d\n' "$PRESSURE_ROOT_PID" "$PRESSURE_ROOT_PID"
    ;;
  *) exit 2 ;;
esac
PS
chmod +x "$scratch/ps"
export PATH="$scratch:$PATH"
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
fn pressure_guard_preserves_command_and_budget_failures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = env!("CARGO_BIN_EXE_compile-pressure");
    let invoke = |script: &str, rss: &str, seconds: &str| {
        Command::new(binary)
            .arg("guard")
            .arg(&root)
            .arg("fixture")
            .args(["bash", "-c", script])
            .env("HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB", rss)
            .env("HIBANA_COMPILE_PRESSURE_MAX_SECONDS", seconds)
            .env("HIBANA_COMPILE_PRESSURE_CRATE_NAME", "")
            .env("HIBANA_COMPILE_PRESSURE_POLL_SECONDS", "0.02")
            .output()
            .unwrap()
    };
    assert_eq!(invoke("exit 0", "100", "0").status.code(), Some(0));
    assert_eq!(invoke("exit 31", "100", "0").status.code(), Some(31));
    assert_eq!(invoke("kill -TERM $$", "100", "0").status.code(), Some(143));
    assert_eq!(invoke("exit 0", "invalid", "0").status.code(), Some(2));
    let rss = invoke("exec -a rustc bash -c 'sleep 10 & wait'", "1", "0");
    assert_eq!(
        rss.status.code(),
        Some(137),
        "{}",
        String::from_utf8_lossy(&rss.stderr)
    );
    assert!(String::from_utf8_lossy(&rss.stderr).contains("exceeded 1MiB RSS"));
    let time = invoke("sleep 10 & wait", "100", "1");
    assert_eq!(time.status.code(), Some(124));
    assert!(String::from_utf8_lossy(&time.stderr).contains("exceeded 1s elapsed time"));
}

#[test]
fn pressure_guard_rejects_process_table_subprocess_failure() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let directory =
        std::env::temp_dir().join(format!("pressure-ps-failure-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let ps = directory.join("ps");
    fs::write(&ps, "#!/bin/sh\nexit 77\n").unwrap();
    fs::set_permissions(&ps, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_compile-pressure"))
        .args(["guard", ".", "ps-failure", "sleep", "10"])
        .env(
            "PATH",
            format!("{}:{}", directory.display(), std::env::var("PATH").unwrap()),
        )
        .env("HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB", "100")
        .env("HIBANA_COMPILE_PRESSURE_MAX_SECONDS", "0")
        .output()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to read process table"));
}

#[test]
fn pressure_guard_rejects_successful_corrupt_tables_and_checks_exit_race() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let resolved = Command::new("sh")
        .args(["-c", "command -v ps"])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let real_ps = String::from_utf8(resolved.stdout).unwrap();
    let directory =
        std::env::temp_dir().join(format!("pressure-ps-records-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let ps = directory.join("ps");
    let marker = directory.join("sampled");
    for (table, exit_race, expected) in [
        ("", false, 2),
        ("garbage", false, 2),
        ("2147483000 0", false, 2),
        ("2147483000 0 0 bash\n2147483000 0 0 bash", false, 2),
        ("2147483000 0 0 bash", false, 2),
        ("2147483000 0 0 bash", true, 0),
    ] {
        let _ = fs::remove_file(&marker);
        fs::write(
            &ps,
            format!(
                r#"#!/bin/sh
case "$*" in
  '-axo pid=,ppid=') printf '2147483000 0\n'; exit 0 ;;
  '-axo pid=,ppid=,rss=,command=') ;;
  *) exit 77 ;;
esac
if [ "$PRESSURE_EXIT_RACE" = 1 ]; then
  guard_pid=$PPID
  sampler_pid=$$
  child_pid=$("$PRESSURE_REAL_PS" -axo pid=,ppid= | while read -r pid parent; do
    if [ "$parent" = "$guard_pid" ] && [ "$pid" != "$sampler_pid" ]; then
      printf '%s\n' "$pid"
    fi
  done)
  case "$child_pid" in ''|*[!0-9]*) exit 77 ;; esac
  : > "$PRESSURE_MARKER"
  attempts=0
  while state=$("$PRESSURE_REAL_PS" -p "$child_pid" -o stat= 2>/dev/null); do
    case "$state" in *Z*) break ;; esac
    attempts=$((attempts + 1))
    [ "$attempts" -lt 1000 ] || exit 77
    sleep 0.01
  done
else
  : > "$PRESSURE_MARKER"
fi
printf '%s\n' '{table}'
"#
            ),
        )
        .unwrap();
        fs::set_permissions(&ps, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_compile-pressure"));
        command.args(["guard", ".", "record-fixture"]);
        if exit_race {
            command.args([
                "bash",
                "-c",
                r#"for attempt in {1..1000}; do
  if [[ -f "$PRESSURE_MARKER" ]]; then exit 0; fi
  sleep 0.01
done
exit 99"#,
            ]);
        } else {
            command.args(["sleep", "10"]);
        }
        let output = command
            .env(
                "PATH",
                format!("{}:{}", directory.display(), std::env::var("PATH").unwrap()),
            )
            .env("PRESSURE_MARKER", &marker)
            .env("PRESSURE_REAL_PS", real_ps.trim())
            .env("PRESSURE_EXIT_RACE", if exit_race { "1" } else { "0" })
            .env("HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB", "100")
            .env("HIBANA_COMPILE_PRESSURE_MAX_SECONDS", "0")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "table {table:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(marker.exists(), "fixture must exercise the ps boundary");
    }
    fs::remove_dir_all(directory).unwrap();
}
