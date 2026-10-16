use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hibana-thumb-codegen-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir_all(path.join("bin")).unwrap();
        fs::write(path.join("probe.o"), b"object-fixture").unwrap();
        Self(path)
    }
    fn executable(&self, name: &str, source: &str) {
        let path = self.0.join("bin").join(name);
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn nm(&self, text: &[u8], first: i32, second: i32) {
        fs::write(self.0.join("nm-output"), text).unwrap();
        self.executable("llvm-nm", &format!("#!/bin/bash\n/bin/cat '{}'/nm-output\nif [[ \"$1\" == '--undefined-only' ]]; then exit {first}; else exit {second}; fi\n", self.0.display()));
    }
    fn scan(&self, object: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_check-thumb-codegen"))
            .arg("--scan-tool")
            .arg(self.0.join("bin/llvm-nm"))
            .arg(object)
            .output()
            .unwrap()
    }
    fn gate(&self, objects: usize, cargo_status: i32) -> Output {
        Command::new(env!("CARGO_BIN_EXE_check-thumb-codegen"))
            .arg(&self.0)
            .env("PATH", self.0.join("bin"))
            .env("TOOLCHAIN", "fixture")
            .env("CARGO_TARGET_DIR", self.0.join("target"))
            .env("THUMB_OBJECTS", objects.to_string())
            .env("THUMB_CARGO_STATUS", cargo_status.to_string())
            .output()
            .unwrap()
    }
    fn build_tools(&self) {
        self.executable(
            "rustup",
            &format!(
                r#"#!/bin/bash
case "$*" in
  'target add --toolchain fixture thumbv6m-none-eabi') exit 0 ;;
  'run fixture rustc --print sysroot') printf '%s\n' '{root}/sysroot' ;;
  'run fixture rustc -vV') printf 'rustc fixture\nhost: x86_64-unknown-linux-gnu\n' ;;
  *) exit 27 ;;
esac
"#,
                root = self.0.display()
            ),
        );
        self.executable(
            "cargo",
            r#"#!/bin/bash
if [[ "$THUMB_CARGO_STATUS" != 0 ]]; then exit "$THUMB_CARGO_STATUS"; fi
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == --target-dir ]]; then target="$2"; break; fi
  shift
done
/bin/mkdir -p "$target/thumbv6m-none-eabi/release/deps"
for ((i=0; i < THUMB_OBJECTS; i++)); do
  printf object > "$target/thumbv6m-none-eabi/release/deps/hibana_frame_header_codegen-$i.o"
done
"#,
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_or_nonzero_symbol_tools_cannot_pass() {
    let f = Fixture::new();
    let object = f.0.join("probe.o");
    assert!(!f.scan(&object).status.success());
    for output in [
        b"".as_slice(),
        b"U memcpy\n".as_slice(),
        b"U __aeabi_lmul\n".as_slice(),
    ] {
        f.nm(output, 2, 19);
        assert!(!f.scan(&object).status.success());
    }
    f.nm(b"U memcpy\n", 2, 0);
    assert!(
        f.scan(&object).status.success(),
        "documented -u fallback must work"
    );
    assert!(!f.scan(&f.0.join("missing.o")).status.success());
    fs::write(&object, []).unwrap();
    assert!(!f.scan(&object).status.success());
}

#[test]
fn malformed_output_fails_but_clean_empty_output_is_valid() {
    let f = Fixture::new();
    let object = f.0.join("probe.o");
    for malformed in [
        b"tool failed silently\n".as_slice(),
        b"U\n".as_slice(),
        &[0xff],
        b"U __aeabi_lmul\n".as_slice(),
    ] {
        f.nm(malformed, 0, 0);
        assert!(!f.scan(&object).status.success());
    }
    for valid in [
        b"".as_slice(),
        b"U memcpy\n".as_slice(),
        b"w optional\nv optional_object\n".as_slice(),
    ] {
        f.nm(valid, 0, 0);
        assert!(f.scan(&object).status.success());
    }
}

#[test]
fn full_gate_requires_successful_build_and_one_object_before_scan() {
    let f = Fixture::new();
    f.build_tools();
    f.nm(b"U memcpy\n", 0, 0);
    let good = f.gate(1, 0);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    assert!(String::from_utf8_lossy(&good.stdout).contains("has no aeabi u64 helpers"));
    let probe =
        fs::read_to_string(f.0.join("target/thumbv6m-frame-header-codegen/harness/src/lib.rs"))
            .unwrap();
    assert!(probe.contains("FrameHeader::from_bytes([0, 0, 0, a, b, 3, 4, 5])"));
    for (objects, status) in [(0, 0), (2, 0), (1, 13)] {
        assert!(!f.gate(objects, status).status.success());
    }
    f.nm(b"U __aeabi_uldivmod\n", 0, 0);
    assert!(!f.gate(1, 0).status.success());
}

#[test]
fn failed_host_probe_cannot_masquerade_as_valid_metadata() {
    let f = Fixture::new();
    f.build_tools();
    f.nm(b"", 0, 0);
    f.executable(
        "rustup",
        "#!/bin/bash\nprintf 'host: x86_64-unknown-linux-gnu\\n'\nexit 21\n",
    );
    assert!(!f.gate(1, 0).status.success());
    fs::remove_file(f.0.join("bin/rustup")).unwrap();
    assert!(!f.gate(1, 0).status.success());
}
