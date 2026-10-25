//! Private Kani process orchestration; the shell entrypoint only launches this.
use std::{
    env,
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Audit, kani_json, walk};

#[path = "kani_recursion.rs"]
mod recursion;

struct Scratch(PathBuf);
impl Scratch {
    fn create() -> Audit<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "hibana-kani-inventory-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|e| format!("create Kani temporary directory: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cargo(root: &Path, target: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .arg("kani")
        .arg("--manifest-path")
        .arg(root.join("proofs/kani/Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .env("RUSTFLAGS", "-D warnings")
        .env("CARGO_BUILD_JOBS", "1");
    command
}

struct Prover {
    driver: PathBuf,
    cbmc: PathBuf,
    path: OsString,
}

impl Prover {
    fn prepare(root: &Path, scratch: &Path, version: &str) -> Audit<Self> {
        // Kani's installer uses KANI_HOME/kani-VERSION, or HOME/.kani/kani-VERSION.
        let base = match env::var_os("KANI_HOME") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(env::var_os("HOME").ok_or("missing HOME")?).join(".kani"),
        };
        let bin = base
            .join(format!("kani-{version}/bin"))
            .canonicalize()
            .map_err(|e| format!("Kani installation: {e}"))?;
        let driver = bin
            .join("kani-driver")
            .canonicalize()
            .map_err(|e| format!("Kani driver: {e}"))?;
        let cbmc = bin
            .join("cbmc")
            .canonicalize()
            .map_err(|e| format!("Kani CBMC: {e}"))?;
        let actual = Command::new(&driver)
            .args(["kani", "--version"])
            .output()
            .map_err(|e| format!("Kani driver version: {e}"))?;
        if !actual.status.success() || actual.stdout != format!("cargo-kani {version}\n").as_bytes()
        {
            return Err("bundled Kani driver version differs from the pinned version".into());
        }
        let build = Command::new("cargo")
            .args([
                "+stable",
                "build",
                "--locked",
                "-p",
                "hibana-ci-tests",
                "--bin",
                "kani-cbmc",
            ])
            .current_dir(root)
            .arg("--target-dir")
            .arg(root.join("target"))
            .env("CARGO_BUILD_JOBS", "1")
            .env("RUSTFLAGS", "-D warnings")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .map_err(|e| format!("build CBMC diagnostic stream: {e}"))?;
        if !build.success() {
            return Err(format!("CBMC diagnostic stream build failed: {build}"));
        }
        let normalizer = root
            .join("target/debug/kani-cbmc")
            .canonicalize()
            .map_err(|e| format!("CBMC diagnostic stream: {e}"))?;
        let shim = scratch.join("cbmc-bin");
        fs::create_dir(&shim).map_err(|e| format!("CBMC diagnostic directory: {e}"))?;
        std::os::unix::fs::symlink(normalizer, shim.join("cbmc"))
            .map_err(|e| format!("CBMC diagnostic command: {e}"))?;
        let inherited = env::var_os("PATH").ok_or("missing PATH")?;
        let path = env::join_paths([shim, bin].into_iter().chain(env::split_paths(&inherited)))
            .map_err(|e| format!("CBMC command PATH: {e}"))?;
        Ok(Self { driver, cbmc, path })
    }

    fn command(&self, manifest: &Path, target: &Path) -> Command {
        let mut command = Command::new(&self.driver);
        command
            .arg("kani")
            .arg("--manifest-path")
            .arg(manifest)
            .arg("--target-dir")
            .arg(target)
            .env("RUSTFLAGS", "-D warnings")
            .env("CARGO_BUILD_JOBS", "1")
            .env("PATH", &self.path)
            .env("HIBANA_REAL_CBMC", &self.cbmc);
        command
    }

    fn check_controls(&self, root: &Path, scratch: &Path) -> Audit<()> {
        let manifest = root.join(".github/ci-tests/fixtures/kani-diagnostics/Cargo.toml");
        let target = scratch.join("diagnostic-controls-target");
        for (harness, reason) in [
            (
                "real_assertion_failure",
                "Failed Checks: \"HIBANA_REAL_ASSERTION_CONTROL\"",
            ),
            (
                "real_memory_failure",
                "Failed Checks: dereference failure: pointer outside object bounds",
            ),
            (
                "real_unwind_failure",
                "Failed Checks: unwinding assertion loop",
            ),
        ] {
            let output = self
                .command(&manifest, &target)
                .args([
                    "-Z",
                    "unstable-options",
                    "--run-sanity-checks",
                    "--jobs",
                    "1",
                    "--output-format",
                    "terse",
                    "--exact",
                    "--harness",
                    harness,
                ])
                .output()
                .map_err(|e| format!("Kani diagnostic control {harness}: {e}"))?;
            let stdout = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
            let stderr = String::from_utf8(output.stderr).map_err(|e| e.to_string())?;
            let expected = "Complete - 0 successfully verified harnesses, 1 failures, 1 total.";
            if output.status.success()
                || !stdout.contains("VERIFICATION:- FAILED")
                || stdout
                    .lines()
                    .filter(|line| line.starts_with("Complete - "))
                    .collect::<Vec<_>>()
                    != [expected]
                || !stdout.contains(reason)
                || !stderr.contains("CBMC JSON: properties=")
            {
                return Err(format!(
                    "Kani diagnostic control {harness} did not preserve its intended failure:\n{stdout}\n{stderr}"
                ));
            }
            println!("Kani diagnostic control {harness}: intended failure preserved");
        }
        Ok(())
    }
}

pub fn summary(text: &str, expected: u64) -> Audit<()> {
    let matches: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("Complete - ")?;
            let (verified, tail) =
                line.split_once(" successfully verified harnesses, 0 failures, ")?;
            let reported = tail.strip_suffix(" total.")?;
            if [verified, reported]
                .iter()
                .all(|value| !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()))
            {
                Some((verified, reported))
            } else {
                None
            }
        })
        .collect();
    if matches.len() != 1 {
        return Err("Kani gate requires exactly one successful complete-harness summary".into());
    }
    let expected = expected.to_string();
    if matches[0] != (expected.as_str(), expected.as_str()) || expected == "0" {
        return Err(format!(
            "Kani gate requires the complete {expected}-harness inventory"
        ));
    }
    Ok(())
}

fn copy_log(mut input: impl Read, log: Arc<Mutex<File>>) -> Audit<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(());
        }
        let mut file = log.lock().map_err(|e| e.to_string())?;
        file.write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
    }
}

pub fn run(root: &Path) -> Audit<()> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let expected =
        fs::read_to_string(root.join(".github/kani-version")).map_err(|e| e.to_string())?;
    let expected = expected.trim_end_matches('\n');
    let version = Command::new("cargo")
        .args(["kani", "--version"])
        .output()
        .map_err(|e| format!("Kani version probe: {e}"))?;
    std::io::stderr()
        .write_all(&version.stderr)
        .map_err(|e| e.to_string())?;
    if !version.status.success() {
        return Err(format!("Kani version probe failed: {}", version.status));
    }
    let actual = String::from_utf8(version.stdout).map_err(|e| e.to_string())?;
    if actual.trim_end_matches('\n') != format!("cargo-kani {expected}") {
        return Err(format!(
            "Kani gate requires cargo-kani {expected}, found: {}",
            actual.trim_end()
        ));
    }
    walk(&root.join("src"))?;
    let scratch = Scratch::create()?;
    let prover = Prover::prepare(&root, &scratch.0, expected)?;
    let target = scratch.0.join("target");
    let status = prover
        .command(&root.join("proofs/kani/Cargo.toml"), &target)
        .args(["list", "--format", "json"])
        .current_dir(&scratch.0)
        .status()
        .map_err(|e| format!("Kani inventory process: {e}"))?;
    if !status.success() {
        return Err(format!("Kani inventory process failed: {status}"));
    }
    let inventory = fs::read_to_string(scratch.0.join("kani-list.json")).map_err(|e| {
        format!("Kani gate did not produce a readable structured harness inventory: {e}")
    })?;
    let total = kani_json::inventory_total(&inventory, expected)?;
    let mut groups = recursion::prepare(&root, &target, &inventory, total)?;
    prover.check_controls(&root, &scratch.0)?;
    groups.sort_by_key(|group| group.recursion.is_none());
    for (index, group) in groups.into_iter().enumerate() {
        println!(
            "Kani proof group={} harnesses={} recursion_bound={}",
            group.label,
            group.harnesses.len(),
            if group.recursion.is_some() {
                "per-function"
            } else {
                "default"
            }
        );
        let log_path = scratch.0.join(format!("verification-{index}.log"));
        let log = Arc::new(Mutex::new(
            File::create(&log_path).map_err(|e| e.to_string())?,
        ));
        let mut command = prover.command(&root.join("proofs/kani/Cargo.toml"), &target);
        command.args([
            "-Z",
            "unstable-options",
            "--run-sanity-checks",
            "--jobs",
            "1",
            "--output-format",
            "terse",
            "--exact",
        ]);
        for harness in &group.harnesses {
            command.args(["--harness", harness]);
        }
        if let Some(bounds) = &group.recursion {
            command.args(["--cbmc-args", "--unwindset", bounds]);
        } else if group.label == "parallel-reentry" {
            // Exhaust the same paths without retaining a merged equation for
            // all descriptor lookups. Refine array/arithmetic constraints
            // instead of eagerly expanding them into one large SAT formula.
            // Keep every assertion, input and unwind check.
            command.args([
                "--cbmc-args",
                "--paths",
                "lifo",
                "--refine",
                "--verbosity",
                "6",
            ]);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Kani verification process: {e}"))?;
        let stdout = child.stdout.take().ok_or("missing verification stdout")?;
        let stderr = child.stderr.take().ok_or("missing verification stderr")?;
        let out_log = Arc::clone(&log);
        let output = thread::spawn(move || copy_log(stdout, out_log));
        let errors = thread::spawn(move || copy_log(stderr, log));
        let status = child.wait().map_err(|e| e.to_string())?;
        let out_result = output.join().map_err(|_| "Kani stdout reader panicked")?;
        let err_result = errors.join().map_err(|_| "Kani stderr reader panicked")?;
        out_result?;
        err_result?;
        if !status.success() {
            return Err(format!("Kani verification process failed: {status}"));
        }
        summary(
            &fs::read_to_string(&log_path).map_err(|e| e.to_string())?,
            group.harnesses.len() as u64,
        )?;
    }
    println!("Kani gate passed version={expected} harnesses={total} backend=CBMC");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_summary_is_required_exactly_once() {
        let good = "Complete - 123 successfully verified harnesses, 0 failures, 123 total.\n";
        assert!(summary(good, 123).is_ok());
        for mutant in [
            String::new(),
            good.repeat(2),
            good.replace("123", "122"),
            good.replace("0 failures", "1 failures"),
            good.replace("123 total", "124 total"),
            good.replace("123", "0123"),
            good.replace("Complete", "INCOMPLETE"),
            good.trim_end_matches('\n').to_owned() + " trailing",
        ] {
            assert!(summary(&mutant, 123).is_err(), "accepted {mutant}");
        }
    }
}
