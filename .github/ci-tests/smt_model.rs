//! Exact solver-outcome checking for standalone SMT-LIB proof models.
use std::{env, fs, process::Command};
fn expected(source: &str) -> Result<Vec<&str>, String> {
    let values = source
        .lines()
        .filter_map(|line| match line {
            "; EXPECT: sat" => Some("sat"),
            "; EXPECT: unsat" => Some("unsat"),
            _ => None,
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        Err("SMT model has no expected queries".into())
    } else {
        Ok(values)
    }
}
fn validate(source: &str, output: &str) -> Result<(), String> {
    let expected = expected(source)?;
    let actual = output.lines().collect::<Vec<_>>();
    if actual != expected {
        return Err(format!(
            "SMT outcomes differ: expected {expected:?}, actual {actual:?}"
        ));
    }
    Ok(())
}
fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: smt-model FILE.smt2".into());
    }
    let source = fs::read_to_string(&args[1]).map_err(|e| e.to_string())?;
    expected(&source)?;
    let output = Command::new("z3")
        .args(["-T:300", &args[1]])
        .output()
        .map_err(|e| format!("cannot run z3: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "z3 failed {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let actual = std::str::from_utf8(&output.stdout).map_err(|e| e.to_string())?;
    validate(&source, actual)?;
    print!("{actual}");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_terminal_results_pass() {
        let source = "; EXPECT: sat\n(check-sat)\n; EXPECT: unsat\n(check-sat)\n";
        assert!(validate(source, "sat\nunsat\n").is_ok());
        for output in [
            "",
            "sat\n",
            "sat\nunknown\n",
            "sat\nunsat\nsat\n",
            "unsat\nsat\n",
            "(error)\nsat\nunsat\n",
        ] {
            assert!(validate(source, output).is_err());
        }
        assert!(expected("(check-sat)").is_err());
    }
}
