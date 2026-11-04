//! Compile and audit every standalone implementation theorem.
mod lean_module;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let [repo] = args.as_slice() else {
        panic!("usage: implementation-proofs REPO");
    };
    let repo = PathBuf::from(repo);
    let toolchain = fs::read_to_string(repo.join("proofs/lean/lean-toolchain"))
        .expect("source-selected Lean toolchain");
    // Resolve once against the same pin as the library proofs. An ambient
    // `lean` shim or LEAN variable must not select another compiler.
    let selected = Command::new("elan")
        .args(["which", "lean"])
        .env("ELAN_TOOLCHAIN", toolchain.trim())
        .current_dir(&repo)
        .output()
        .expect("resolve source-selected Lean compiler");
    assert!(
        selected.status.success(),
        "cannot select {}: {}",
        toolchain.trim(),
        String::from_utf8_lossy(&selected.stderr)
    );
    let lean = String::from_utf8(selected.stdout).expect("UTF-8 compiler path");
    let lean = lean.trim();
    assert!(
        Path::new(lean).is_absolute(),
        "Lean compiler path is not absolute"
    );
    println!("Lean compiler: {} ({lean})", toolchain.trim());
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
        lean_module::check(lean, file, None).expect("compiled implementation theorem closures");
    }
    println!("PASS implementation proof modules={}", files.len());
}
