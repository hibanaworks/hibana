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
fn proof_trust_audits_reject_tool_failures_before_running_proofs() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!(
        "hibana-proof-trust-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for directory in [
        "bin",
        ".github/scripts/lib",
        ".github/ci-tests",
        "target/ci-tools",
        "proofs/lean",
        "proofs/checks/runtime/rolled-route-ownership",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for file in [
        ".github/scripts/lib/proof_audit.sh",
        ".github/scripts/lib/lean_audit_tool.sh",
        ".github/scripts/lib/supplemental_proofs_tool.sh",
        ".github/ci-tests/supplemental_proofs.rs",
        ".github/ci-tests/check_lean_proof_guard.rs",
        ".github/ci-tests/lean_audit.rs",
        "proofs/checks/runtime/rolled-route-ownership/check.sh",
    ] {
        fs::copy(repo.join(file), root.join(file)).unwrap();
    }
    fs::copy(
        env!("CARGO_BIN_EXE_supplemental-proofs"),
        root.join("target/ci-tools/supplemental-proofs"),
    )
    .unwrap();
    for (name, executable) in [
        ("bash", "/bin/bash"),
        ("dirname", "/usr/bin/dirname"),
        ("mkdir", "/bin/mkdir"),
    ] {
        symlink(executable, root.join("bin").join(name)).unwrap();
    }
    for tool in ["lake", "lean"] {
        let path = root.join("bin").join(tool);
        fs::write(
            &path,
            "#!/bin/bash\nif [[ \"$*\" == --version ]]; then printf 'Lean (version 4.30.0)\\n'; exit 0; fi\nprintf '%s\\n' \"$*\" > \"$PROOF_AUDIT_MARKER\"\nexit 91\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let binary = root.join("target/ci-tools/check_lean_proof_guard");
    let marker = root.join("proof-ran");
    let source = root.join("proofs/checks/runtime/rolled-route-ownership/Fixture.lean");
    for case in ["missing", "error", "forbidden", "unreadable", "clean"] {
        let _ = fs::remove_file(&marker);
        let _ = fs::remove_file(&binary);
        fs::write(&source, "theorem fixture : True := by trivial\n").unwrap();
        if case != "missing" {
            fs::copy(env!("CARGO_BIN_EXE_check-lean-proof-guard"), &binary).unwrap();
        }
        match case {
            "error" => {
                fs::write(&binary, "#!/bin/bash\nexit 2\n").unwrap();
            }
            "forbidden" => {
                fs::write(&source, "private axiom hidden : False\n").unwrap();
            }
            "unreadable" => {
                fs::remove_file(&source).unwrap();
            }
            _ => {}
        }
        let output = Command::new("/bin/bash")
            .arg(root.join("proofs/checks/runtime/rolled-route-ownership/check.sh"))
            .arg(root.join("evidence"))
            .env("PATH", root.join("bin"))
            .env("PROOF_AUDIT_MARKER", &marker)
            .output()
            .unwrap();
        if case == "clean" {
            assert_eq!(
                output.status.code(),
                Some(91),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(marker.exists());
        } else {
            assert!(!output.status.success(), "accepted {case}");
            assert!(!marker.exists(), "ran proofs after failed audit: {case}");
        }
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn smt_runner_requires_exact_results_and_successful_solver() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"
set -euo pipefail
root="$1"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-smt-test.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/bin"
printf '; EXPECT: unsat\n(check-sat)\n' > "$scratch/model.smt2"
for case in correct wrong unknown extra empty failed stdout_error; do
  case "$case" in
    correct) result='unsat'; status=0 ;;
    wrong) result='sat'; status=0 ;;
    unknown) result='unknown'; status=0 ;;
    extra) result='unsat\nunsat'; status=0 ;;
    empty) result=''; status=0 ;;
    failed) result='unsat'; status=9 ;;
    stdout_error) result='(error "solver diagnostic")'; status=1 ;;
  esac
  cat > "$scratch/bin/z3" <<SOLVER
#!/bin/bash
printf '%b\n' '$result'
printf 'solver stderr\n' >&2
exit $status
SOLVER
  chmod +x "$scratch/bin/z3"
  if PATH="$scratch/bin:$PATH" bash "$root/.github/scripts/check_smt2.sh" "$scratch/model.smt2" > "$scratch/output" 2>&1; then
    [[ "$case" == correct ]] || { cat "$scratch/output"; exit 1; }
  else
    [[ "$case" != correct ]] || { cat "$scratch/output"; exit 1; }
    if [[ "$case" == failed || "$case" == stdout_error ]]; then
      rg -F "$scratch/model.smt2" "$scratch/output"
      rg -F "$result" "$scratch/output"
      rg -F 'solver stderr' "$scratch/output"
    fi
  fi
done
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("smt-runner-test")
        .arg(root)
        .output()
        .expect("run SMT solver failure regression");
    assert!(
        output.status.success(),
        "{}\n{}",
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
    let directory =
        std::env::temp_dir().join(format!("pressure-ps-records-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let ps = directory.join("ps");
    let marker = directory.join("sampled");
    for (table, delay, child_sleep, expected) in [
        ("", "", "10", 2),
        ("garbage", "", "10", 2),
        ("2147483000 0", "", "10", 2),
        ("2147483000 0 0 bash\n2147483000 0 0 bash", "", "10", 2),
        ("2147483000 0 0 bash", "", "10", 2),
        ("2147483000 0 0 bash", "sleep 0.15", "0.05", 0),
    ] {
        let _ = fs::remove_file(&marker);
        fs::write(
            &ps,
            format!("#!/bin/sh\n: > \"$PRESSURE_MARKER\"\n{delay}\nprintf '%s\\n' '{table}'\n"),
        )
        .unwrap();
        fs::set_permissions(&ps, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_compile-pressure"))
            .args(["guard", ".", "record-fixture", "sleep", child_sleep])
            .env(
                "PATH",
                format!("{}:{}", directory.display(), std::env::var("PATH").unwrap()),
            )
            .env("PRESSURE_MARKER", &marker)
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

#[test]
fn standalone_proof_logs_preserve_failed_tool_status_and_diagnostics() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("hibana-proof-logging-{}", std::process::id()));
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join(".github/scripts/lib")).unwrap();
    // Use the real Rust validator, avoiding a compiler or mutable shared cache.
    fs::copy(
        env!("CARGO_BIN_EXE_supplemental-proofs"),
        root.join("validator"),
    )
    .unwrap();
    fs::write(
        root.join(".github/scripts/lib/supplemental_proofs_tool.sh"),
        "#!/bin/bash\nexec \"$PROOF_TEST_ROOT/validator\" \"$@\"\n",
    )
    .unwrap();
    let lean = root.join("bin/lean");
    fs::write(&lean, "#!/bin/bash\nif [[ $1 == --version ]]; then echo 'Lean (version 4.30.0, test)'; exit 0; fi\nif [[ $PROOF_TEST_EXIT == solver ]]; then cat \"$PROOF_TEST_ROOT/lean-success\"; exit 0; fi\nprintf 'proof stdout diagnostic\\n'\nprintf 'proof stderr diagnostic\\n' >&2\nexit \"$PROOF_TEST_EXIT\"\n").unwrap();
    fs::set_permissions(&lean, fs::Permissions::from_mode(0o755)).unwrap();
    for family in [
        "recv-lane-census",
        "pending-events",
        "event-admission",
        "parallel-offer-ingress",
    ] {
        let relative = format!("proofs/checks/runtime/{family}/check.sh");
        fs::create_dir_all(root.join(format!("proofs/checks/runtime/{family}"))).unwrap();
        fs::copy(repo.join(&relative), root.join(&relative)).unwrap();
        let evidence = root.join(format!("evidence-{family}"));
        let run = |code: &str| {
            Command::new("bash")
                .arg(&relative)
                .arg(&evidence)
                .current_dir(&root)
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        root.join("bin").display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .env("PROOF_TEST_ROOT", &root)
                .env("PROOF_TEST_EXIT", code)
                .output()
                .unwrap()
        };
        let output = run("17");
        assert_eq!(output.status.code(), Some(17), "{family}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let log = fs::read_to_string(evidence.join("lean.log")).unwrap();
        for diagnostic in ["proof stdout diagnostic", "proof stderr diagnostic"] {
            assert!(stdout.contains(diagnostic), "{family}: {stdout}");
            assert!(log.contains(diagnostic), "{family}: {log}");
        }
        let tee = root.join("bin/tee");
        fs::write(&tee, "#!/bin/bash\ncat >/dev/null\nexit 19\n").unwrap();
        fs::set_permissions(&tee, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            run("0").status.code(),
            Some(19),
            "{family}: failed tee must reject"
        );
        fs::remove_file(tee).unwrap();
        let source = fs::read_to_string(repo.join(&relative)).unwrap();
        let lean_success = if let Some((_, rest)) = source.split_once("<<'EOF'\n") {
            rest.split_once("\nEOF").unwrap().0.to_owned() + "\n"
        } else if family == "parallel-offer-ingress" {
            (0..14)
                .map(|i| format!("'Fixture{i}' depends on axioms: [propext]\n"))
                .collect()
        } else {
            String::new()
        };
        fs::write(root.join("lean-success"), lean_success).unwrap();
        let z3 = root.join("bin/z3");
        fs::write(&z3, "#!/bin/bash\nprintf 'solver stdout diagnostic\\n'\nprintf 'solver stderr diagnostic\\n' >&2\nexit 23\n").unwrap();
        fs::set_permissions(&z3, fs::Permissions::from_mode(0o755)).unwrap();
        let output = run("solver");
        assert_eq!(output.status.code(), Some(23), "{family}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("solver stdout diagnostic"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("solver stderr diagnostic"));
        assert!(
            fs::read_to_string(evidence.join("z3.log"))
                .unwrap()
                .contains("solver stdout diagnostic")
        );
        fs::remove_file(z3).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}
