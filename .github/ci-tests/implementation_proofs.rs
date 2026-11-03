//! Compile and audit every standalone implementation theorem.
mod lean_module;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn run(program: &str, args: &[&str], cwd: &Path) -> String {
    let output = Command::new("timeout")
        .arg("300")
        .arg(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("start proof process");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    print!("{stdout}");
    eprint!("{stderr}");
    assert!(
        output.status.success(),
        "{program} failed: {}",
        output.status
    );
    stdout
}
fn main() {
    let mut args = env::args().skip(1);
    let repo = PathBuf::from(
        args.next()
            .expect("usage: implementation-proofs REPO [--lean LEAN]"),
    );
    let mut lean = env::var("LEAN").unwrap_or_else(|_| "lean".into());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lean" => lean = args.next().expect("--lean requires path"),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    let version = run(&lean, &["--version"], &repo);
    assert!(version.contains("version 4.30.0"), "Lean 4.30.0 required");
    fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("implementation proof directory") {
            let path = entry.expect("proof directory entry").path();
            if path.is_dir() {
                if path.file_name().unwrap() != "target" && path.file_name().unwrap() != ".lake" {
                    collect(&path, files);
                }
            } else if path
                .extension()
                .is_some_and(|extension| extension == "lean")
                && path.file_name().unwrap() != "CheckCompletion.lean"
            {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    collect(&repo.join("proofs/checks"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no implementation proofs");
    for file in &files {
        println!("CHECK {}", file.strip_prefix(&repo).unwrap().display());
        lean_module::check(&lean, file, None).expect("compiled implementation theorem closures");
    }
    println!("PASS implementation proof modules={}", files.len());
}
