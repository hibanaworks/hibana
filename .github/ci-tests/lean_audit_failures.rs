use std::{fs, path::Path, process::Command};

#[test]
fn lean_auditors_fail_closed_on_mutants_and_process_failures() {
    let root =
        std::env::temp_dir().join(format!("hibana-lean-audit-failures-{}", std::process::id()));
    fs::create_dir_all(root.join("bin")).unwrap();
    let generated = root.join("Minimal.lean");
    let snapshot = root.join("claims.txt");
    fs::write(&generated, "theorem fixture : True := by trivial\n").unwrap();
    fs::write(&snapshot, "fixture : True\n").unwrap();
    let good = "HIBANA_GENERATED_CLAIM_SURFACE_BEGIN\nfixture : True\nHIBANA_GENERATED_CLAIM_SURFACE_END\n'fixture' does not depend on any axioms\n";
    let binary = env!("CARGO_BIN_EXE_check-generated-lean-axioms");
    let run = || {
        Command::new(binary)
            .arg("--kernel")
            .arg(&generated)
            .arg(&root)
            .arg(&snapshot)
            .arg("1")
            .env("PATH", root.join("bin"))
            .output()
            .unwrap()
    };
    let install = |output: &str, status: i32| {
        fs::write(root.join("output"), output).unwrap();
        fs::write(root.join("bin/lake"),format!("#!/bin/bash\n/bin/cat >/dev/null\n/bin/cat '{}'\nprintf 'lean-stderr-evidence\\n' >&2\nexit {status}\n",root.join("output").display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("bin/lake"), fs::Permissions::from_mode(0o755)).unwrap();
    };
    install(good, 0);
    assert!(run().status.success());
    for (label, mutant) in [
        (
            "weakened-type",
            good.replace("fixture : True", "fixture : False"),
        ),
        (
            "missing-axiom-report",
            good.replace("'fixture' does not depend on any axioms\n", ""),
        ),
        (
            "custom-axiom",
            good.replace(
                "does not depend on any axioms",
                "depends on axioms: [Classical.choice]",
            ),
        ),
        (
            "native-contamination",
            good.replace(
                "does not depend on any axioms",
                "depends on axioms: [fixture._native.native_decide.ax_1]",
            ),
        ),
        (
            "duplicate-axiom",
            format!("{good}'fixture' does not depend on any axioms\n"),
        ),
        (
            "duplicate-header",
            good.replace("fixture : True\n", "fixture : True\nfixture : True\n"),
        ),
        (
            "duplicate-marker",
            format!("HIBANA_GENERATED_CLAIM_SURFACE_BEGIN\n{good}"),
        ),
        (
            "empty-axioms",
            good.replace("does not depend on any axioms", "depends on axioms: []"),
        ),
        ("wrong-name", good.replace("'fixture'", "'foreign'")),
        (
            "malformed-list",
            good.replace(
                "does not depend on any axioms",
                "depends on axioms: [propext",
            ),
        ),
    ] {
        install(&mutant, 0);
        assert!(!run().status.success(), "accepted {label}");
    }
    for status in [1, 17, 91] {
        install(good, status);
        let output = run();
        assert_eq!(output.status.code(), Some(status));
        assert!(String::from_utf8_lossy(&output.stdout).contains("fixture : True"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("lean-stderr-evidence"));
    }
    install(good, 0);
    for mutant in [
        "private axiom hidden : False\n",
        "example : True := by trivial\n",
        "run_cmd synthesizeClaim\n",
        "theorem extra : True := by trivial\n",
        "def quoted := \"unterminated\n",
        "/- unterminated",
    ] {
        fs::write(
            &generated,
            format!("theorem fixture : True := by trivial\n{mutant}"),
        )
        .unwrap();
        assert!(!run().status.success(), "accepted source mutant {mutant}");
    }
    fs::write(&generated,"theorem fixture : True := by trivial\n-- axiom hidden : False\n/- example : False /- nested -/ -/\ndef quoted : String := \"native_decide axiom example\"\n").unwrap();
    assert!(run().status.success());
    fs::remove_file(root.join("bin/lake")).unwrap();
    assert!(!run().status.success());
    // Both tools reject missing or malformed command-line inputs.
    for executable in [binary, env!("CARGO_BIN_EXE_check-lean-theorem-inventory")] {
        assert_eq!(
            Command::new(executable).output().unwrap().status.code(),
            Some(2)
        );
    }
    let _ = fs::remove_dir_all(Path::new(&root));
}

#[test]
fn static_inventory_rejects_hidden_claims_and_changed_closures() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "hibana-static-audit-failures-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("Hibana")).unwrap();
    let source = root.join("Hibana/Claims.lean");
    fs::write(&source, "theorem fixture : True := by trivial\n").unwrap();
    fs::write(
        root.join("Hibana/StaticProjectabilityExamples.lean"),
        "example : True := by native_decide\n".repeat(32),
    )
    .unwrap();
    let snapshot = root.join("claims.txt");
    fs::write(&snapshot, "Hibana.fixture : True\n").unwrap();
    let good = "HIBANA_STATIC_CLAIM_SURFACE_BEGIN\nHibana.fixture : True\nHIBANA_STATIC_CLAIM_SURFACE_END\nHIBANA_ENV_THEOREM=Hibana.fixture\n'Hibana.fixture' does not depend on any axioms\n";
    fs::write(
        root.join("bin/lake"),
        format!(
            "#!/bin/bash\n/bin/cat >/dev/null\n/bin/cat '{}'\n",
            root.join("output").display()
        ),
    )
    .unwrap();
    fs::set_permissions(root.join("bin/lake"), fs::Permissions::from_mode(0o755)).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_check-lean-theorem-inventory"))
            .arg("--static")
            .arg(&root)
            .arg(&snapshot)
            .args(["1", "0", "0", "1"])
            .env("PATH", root.join("bin"))
            .output()
            .unwrap()
    };
    fs::write(root.join("output"), good).unwrap();
    assert!(run().status.success());
    for mutant in [
        good.replace("Hibana.fixture : True", "Hibana.fixture : False"),
        good.replace("HIBANA_ENV_THEOREM=Hibana.fixture\n", ""),
        good.replace(
            "HIBANA_ENV_THEOREM=Hibana.fixture",
            "HIBANA_ENV_THEOREM=Hibana.fixture\nHIBANA_ENV_THEOREM=Hibana.hidden",
        ),
        good.replace(
            "does not depend on any axioms",
            "depends on axioms: [Classical.choice]",
        ),
        good.replace(
            "does not depend on any axioms",
            "depends on axioms: [propext]",
        ),
        good.replace(
            "does not depend on any axioms",
            "depends on axioms: [Quot.sound]",
        ),
        good.replace("'Hibana.fixture'", "'Hibana.foreign'"),
    ] {
        fs::write(root.join("output"), mutant).unwrap();
        assert!(!run().status.success());
    }
    fs::write(root.join("output"), good).unwrap();
    for extra in [
        "private axiom hidden : False",
        "macro generated : command",
        "run_tac trivial",
        "theorem other : True := by trivial",
        "example : True := by native_decide",
    ] {
        fs::write(
            &source,
            format!("theorem fixture : True := by trivial\n{extra}\n"),
        )
        .unwrap();
        assert!(!run().status.success(), "accepted {extra}");
    }
    fs::write(&source, "theorem fixture : True := by trivial\n").unwrap();
    fs::write(
        root.join("Hibana/StaticProjectabilityExamples.lean"),
        "example : True := by native_decide\n".repeat(31),
    )
    .unwrap();
    assert!(
        !run().status.success(),
        "accepted changed native regression inventory"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn proof_guard_rejects_version_artifact_and_source_failures() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "hibana-proof-guard-failures-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    let binary = env!("CARGO_BIN_EXE_check-lean-proof-guard");
    let version = || {
        Command::new(binary)
            .arg("toolchain")
            .arg(&root)
            .env("PATH", root.join("bin"))
            .output()
            .unwrap()
    };
    let toolchain = root.join("lean-toolchain");
    assert!(!version().status.success());
    fs::write(&toolchain, "leanprover/lean4:v4.30.0\n").unwrap();
    assert!(!version().status.success(), "accepted missing lake");
    let lake = root.join("bin/lake");
    for (text, status, accepted) in [
        ("Lean (version 4.30.0)", 0, true),
        ("Lean (version 4.29.0)", 0, false),
        ("Lean (version 4.30.0)", 91, false),
    ] {
        fs::write(
            &lake,
            format!("#!/bin/bash\nprintf '%s\\n' '{text}'\nexit {status}\n"),
        )
        .unwrap();
        fs::set_permissions(&lake, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(version().status.success(), accepted);
    }
    fs::write(&toolchain, "leanprover/lean4:v4.29.0\n").unwrap();
    assert!(!version().status.success());
    let artifact = root.join("Generated.lean");
    let check_artifact = || {
        Command::new(binary)
            .arg("artifact")
            .arg(&artifact)
            .arg("fixture")
            .output()
            .unwrap()
    };
    assert!(!check_artifact().status.success());
    fs::write(&artifact, "").unwrap();
    assert!(!check_artifact().status.success());
    fs::write(&artifact, "theorem fixture : True := by trivial\n").unwrap();
    assert!(check_artifact().status.success());
    let check_source = || {
        Command::new(binary)
            .args(["sources", "fixture"])
            .arg(&artifact)
            .output()
            .unwrap()
    };
    assert!(check_source().status.success());
    for source in [
        "private axiom hidden : False",
        "-- sorry in a comment",
        "def quoted : String := \"native_decide\"",
        "unsafe def foreign := 1",
        "theorem bad : False := by admit",
    ] {
        fs::write(&artifact, source).unwrap();
        assert!(!check_source().status.success(), "accepted {source}");
    }
    fs::remove_file(&artifact).unwrap();
    assert!(!check_source().status.success());
    let _ = fs::remove_dir_all(root);
}
