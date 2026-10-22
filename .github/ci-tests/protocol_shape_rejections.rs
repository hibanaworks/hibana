//! Build each expected rejection independently and inspect rustc diagnostics.
#[path = "kani_json.rs"]
mod json;
use json::Json;
use std::{collections::BTreeSet, env, fs, path::PathBuf, process::Command};

fn check_diagnostics(output: &str, reason: &str) -> Result<(), String> {
    let mut found = false;
    let expected = Json::String(format!("evaluation panicked: {reason}"));
    for line in output.lines() {
        let record = json::parse(line)?;
        if record.get("reason") != Some(&Json::String("compiler-message".into())) {
            continue;
        }
        let diagnostic = record.get("message").ok_or("missing compiler diagnostic")?;
        if diagnostic.get("level") != Some(&Json::String("error".into())) {
            continue;
        }
        if diagnostic.get("code").and_then(|code| code.get("code"))
            != Some(&Json::String("E0080".into()))
            || diagnostic.get("message") != Some(&expected)
        {
            return Err("unexpected compiler error alongside the expected rejection".into());
        }
        found = true;
    }
    if found {
        Ok(())
    } else {
        Err(format!("rejection did not establish {reason}"))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: protocol-shape-rejections GENERATED TOOLCHAIN".into());
    }
    let root = PathBuf::from(&args[0]).join("rejected");
    let inventory = fs::read_to_string(root.join("inventory.tsv"))?;
    let mut cases = BTreeSet::new();
    for line in inventory.lines() {
        let (case, reason) = line.split_once('\t').ok_or("invalid rejection inventory")?;
        if case.is_empty()
            || reason.is_empty()
            || !case.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || !cases.insert(case.to_owned())
        {
            return Err("empty or duplicate rejection".into());
        }
        let bin = case.to_owned();
        let output = Command::new("cargo")
            .arg(format!("+{}", args[1]))
            .args(["build", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .args(["--bin", &bin, "--message-format=json"])
            .output()?;
        fs::write(root.join(format!("{case}.jsonl")), &output.stdout)?;
        fs::write(root.join(format!("{case}.stderr")), &output.stderr)?;
        if output.status.code() != Some(101) {
            return Err(format!(
                "shape {case}: expected compiler rejection, got {}",
                output.status
            )
            .into());
        }
        check_diagnostics(std::str::from_utf8(&output.stdout)?, reason)
            .map_err(|error| format!("shape {case}: {error}; see {}", root.display()))?;
    }
    let actual = fs::read_dir(root.join("src/bin"))?
        .map(|entry| {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                return Err(std::io::Error::other("unknown rejection source extension"));
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| std::io::Error::other("non-UTF8 rejection path"))?;
            Ok::<_, std::io::Error>(name.to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual != cases {
        return Err("missing or unknown rejection source".into());
    }
    println!(
        "Generated shape compile-fail: cases={} independently rejected",
        cases.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_diagnostics;

    const EXPECTED: &str = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0080"},"message":"evaluation panicked: expected"}}"#;

    #[test]
    fn the_expected_const_error_and_instantiation_notes_are_accepted() {
        let output = format!(
            "{EXPECTED}\n{}",
            r#"{"reason":"compiler-message","message":{"level":"note","code":null,"message":"erroneous constant encountered"}}"#
        );
        assert!(check_diagnostics(&output, "expected").is_ok());
    }

    #[test]
    fn an_expected_error_does_not_hide_an_unrelated_compile_failure() {
        let unrelated = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types"}}"#;
        for output in [
            format!("{EXPECTED}\n{unrelated}"),
            format!("{unrelated}\n{EXPECTED}"),
        ] {
            assert!(check_diagnostics(&output, "expected").is_err());
        }
    }

    #[test]
    fn the_error_code_alone_does_not_establish_the_intended_rejection() {
        assert!(check_diagnostics(EXPECTED, "another rule").is_err());
        assert!(check_diagnostics("", "expected").is_err());
    }

    #[test]
    fn malformed_diagnostics_are_rejected() {
        assert!(check_diagnostics(&format!("{EXPECTED}\n{{"), "expected").is_err());
    }
}
