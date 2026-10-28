//! Replay checks for current source projection algorithms.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(program: &str, args: &[&str], cwd: &Path, build: &Path, lean: bool) -> String {
    let output = Command::new("timeout")
        .arg("300")
        .arg(program)
        .args(args)
        .current_dir(cwd)
        .env("LEAN_PATH", build)
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
    if lean {
        assert!(
            !stdout.contains("sorryAx")
                && !stderr.contains("sorryAx")
                && !stdout.contains("error:")
                && !stderr.contains("error:"),
            "Lean proof failed"
        );
    }
    stdout
}
fn smt(path: &Path, build: &Path) {
    let model = fs::read_to_string(path).expect("SMT source");
    let expected: Vec<_> = model
        .lines()
        .filter_map(|l| l.strip_prefix("; EXPECT: "))
        .collect();
    assert!(!expected.is_empty() && expected.iter().all(|s| *s == "sat" || *s == "unsat"));
    let actual = run(
        "z3",
        &["-T:300", path.to_str().unwrap()],
        path.parent().unwrap(),
        build,
        false,
    );
    assert_eq!(
        actual.lines().collect::<Vec<_>>(),
        expected,
        "unknown or mismatched SMT results"
    );
}
fn main() {
    let mut args = env::args().skip(1);
    let repo = PathBuf::from(
        args.next()
            .expect("usage: projection-checks REPO [--lean LEAN]"),
    );
    let mut lean = env::var("LEAN").unwrap_or_else(|_| "lean".into());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lean" => lean = args.next().expect("--lean requires path"),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    let temp = env::temp_dir().join(format!(
        "hibana-projection-checks-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&temp).expect("create fresh build directory");
    let scratch = Scratch(temp);
    let build = &scratch.0;
    let version = run(&lean, &["--version"], &repo, build, false);
    assert!(version.contains("version 4.30.0"), "Lean 4.30.0 required");
    for (directory, leans, models) in [
        (
            "compiler-participant-mask",
            vec!["ParticipantMask"],
            vec!["ParticipantMask", "ActorParticipants"],
        ),
        (
            "passive-child-window",
            vec!["PassiveChildWindow"],
            vec!["PassiveChildWindow"],
        ),
        (
            "projection-conflict-reuse",
            vec!["ConflictReuse"],
            vec!["ConflictReuse"],
        ),
    ] {
        let root = repo.join("proofs/checks/projection").join(directory);
        let replay = build.join(directory);
        fs::create_dir(&replay).unwrap();
        for module in leans {
            run(
                &lean,
                &[root.join(format!("{module}.lean")).to_str().unwrap()],
                &root,
                build,
                true,
            );
        }
        for model in models {
            let file = format!("{model}.smt2");
            fs::copy(root.join(&file), replay.join(&file)).unwrap();
            smt(&replay.join(file), build);
        }
    }
    for model in [
        "lane-allocation/RouteAlignment.smt2",
        "message-identity/MessageIdentity.smt2",
    ] {
        smt(&repo.join("proofs/checks/projection").join(model), build);
    }
    println!(
        "PASS projection algorithm checks: participants, scope windows, conflict reuse, lanes and message identity"
    );
}
