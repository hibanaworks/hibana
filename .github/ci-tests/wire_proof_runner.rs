//! Private Rust orchestration for wire proof replay; shell only launches this tool.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
type Result<T> = std::result::Result<T, String>;
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fresh_scratch() -> Result<Scratch> {
    let path = env::temp_dir().join(format!(
        "hibana-wire-proof-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(Scratch(path))
}
fn executable(name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.components().count() > 1 {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            env::current_dir().map_err(|e| e.to_string())?.join(path)
        };
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!("executable does not exist: {}", path.display()))
        };
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                env::current_dir().expect("current directory").join(path)
            }
        })
        .ok_or_else(|| format!("executable not found: {name}"))
}
fn checked_output(label: &str, output: Output, lean: bool) -> Result<String> {
    let stdout = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let stderr = String::from_utf8(output.stderr).map_err(|e| e.to_string())?;
    print!("{stdout}");
    eprint!("{stderr}");
    if !output.status.success() {
        return Err(format!("{label} failed: {}", output.status));
    }
    let combined = format!("{stdout}{stderr}");
    if lean && (combined.contains("error:") || combined.contains("sorryAx")) {
        return Err(format!("{label}: Lean error or sorry axiom"));
    }
    Ok(combined)
}
fn invoke(
    program: &Path,
    args: &[&str],
    cwd: &Path,
    lean_path: Option<&str>,
    lean: bool,
) -> Result<String> {
    let mut command = Command::new("timeout");
    command.arg("300").arg(program).args(args).current_dir(cwd);
    if let Some(path) = lean_path {
        command.env("LEAN_PATH", path);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot launch {}: {e}", program.display()))?;
    checked_output(&program.display().to_string(), output, lean)
}
fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 proof path")
}
fn compile(repo: &Path, build: &Path, name: &str, toolchain: &str) -> Result<PathBuf> {
    let binary = build.join(name);
    invoke(
        Path::new("rustc"),
        &[
            &format!("+{toolchain}"),
            "--edition=2024",
            "-O",
            text(&repo.join(format!(".github/ci-tests/{name}.rs"))),
            "-o",
            text(&binary),
        ],
        repo,
        None,
        false,
    )?;
    Ok(binary)
}
fn resolved_lean_path(base: &str, proofs: &Path, scratch: &Path) -> Result<String> {
    let base = base.trim_end_matches('\n');
    if base.contains('\n') || base.contains('\r') {
        return Err("unexpected multiline LEAN_PATH".into());
    }
    let mut paths = vec![scratch.to_owned()];
    if !base.is_empty() {
        for path in base.split(':') {
            let path = Path::new(path);
            paths.push(if path.is_absolute() {
                path.to_owned()
            } else {
                proofs.join(path)
            });
        }
    }
    env::join_paths(paths)
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|_| "non-UTF-8 LEAN_PATH".into())
}
fn run(args: &[String]) -> Result<()> {
    let mut args = args.iter();
    let mut repo = PathBuf::from(
        args.next()
            .ok_or("usage: wire-proof-runner REPO [--lean LEAN] [--repo REPO]")?,
    );
    let mut lean = env::var("LEAN").unwrap_or_else(|_| "lean".into());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lean" => lean = args.next().ok_or("--lean requires executable")?.clone(),
            "--repo" => repo = PathBuf::from(args.next().ok_or("--repo requires directory")?),
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    let repo = fs::canonicalize(repo).map_err(|e| e.to_string())?;
    let here = repo.join("proofs/wire-frame-refinement");
    let proofs = repo.join("proofs/lean");
    let lean = executable(&lean)?;
    let lake = lean
        .parent()
        .ok_or("Lean executable has no parent")?
        .join("lake");
    let version = invoke(&lean, &["--version"], &repo, None, false)?;
    if !version.contains("version 4.30.0") {
        return Err("Lean 4.30.0 required".into());
    }
    executable("z3")?;
    executable("gzip")?;
    let scratch = fresh_scratch()?;
    let build = &scratch.0;
    let toolchain = env::var("TOOLCHAIN").unwrap_or_else(|_| "1.95.0".into());
    let audit = compile(&repo, build, "wire_audit", &toolchain)?;
    let model = compile(&repo, build, "wire_model", &toolchain)?;
    let smt = compile(&repo, build, "smt_model", &toolchain)?;
    let audit_source = build.join("Audit.lean");
    invoke(
        &audit,
        &["emit", text(&repo), text(&audit_source)],
        &repo,
        None,
        false,
    )?;
    invoke(&lake, &["build"], &proofs, None, false)?;
    // Capture only stdout for the path query; stderr is diagnostic, never a path.
    let path_output = Command::new("timeout")
        .arg("300")
        .arg(&lake)
        .args(["env", "printenv", "LEAN_PATH"])
        .current_dir(&proofs)
        .output()
        .map_err(|e| e.to_string())?;
    if !path_output.status.success() {
        return Err(format!("lake env failed: {}", path_output.status));
    }
    eprint!("{}", String::from_utf8_lossy(&path_output.stderr));
    let base = String::from_utf8(path_output.stdout).map_err(|e| e.to_string())?;
    let resolved = resolved_lean_path(&base, &proofs, build)?;
    for module in [
        "OptionMatchEquivalence",
        "WireFrameRefinement",
        "WireSourceBridge",
    ] {
        let output = invoke(
            &lean,
            &[
                "-o",
                text(&build.join(format!("{module}.olean"))),
                text(&here.join(format!("{module}.lean"))),
            ],
            &repo,
            Some(&resolved),
            true,
        )?;
        fs::write(build.join(format!("{module}.log")), output).map_err(|e| e.to_string())?;
    }
    let output = invoke(&lean, &[text(&audit_source)], &repo, Some(&resolved), true)?;
    let audit_log = build.join("audit.log");
    fs::write(&audit_log, output).map_err(|e| e.to_string())?;
    invoke(
        &audit,
        &["verify", text(&repo), text(&audit_log)],
        &repo,
        Some(&resolved),
        false,
    )?;
    for file in [
        "Correspondence.smt2",
        "explicit-match/OptionMatch.smt2",
        "ExternalAllocator.smt2",
    ] {
        invoke(
            &smt,
            &[text(&here.join(file))],
            &repo,
            Some(&resolved),
            false,
        )?;
    }
    invoke(&model, &[text(&repo)], &repo, Some(&resolved), false)?;
    println!(
        "PASS wire-frame refinement: 54 Lean theorems; unchanged exact admission; Z3 and finite model correspondence"
    );
    println!("LIMIT: concrete runtime Covers/SameClassUnique remains an explicit premise");
    Ok(())
}
fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
    #[test]
    fn proof_process_failures_cannot_pass() {
        assert!(checked_output("test", output(0, b"theorem passed\n", b""), true).is_ok());
        for failed in [
            output(1, b"", b""),
            output(124, b"", b""),
            output(0, b"sorryAx", b""),
            output(0, b"", b"error: broken"),
            output(0, &[255], b""),
        ] {
            assert!(checked_output("test", failed, true).is_err());
        }
    }
    #[test]
    fn lean_search_paths_resolve_from_proof_root() {
        assert_eq!(
            resolved_lean_path(
                ".lake/build/lib/lean:/tool/lean\n",
                Path::new("/repo/proofs/lean"),
                Path::new("/scratch")
            )
            .unwrap(),
            "/scratch:/repo/proofs/lean/.lake/build/lib/lean:/tool/lean"
        );
        assert_eq!(
            resolved_lean_path("", Path::new("/repo"), Path::new("/scratch")).unwrap(),
            "/scratch"
        );
        assert!(
            resolved_lean_path("first\nsecond", Path::new("/repo"), Path::new("/scratch")).is_err()
        );
    }
    #[test]
    fn bad_cli_fails_before_any_proof_action() {
        for args in [
            vec![],
            vec!["/".into(), "--lean".into()],
            vec!["/".into(), "--repo".into()],
            vec!["/".into(), "--unknown".into()],
        ] {
            assert!(run(&args).is_err());
        }
    }
    #[test]
    fn temporary_output_is_removed() {
        let path;
        {
            let scratch = fresh_scratch().unwrap();
            path = scratch.0.clone();
            fs::write(path.join("test"), "proof").unwrap();
        }
        assert!(!path.exists());
    }
}
