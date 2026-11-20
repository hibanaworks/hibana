//! Fail-closed verdict checks for the small standalone proof families.
mod lean_audit;
mod lean_module;
use lean_audit::*;
use std::{
    env, fs,
    io::{self, Read},
    path::Path,
    process::Command,
};
fn lean(output: &str, count: usize) -> AuditResult<()> {
    let reports = parse_axioms(output)?;
    require(
        reports.len() == count,
        format!(
            "expected {count} Lean axiom reports, found {}",
            reports.len()
        ),
    )?;
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        require(
            line.starts_with('\'')
                && (line.ends_with("does not depend on any axioms")
                    || (line.contains("' depends on axioms: [") && line.ends_with(']'))),
            "unexpected Lean output",
        )?;
    }
    for (name, axioms) in reports {
        require(
            axioms.is_subset(&names(&["propext", "Quot.sound", "Classical.choice"])),
            format!("forbidden Lean axioms: {name} {axioms:?}"),
        )?;
    }
    Ok(())
}
fn run(args: &[String]) -> AuditResult<()> {
    require(args.len() >= 2, "missing proof validation mode")?;
    if args[1] == "module-self-test" {
        require(args.len() == 2, "usage: module-self-test")?;
        return module_self_test(&env::var("LEAN_BIN").unwrap_or_else(|_| "lean".into()));
    }
    if args[1] == "module" {
        require(
            args.len() == 3 || args.len() == 4,
            "usage: module SOURCE [OUTPUT]",
        )?;
        return lean_module::check(
            &env::var("LEAN_BIN").unwrap_or_else(|_| "lean".into()),
            Path::new(&args[2]),
            args.get(3).map(Path::new),
        );
    }
    if args[1] == "version" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        require(text.contains("version 4.30.0"), "requires Lean 4.30.0")?;
        print!("{text}");
        return Ok(());
    }
    require(args.len() >= 3, "missing evidence path")?;
    let text = read(Path::new(&args[2]))?;
    match args[1].as_str() {
        "lean" => lean(
            &text,
            args.get(3)
                .ok_or("missing theorem count")?
                .parse()
                .map_err(|_| "invalid theorem count")?,
        ),
        _ => Err("unknown proof validation mode".into()),
    }
}

fn module_self_test(lean: &str) -> Result<(), String> {
    let directory =
        std::env::temp_dir().join(format!("hibana-compiled-axioms-{}", std::process::id()));
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut checked = 0;
        for (name, source, accepted) in [
            ("valid", "theorem checked : True := by trivial", true),
            (
                "valid-binder",
                "theorem checked (admit : Bool) (allowed : admit = true) : admit = true := allowed",
                true,
            ),
            (
                "private-axiom",
                "private axiom hidden : False\ntheorem checked : True := by trivial",
                false,
            ),
            (
                "unused-hole",
                "theorem hidden : False := by sorry\ntheorem checked : True := by trivial",
                false,
            ),
            (
                "unused-private-hole",
                "private theorem hidden : False := by sorry\ntheorem checked : True := by trivial",
                false,
            ),
            (
                "axiom-dependency",
                "axiom hidden : False\ntheorem checked : False := hidden",
                false,
            ),
            ("empty", "def data : Nat := 3", false),
            (
                "comment",
                "-- sorry axiom native_decide\ntheorem checked : True := by trivial",
                true,
            ),
        ] {
            let file = directory.join(format!("{name}.lean"));
            fs::write(&file, source).map_err(|e| e.to_string())?;
            println!("Compiled-axiom control {name}: expected accepted={accepted}");
            if lean_module::check(lean, &file, None).is_ok() != accepted {
                return Err(format!("compiled axiom audit misclassified {name}"));
            }
            checked += 1;
        }
        fs::create_dir(directory.join("Hibana")).map_err(|e| e.to_string())?;
        for (name, source, accepted) in [
            (
                "imported-binder",
                "theorem checked (admit : Bool) (h : admit = true) : admit = true := h",
                true,
            ),
            (
                "imported-private-axiom",
                "private axiom hidden : False\ntheorem checked : True := by trivial",
                false,
            ),
            (
                "imported-private-hole",
                "private theorem hidden : False := by sorry\ntheorem checked : True := by trivial",
                false,
            ),
            (
                "imported-native",
                "theorem checked : True := by native_decide",
                false,
            ),
        ] {
            fs::write(
                directory.join("Hibana/Fixture.lean"),
                format!("import Std\n{source}"),
            )
            .map_err(|e| e.to_string())?;
            let compiled = Command::new(lean)
                .current_dir(&directory)
                .args(["-o", "Hibana/Fixture.olean", "Hibana/Fixture.lean"])
                .output()
                .map_err(|e| e.to_string())?;
            require(
                compiled.status.success(),
                format!(
                    "compile {name}: {}",
                    String::from_utf8_lossy(&compiled.stderr)
                ),
            )?;
            let query = format!(
                "import Hibana.Fixture\nimport Lean.Elab.Command\n{}",
                declaration_guard(
                    "definingModule.any (fun moduleName => (`Hibana).isPrefixOf moduleName)",
                    &[]
                )
            );
            fs::write(directory.join("ImportAudit.lean"), query).map_err(|e| e.to_string())?;
            let audited = Command::new(lean)
                .current_dir(&directory)
                .env("LEAN_PATH", &directory)
                .arg("ImportAudit.lean")
                .output()
                .map_err(|e| e.to_string())?;
            println!("Compiled-axiom control {name}: expected accepted={accepted}");
            require(
                audited.status.success() == accepted,
                format!(
                    "compiled import audit misclassified {name}: {}{}",
                    String::from_utf8_lossy(&audited.stdout),
                    String::from_utf8_lossy(&audited.stderr)
                ),
            )?;
            checked += 1;
        }
        println!("Compiled-axiom controls passed: {checked}");
        Ok(())
    })();
    let removed = fs::remove_dir_all(&directory).map_err(|e| e.to_string());
    result.and(removed)
}

fn main() {
    if let Err(error) = run(&env::args().collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lean_closure_mutants() {
        let good =
            "'A' depends on axioms: [propext, Quot.sound]\n'B' does not depend on any axioms\n";
        assert!(lean(good, 2).is_ok());
        for bad in [
            good.replace("propext", "sorryAx"),
            format!("garbage\n{good}"),
            good.replace("'B' does not depend on any axioms\n", ""),
        ] {
            assert!(lean(&bad, 2).is_err());
        }
    }
}
