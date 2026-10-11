//! Private dependency-free thumbv6m FrameHeader codegen gate.
use std::{
    env, fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{self, Command, Output},
};

type Check<T> = Result<T, String>;
const TARGET: &str = "thumbv6m-none-eabi";
const HELPERS: &[&str] = &[
    "lmul", "lcmp", "ulcmp", "ldivmod", "uldivmod", "llsl", "llsr", "lasr",
];
const PROBE: &str = r#"#![no_std]
use hibana::runtime::transport::FrameHeader;
#[unsafe(no_mangle)]
pub extern "C" fn hibana_frame_header_codegen_probe(a: u8, b: u8) -> u8 {
    let header = FrameHeader::from_bytes([0, 0, 0, a, b, 3, 4, 5]);
    let bytes = header.bytes();
    bytes[3] ^ bytes[4] ^ bytes[5] ^ bytes[6] ^ bytes[7]
}
"#;

fn forbidden(text: &str) -> bool {
    HELPERS.iter().any(|suffix| {
        let helper = format!("__aeabi_{suffix}");
        text.match_indices(&helper).any(|(index, _)| {
            text[index + helper.len()..]
                .chars()
                .next()
                .is_none_or(|c| c != '_' && !c.is_alphanumeric())
        })
    })
}

fn scan(text: &str) -> Check<()> {
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if forbidden(line) {
            return Err(format!(
                "thumbv6m FrameHeader codegen regained aeabi u64 helper calls: {line}"
            ));
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let (kind, symbol) = match fields.as_slice() {
            [kind, symbol] => (*kind, *symbol),
            [address, kind, symbol] if address.bytes().all(|c| c.is_ascii_hexdigit()) => {
                (*kind, *symbol)
            }
            _ => return Err(format!("malformed undefined-symbol output: {line}")),
        };
        if !matches!(kind, "U" | "u" | "w" | "v") || symbol.chars().any(char::is_control) {
            return Err(format!("unexpected undefined-symbol output: {line}"));
        }
    }
    Ok(())
}

fn output(command: &mut Command, label: &str) -> Check<Output> {
    command.output().map_err(|e| format!("{label}: {e}"))
}
fn checked(command: &mut Command, label: &str) -> Check<String> {
    let result = output(command, label)?;
    std::io::stderr()
        .write_all(&result.stderr)
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!("{label} failed: {}", result.status));
    }
    String::from_utf8(result.stdout).map_err(|e| format!("{label} returned non-UTF8 output: {e}"))
}

fn scan_tool(tool: &Path, object: &Path) -> Check<()> {
    let metadata = fs::metadata(object).map_err(|e| format!("codegen object missing: {e}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("codegen object must be a nonempty regular file".into());
    }
    let first = output(
        Command::new(tool).arg("--undefined-only").arg(object),
        "undefined-symbol scan",
    )?;
    let result = if first.status.success() {
        first
    } else {
        let fallback = output(
            Command::new(tool).arg("-u").arg(object),
            "undefined-symbol scan fallback",
        )?;
        if !fallback.status.success() {
            std::io::stderr()
                .write_all(&first.stderr)
                .map_err(|e| e.to_string())?;
            std::io::stderr()
                .write_all(&fallback.stderr)
                .map_err(|e| e.to_string())?;
            return Err(format!(
                "undefined-symbol tool failed in both supported modes: {}, {}",
                first.status, fallback.status
            ));
        }
        fallback
    };
    std::io::stderr()
        .write_all(&result.stderr)
        .map_err(|e| e.to_string())?;
    let text =
        String::from_utf8(result.stdout).map_err(|e| format!("non-UTF8 symbol output: {e}"))?;
    scan(&text)
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn which(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|path| executable(path))
}
fn find_object(directory: &Path) -> Check<PathBuf> {
    let entries = fs::read_dir(directory).map_err(|e| format!("codegen objects missing: {e}"))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| {
            name.starts_with("hibana_frame_header_codegen-") && name.ends_with(".o")
        }) {
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err("codegen object is not a regular file".into());
            }
            found.push(entry.path());
        }
    }
    if found.len() != 1 {
        return Err(format!(
            "expected exactly one FrameHeader codegen object, found {}",
            found.len()
        ));
    }
    Ok(found.remove(0))
}

fn gate(root: &Path) -> Check<()> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let toolchain = env::var("TOOLCHAIN").unwrap_or_else(|_| "stable".into());
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target_dir = if target_dir.is_absolute() {
        target_dir
    } else {
        root.join(target_dir)
    };
    let work = target_dir.join("thumbv6m-frame-header-codegen");
    let harness = work.join("harness");
    let target = work.join("target");
    checked(
        Command::new("rustup").args(["target", "add", "--toolchain", &toolchain, TARGET]),
        "thumbv6m target setup",
    )?;
    let sysroot = checked(
        Command::new("rustup").args(["run", &toolchain, "rustc", "--print", "sysroot"]),
        "rustc sysroot probe",
    )?;
    let version = checked(
        Command::new("rustup").args(["run", &toolchain, "rustc", "-vV"]),
        "rustc host probe",
    )?;
    let hosts: Vec<_> = version
        .lines()
        .filter_map(|line| line.strip_prefix("host: "))
        .collect();
    if hosts.len() != 1
        || hosts[0].is_empty()
        || !hosts[0]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err("rustc host probe did not return one valid target triple".into());
    }
    let sysroots: Vec<_> = sysroot.lines().filter(|line| !line.is_empty()).collect();
    if sysroots.len() != 1 {
        return Err("rustc sysroot probe did not return one path".into());
    }
    let bundled = Path::new(sysroots[0])
        .join("lib/rustlib")
        .join(hosts[0])
        .join("bin/llvm-nm");
    let tool = if executable(&bundled) {
        bundled
    } else if let Some(tool) = which("rust-nm").or_else(|| which("llvm-nm")) {
        tool
    } else {
        let homebrew = PathBuf::from("/opt/homebrew/opt/llvm/bin/llvm-nm");
        if !executable(&homebrew) {
            return Err("thumbv6m FrameHeader codegen check requires llvm-nm or rust-nm".into());
        }
        homebrew
    };
    if fs::symlink_metadata(&work).is_ok() {
        fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(harness.join("src")).map_err(|e| e.to_string())?;
    let path = root
        .to_str()
        .ok_or("repository path is not UTF8")?
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let manifest = format!(
        "[package]\nname = \"hibana-frame-header-codegen\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n[workspace]\n[dependencies]\nhibana = {{ path = \"{path}\", default-features = false }}\n[lib]\npath = \"src/lib.rs\"\n"
    );
    fs::write(harness.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(harness.join("src/lib.rs"), PROBE).map_err(|e| e.to_string())?;
    checked(
        Command::new("cargo")
            .arg(format!("+{toolchain}"))
            .arg("rustc")
            .arg("--manifest-path")
            .arg(harness.join("Cargo.toml"))
            .args(["--lib", "--release", "--target", TARGET])
            .arg("--target-dir")
            .arg(&target)
            .args(["--", "--emit=obj"])
            .env("CARGO_TERM_COLOR", "never")
            .env("CARGO_TERM_PROGRESS_WHEN", "never")
            .env("TERM", "dumb")
            .current_dir(&root),
        "FrameHeader codegen build",
    )?;
    let object = find_object(&target.join(TARGET).join("release/deps"))?;
    scan_tool(&tool, &object)?;
    println!("thumbv6m FrameHeader codegen has no aeabi u64 helpers");
    Ok(())
}

fn main() {
    let args: Vec<_> = env::args_os().collect();
    let result = if args.len() == 2 {
        gate(Path::new(&args[1]))
    } else if args.len() == 4 && args[1] == "--scan-tool" {
        scan_tool(Path::new(&args[2]), Path::new(&args[3]))
    } else {
        eprintln!("usage: check-thumb-codegen ROOT | --scan-tool NM OBJECT");
        process::exit(2);
    };
    if let Err(error) = result {
        eprintln!("thumbv6m codegen gate failed: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_every_forbidden_helper_with_original_word_boundary() {
        for helper in HELPERS {
            for suffix in ["", ".llvm.123", "@VERSION"] {
                assert!(scan(&format!("         U __aeabi_{helper}{suffix}\n")).is_err());
            }
            assert!(scan(&format!("U __aeabi_{helper}_other\n")).is_ok());
        }
        assert!(scan("U prefix__aeabi_lmul\n").is_err());
    }
    #[test]
    fn accepts_empty_and_valid_undefined_symbol_lists() {
        for text in [
            "",
            "\n",
            "                 U memcpy\n",
            "w optional_symbol\nv optional_object\n",
            "00000000 U __aeabi_memcpy4\n",
        ] {
            assert!(scan(text).is_ok(), "{text}");
        }
    }
    #[test]
    fn rejects_malformed_symbol_output() {
        for text in [
            "garbage",
            "tool error: no symbols",
            "U",
            "00000000 T locally_defined",
            "U symbol unexpected",
            "nothex U symbol",
            "U sym\u{0}bol",
        ] {
            assert!(scan(text).is_err(), "{text}");
        }
    }
}
