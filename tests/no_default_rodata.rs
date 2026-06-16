#![cfg(feature = "std")]

use std::{env, fs, path::Path, path::PathBuf, process::Command};

fn command_path(name: &str) -> Option<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8(output.stdout)
            .expect("command path utf8")
            .trim()
            .to_owned()
    })
}

fn write_probe(manifest_dir: &Path, probe_dir: &Path) {
    fs::create_dir_all(probe_dir.join("src")).expect("create probe src");
    fs::write(
        probe_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "hibana-no-default-rodata-probe"
version = "0.0.0"
edition = "2024"

[profile.release]
panic = "abort"
debug = false

[dependencies]
hibana = {{ path = "{}", default-features = false }}
"#,
            manifest_dir.display()
        ),
    )
    .expect("write probe Cargo.toml");
    fs::write(
        probe_dir.join("src/main.rs"),
        r#"#![no_std]
#![no_main]

use core::panic::PanicInfo;
use hibana::runtime::{
    resolver::ResolverError,
    transport::FrameHeader,
};

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let header = FrameHeader::from_bytes([0, 0, 0, 0, 0, 0, 0, 7]);
    let err = ResolverError::reject();
    let _ = header.bytes()[7] ^ (err.operation().as_bytes().len() as u8);
    loop {}
}
"#,
    )
    .expect("write probe main.rs");
}

#[test]
fn no_default_no_source_path_rodata() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = "thumbv6m-none-eabi";
    let installed_targets = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup target list");
    assert!(
        String::from_utf8(installed_targets.stdout)
            .expect("target list utf8")
            .lines()
            .any(|line| line == target),
        "{target} must be installed for no-default rodata guard"
    );

    let probe_dir = manifest_dir.join("target/no-default-rodata-probe");
    let target_dir = manifest_dir.join("target/no-default-rodata-target");
    let _ = fs::remove_dir_all(&probe_dir);
    let _ = fs::remove_dir_all(&target_dir);
    write_probe(&manifest_dir, &probe_dir);

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(&probe_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("CARGO_BUILD_JOBS", "1")
        .args([
            "build",
            "--release",
            "--target",
            target,
            "--no-default-features",
        ])
        .status()
        .expect("build no-default probe");
    assert!(status.success(), "no-default thumb probe must build");

    let strings = command_path("llvm-strings")
        .or_else(|| command_path("rust-llvm-strings"))
        .or_else(|| command_path("strings"))
        .expect("llvm-strings or strings must be available");
    let artifact = target_dir
        .join(target)
        .join("release")
        .join("hibana-no-default-rodata-probe");
    let output = Command::new(strings)
        .arg(&artifact)
        .output()
        .unwrap_or_else(|err| panic!("strings {} failed: {err}", artifact.display()));
    assert!(output.status.success(), "strings must read probe artifact");
    let rendered = String::from_utf8_lossy(&output.stdout);
    for forbidden in [
        manifest_dir.join("src").display().to_string(),
        "src/diag.rs".to_owned(),
        "src/endpoint/error.rs".to_owned(),
        "src/session/cluster/error.rs".to_owned(),
        "src/session/cluster/core/dynamic_resolvers.rs".to_owned(),
        "Location::caller".to_owned(),
        "core::panic::Location".to_owned(),
        "panic::Location".to_owned(),
    ] {
        assert!(
            !rendered.contains(&forbidden),
            "no-default thumb artifact must not retain source-location diagnostic string: {forbidden}"
        );
    }
}
