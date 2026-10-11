//! Fail-closed verdict checks for the small standalone proof families.
mod lean_audit;
use lean_audit::*;
use std::{
    env,
    io::{self, Read},
    path::Path,
};
fn smt(output: &str, expected: &[&str], counts: bool) -> AuditResult<()> {
    let actual = output.lines().collect::<Vec<_>>();
    if counts {
        require(expected.len() == 2, "counts require sat and unsat totals")?;
        let sat = expected[0]
            .parse::<usize>()
            .map_err(|_| "invalid sat count")?;
        let unsat = expected[1]
            .parse::<usize>()
            .map_err(|_| "invalid unsat count")?;
        require(
            actual
                .iter()
                .all(|line| matches!(*line, "sat" | "unsat") || line.trim().is_empty()),
            "unexpected SMT output",
        )?;
        require(
            actual.iter().filter(|&&line| line == "sat").count() == sat
                && actual.iter().filter(|&&line| line == "unsat").count() == unsat,
            "SMT outcome count mismatch",
        )
    } else {
        require(
            actual == expected,
            format!("SMT outcome order mismatch: expected {expected:?}, actual {actual:?}"),
        )
    }
}
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
            axioms.is_subset(&names(&["propext", "Quot.sound"])),
            format!("forbidden Lean axioms: {name} {axioms:?}"),
        )?;
    }
    Ok(())
}
fn run(args: &[String]) -> AuditResult<()> {
    require(args.len() >= 2, "missing proof validation mode")?;
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
        "smt-counts" | "smt-sequence" => smt(
            &text,
            &args[3..].iter().map(String::as_str).collect::<Vec<_>>(),
            args[1] == "smt-counts",
        ),
        "lean" => lean(
            &text,
            args.get(3)
                .ok_or("missing theorem count")?
                .parse()
                .map_err(|_| "invalid theorem count")?,
        ),
        "lean-clean" => require(
            !["sorryAx", "_native", "error:", "warning:"]
                .iter()
                .any(|s| text.contains(s)),
            "untrusted Lean output",
        ),
        "source" => require(
            forbidden(
                &erase_non_code(&text)?,
                &["sorry", "admit", "axiom", "native_decide", "unsafe"],
            )
            .is_none(),
            "untrusted proof declaration",
        ),
        _ => Err("unknown proof validation mode".into()),
    }
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
    fn verdict_mutants() {
        assert!(smt("sat\nunsat\n", &["1", "1"], true).is_ok());
        for bad in ["unknown\nunsat\n", "sat\n", "sat\nunsat\ngarbage\n"] {
            assert!(smt(bad, &["1", "1"], true).is_err());
        }
        assert!(smt("sat\nunsat\n", &["sat", "unsat"], false).is_ok());
        assert!(smt("unsat\nsat\n", &["sat", "unsat"], false).is_err());
    }
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
