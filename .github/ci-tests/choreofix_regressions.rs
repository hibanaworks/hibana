//! Required consumer regressions, compiled beside ChoreoFix's original qualifier.
//! The source/evidence helpers are that qualifier's; the globals, localsides,
//! receivers and scripted WASI tests remain the pinned consumer's actual code.
use super::{Result, completed_tests, digest, execute, files, save, sources, unchanged};
use serde_json::json;
use std::{collections::BTreeMap, env, fs, path::PathBuf, process::Command};

const HOSTS: [&str; 3] = ["review", "codebase", "cicd"];
const CASES: [(&str, &str); 2] = [
    (
        "codebase",
        "codebase_boundary::three_disjoint_commits_are_admitted_before_any_response",
    ),
    (
        "cicd",
        "cicd_boundary::real_wasi_cicd_uses_resource_endpoints_and_keeps_shell_freedom",
    ),
];

pub fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [core, output] = args.as_slice() else {
        return Err("usage: core-regressions CORE_SOURCE NEW_EVIDENCE".into());
    };
    let root = env::current_dir()?.canonicalize()?;
    let core = PathBuf::from(core).canonicalize()?;
    let output = PathBuf::from(output);
    fs::create_dir(&output)?;
    let before = sources(&root)?;
    save(&output.join("source-before.json"), &json!(before))?;
    let mut core_before = BTreeMap::new();
    files(&core, &core.join("src"), &mut core_before)?;
    core_before.insert(
        PathBuf::from("Cargo.toml"),
        digest(&core.join("Cargo.toml"))?,
    );
    save(&output.join("core-source-before.json"), &json!(core_before))?;
    let metadata = Command::new("cargo")
        .args(["+stable", "metadata", "--locked", "--format-version", "1"])
        .output()?;
    if !metadata.status.success() {
        return Err("consumer dependency resolution failed".into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let selected: Vec<_> = metadata["packages"]
        .as_array()
        .ok_or("metadata packages")?
        .iter()
        .filter(|p| p["name"] == "hibana")
        .collect();
    if selected.len() != 1
        || selected[0]["manifest_path"].as_str() != core.join("Cargo.toml").to_str()
    {
        return Err("consumer regression must select only the candidate core".into());
    }
    save(&output.join("dependency-tree.json"), &metadata)?;
    let manifest = before
        .iter()
        .map(|(path, hash)| format!("{hash}  {}\n", path.display()))
        .collect::<String>();
    fs::write(output.join("runtime-source.sha256"), manifest)?;
    let mut build = Command::new("cargo");
    build.args(["+stable", "build", "--locked"]);
    for family in HOSTS {
        build.args(["--bin", &format!("{family}-wasi")]);
    }
    execute(&mut build, &output, "build-hosts")?;
    execute(
        Command::new("sh").arg("vendor/hibana-wash/scripts/build.sh"),
        &output,
        "build-wash",
    )?;
    unchanged(&root, &before)?;
    let mut host_binaries = BTreeMap::new();
    for family in HOSTS {
        host_binaries.insert(
            family,
            digest(&root.join("target/debug").join(format!("{family}-wasi")))?,
        );
    }
    save(&output.join("compiled-hosts.json"), &json!(host_binaries))?;
    let mut tests = BTreeMap::new();
    for (family, test) in CASES {
        let binary = root
            .join("target/debug")
            .join(format!("{family}-wasi"))
            .canonicalize()?;
        let receipt = output.join(format!("{family}-build-receipt.json"));
        save(
            &receipt,
            &json!({
                "binary_sha256":digest(&binary)?, "source_manifest":"runtime-source.sha256",
                "source_manifest_sha256":digest(&output.join("runtime-source.sha256"))?,
                "source_unchanged":true, "scored_benchmark":false
            }),
        )?;
        let log = execute(
            Command::new("cargo")
                .args([
                    "+stable",
                    "test",
                    "--locked",
                    "--release",
                    "--manifest-path",
                    "tools/runner/Cargo.toml",
                    "--bin",
                    "choreofix-experiments",
                    test,
                    "--",
                    "--ignored",
                    "--exact",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS", "true")
                .env("CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS", "true")
                .env("CHOREOFIX_EVIDENCE_ROOT", output.join("evidence"))
                .env(
                    format!("CHOREOFIX_{}_WASI_BINARY", family.to_uppercase()),
                    &binary,
                )
                .env(
                    format!("CHOREOFIX_{}_BUILD_RECEIPT", family.to_uppercase()),
                    &receipt,
                ),
            &output,
            family,
        )?;
        if completed_tests(&log)? != 1 {
            return Err(format!("{family}: expected exactly one complete regression").into());
        }
        tests.insert(family, test);
        unchanged(&root, &before)?;
    }
    let mut core_after = BTreeMap::new();
    files(&core, &core.join("src"), &mut core_after)?;
    core_after.insert(
        PathBuf::from("Cargo.toml"),
        digest(&core.join("Cargo.toml"))?,
    );
    if core_after != core_before {
        return Err("candidate core changed during consumer regressions".into());
    }
    save(&output.join("source-after.json"), &json!(sources(&root)?))?;
    save(
        &output.join("result.json"),
        &json!({
            "all_required_consumer_regressions_passed":true, "test_functions":tests,
            "compiled_hosts":HOSTS,
            "model_calls":0, "scored_runs":0, "production_source_unchanged":true,
            "test_profile":"release with debug assertions and overflow checks",
            "purpose":"candidate-core regression; not publication qualification of a consumer dependency override"
        }),
    )?;
    println!(
        "Required ChoreoFix global/localside/WASI regressions passed: {}",
        output.display()
    );
    Ok(())
}
