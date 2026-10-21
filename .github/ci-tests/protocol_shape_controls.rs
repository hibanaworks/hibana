//! Corrupt result data to test the compiled shape checker, not Rust source text.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn copy_directory(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        fs::copy(entry.path(), target.join(entry.file_name()))?;
    }
    Ok(())
}
fn rejected(
    checker: &Path,
    inputs: &Path,
    outputs: &Path,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = Command::new(checker).arg(inputs).arg(outputs).output()?;
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if result.status.success() || !report.contains(expected) {
        return Err(format!("negative control was not detected: {expected}: {report}").into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: protocol-shape-controls CHECKER GENERATED_DIRECTORY".into());
    }
    let checker = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let inputs = output.join("inputs.json");
    let original = output.join("observations");
    let mut paths = fs::read_dir(&original)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let first = paths.first().ok_or("no observed cases")?;
    for (name, needle, replacement, expected) in [
        (
            "false-rejection",
            r#""accepted":true"#,
            r#""accepted":false"#,
            "runtime-only=0 model-only=1",
        ),
        (
            "false-admission",
            r#""accepted":false"#,
            r#""accepted":true"#,
            "runtime-only=1 model-only=0",
        ),
    ] {
        let target = output.join("controls").join(name);
        copy_directory(&original, &target)?;
        let path = target.join(first.file_name().unwrap());
        let text = fs::read_to_string(&path)?;
        if !text.contains(needle) {
            return Err(format!("missing mutation point {name}").into());
        }
        fs::write(&path, text.replacen(needle, replacement, 1))?;
        rejected(&checker, &inputs, &target, expected)?;
    }
    let missing = output.join("controls/missing-case");
    copy_directory(&original, &missing)?;
    fs::remove_file(missing.join(first.file_name().unwrap()))?;
    rejected(
        &checker,
        &inputs,
        &missing,
        "missing, duplicate or unknown shape observation",
    )?;
    let malformed = output.join("controls/malformed");
    copy_directory(&original, &malformed)?;
    fs::write(malformed.join(first.file_name().unwrap()), "{")?;
    rejected(&checker, &inputs, &malformed, "invalid shape JSON")?;
    println!(
        "Generated shape checker controls: false-admission, false-rejection, missing-case, malformed: pass"
    );
    Ok(())
}
