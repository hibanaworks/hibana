use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hibana-kani-mutants-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir_all(path.join("src")).unwrap();
        fs::create_dir_all(path.join(".github")).unwrap();
        fs::create_dir_all(path.join("bin")).unwrap();
        fs::create_dir_all(path.join("proofs/kani")).unwrap();
        fs::create_dir_all(path.join("kani-home/kani-0.67.0/bin")).unwrap();
        fs::create_dir_all(path.join("target/debug")).unwrap();
        fs::write(path.join("target/debug/kani-cbmc"), "fixture normalizer").unwrap();
        fs::write(path.join("kani-home/kani-0.67.0/bin/cbmc"), "fixture CBMC").unwrap();
        fs::write(path.join(".github/kani-version"), "0.67.0\n").unwrap();
        fs::write(path.join("proofs/kani/harness-inventory.json"), INVENTORY).unwrap();
        fs::write(
            path.join("src/fixture.rs"),
            "#[kani::should_panic] fn rejects() { production(checked()); }\n",
        )
        .unwrap();
        Self(path)
    }
    fn cargo(
        &self,
        version_status: i32,
        inventory: &str,
        list_status: i32,
        summary: &str,
        proof_status: i32,
    ) {
        fs::write(self.0.join("inventory.json"), inventory).unwrap();
        fs::write(self.0.join("summary.txt"), summary).unwrap();
        let script = format!(
            r#"#!/bin/bash
if [[ "$*" == 'kani --version' ]]; then
  printf 'cargo-kani 0.67.0\n'
  exit {version_status}
fi
if [[ "$*" == '+stable build --locked'* ]]; then
  exit 0
fi
if [[ "$*" == *'list --format json' ]]; then
  /bin/cat '{root}/inventory.json' > kani-list.json
  exit {list_status}
fi
if [[ "$*" == *'--harness real_'* ]]; then
  if [[ -f '{root}/control-pass' ]]; then
    printf 'Complete - 1 successfully verified harnesses, 0 failures, 1 total.\n'
    exit 0
  fi
  if [[ "$*" == *'--harness real_assertion_failure'* ]]; then
    printf 'Failed Checks: "HIBANA_REAL_ASSERTION_CONTROL"\n'
  elif [[ "$*" == *'--harness real_memory_failure'* ]]; then
    printf 'Failed Checks: dereference failure: pointer outside object bounds\n'
  else
    printf 'Failed Checks: unwinding assertion loop 0\n'
  fi
  printf 'VERIFICATION:- FAILED\nComplete - 0 successfully verified harnesses, 1 failures, 1 total.\n'
  printf 'CBMC JSON: properties=2 diagnostic-traces-omitted=1\n' >&2
  exit 1
fi
printf 'verification-started\n' > '{root}/proof-ran'
/bin/cat '{root}/summary.txt'
printf 'stderr-preserved\n' >&2
exit {proof_status}
"#,
            root = self.0.display()
        );
        fs::write(self.0.join("bin/cargo"), &script).unwrap();
        fs::write(self.0.join("kani-home/kani-0.67.0/bin/kani-driver"), script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(self.0.join("bin/cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(
            self.0.join("kani-home/kani-0.67.0/bin/kani-driver"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fn run(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_check-kani-source"))
            .arg("--gate")
            .arg(&self.0)
            .env("PATH", self.0.join("bin"))
            .env("KANI_HOME", self.0.join("kani-home"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const INVENTORY: &str = r#"{"kani-version":"0.67.0","standard-harnesses":{"fixture.rs":["a","b","c"]},"totals":{"standard-harnesses":3,"contract-harnesses":0}}"#;
const SUMMARY: &str = "Complete - 3 successfully verified harnesses, 0 failures, 3 total.\n";

#[test]
fn kani_gate_preserves_logs_and_checks_process_exit_status() {
    let f = Fixture::new();
    f.cargo(0, INVENTORY, 0, SUMMARY, 0);
    let result = f.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("stderr-preserved"));
    for (version, list, proof) in [(19, 0, 0), (0, 23, 0), (0, 0, 27)] {
        let _ = fs::remove_file(f.0.join("proof-ran"));
        f.cargo(version, INVENTORY, list, SUMMARY, proof);
        let result = f.run();
        assert!(
            !result.status.success(),
            "accepted failed process {version}/{list}/{proof}"
        );
        if version != 0 || list != 0 {
            assert!(!f.0.join("proof-ran").exists());
        } else {
            assert!(String::from_utf8_lossy(&result.stdout).contains("stderr-preserved"));
        }
    }
}

#[test]
fn inventory_mutants_never_start_verification() {
    let f = Fixture::new();
    for bad in [
        "".into(),
        "{".into(),
        "[]".into(),
        INVENTORY.replace("0.67.0", "0.68.0"),
        INVENTORY.replace(":3", ":0"),
        INVENTORY.replace(":3", ":true"),
        INVENTORY.replace(":3", ":3.5"),
        INVENTORY.replace(":0", ":1"),
        INVENTORY.replace("\"a\"", "\"unreviewed\""),
        format!("{INVENTORY}{INVENTORY}"),
    ] {
        f.cargo(0, &bad, 0, SUMMARY, 0);
        assert!(!f.run().status.success(), "accepted {bad}");
        assert!(!f.0.join("proof-ran").exists());
    }
}

#[test]
fn source_audit_mutants_and_missing_tools_fail_closed() {
    let f = Fixture::new();
    for source in [
        "#[kani::should_panic] fn bad() { panic!(); }",
        "#[kani::should_panic] fn bad() { kani::any::<u8>(); }",
        "#[kani::should_panic] fn bad() {",
        "fn bad() { kani::assume(true); }",
        "fn bad() { kani /* gap */ :: assume(true); }",
    ] {
        fs::write(f.0.join("src/fixture.rs"), source).unwrap();
        f.cargo(0, INVENTORY, 0, SUMMARY, 0);
        assert!(!f.run().status.success(), "accepted {source}");
        assert!(!f.0.join("proof-ran").exists());
    }
    fs::remove_file(f.0.join("bin/cargo")).unwrap();
    assert!(!f.run().status.success());
    fs::write(f.0.join("src/fixture.rs"), [0xff]).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_check-kani-source"))
        .arg(f.0.join("src"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    let result = Command::new(env!("CARGO_BIN_EXE_check-kani-source"))
        .arg(f.0.join("missing"))
        .output()
        .unwrap();
    assert!(!result.status.success());
}

#[test]
fn success_process_with_incomplete_or_duplicate_summary_fails() {
    let f = Fixture::new();
    for bad in [
        "".into(),
        SUMMARY.repeat(2),
        SUMMARY.replace("3 total", "4 total"),
        SUMMARY.replace("0 failures", "1 failures"),
        SUMMARY.replace('3', "2"),
    ] {
        f.cargo(0, INVENTORY, 0, &bad, 0);
        assert!(!f.run().status.success(), "accepted {bad}");
    }
}

#[test]
fn a_diagnostic_control_that_turns_green_blocks_verification() {
    let f = Fixture::new();
    f.cargo(0, INVENTORY, 0, SUMMARY, 0);
    fs::write(f.0.join("control-pass"), "").unwrap();
    let result = f.run();
    assert!(!result.status.success());
    assert!(!f.0.join("proof-ran").exists());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("did not preserve its intended failure")
    );
}
