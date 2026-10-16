mod lean_audit;
use lean_audit::*;
use std::{env, fs, path::Path};
const MODULES: &[(&str, &str)] = &[
    (
        "OptionMatchEquivalence",
        "Hibana.WireOptionMatchEquivalence",
    ),
    ("WireFrameRefinement", "Hibana.WireFrameRefinement"),
    ("WireSourceBridge", "Hibana.WireFrameSourceBridge"),
];
fn inventory(repo: &Path) -> AuditResult<Names> {
    let mut result = Names::new();
    for (module, namespace) in MODULES {
        let source = read(&repo.join(format!("proofs/wire-frame-refinement/{module}.lean")))?;
        let code = erase_non_code(&source)?;
        require(
            forbidden(
                &code,
                &[
                    "sorry",
                    "admit",
                    "axiom",
                    "constant",
                    "opaque",
                    "unsafe",
                    "native_decide",
                ],
            )
            .is_none(),
            format!("untrusted declaration in {module}"),
        )?;
        for name in theorem_names(&source)? {
            result.insert(format!("{namespace}.{name}"));
        }
    }
    require(
        result.len() == 56,
        format!("expected 56 wire theorems, found {}", result.len()),
    )?;
    Ok(result)
}
fn verify(output: &str, expected: &Names) -> AuditResult<()> {
    require(
        !output.contains("error:") && !output.contains("sorryAx"),
        "Lean error or sorry axiom",
    )?;
    let actual = parse_axioms(output)?;
    same_inventory(
        "wire axiom audit",
        &actual.keys().cloned().collect(),
        expected,
    )?;
    for (name, axioms) in actual {
        require(
            axioms.is_subset(&names(&["propext", "Quot.sound"])),
            format!("forbidden wire axioms in {name}: {axioms:?}"),
        )?;
    }
    Ok(())
}
fn run(args: &[String]) -> AuditResult<()> {
    require(args.len() == 4, "usage: wire-audit emit|verify REPO FILE")?;
    let expected = inventory(Path::new(&args[2]))?;
    match args[1].as_str() {
        "emit" => {
            let mut source = MODULES
                .iter()
                .map(|(module, _)| format!("import {module}\n"))
                .collect::<String>();
            for name in expected {
                source.push_str(&format!("#print axioms {name}\n"));
            }
            fs::write(&args[3], source).map_err(|e| e.to_string())?;
        }
        "verify" => {
            verify(&read(Path::new(&args[3]))?, &expected)?;
            println!(
                "Wire theorem axiom audit: {} theorems passed",
                expected.len()
            );
        }
        _ => return Err("unknown wire audit command".into()),
    }
    Ok(())
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
    fn exact_closed_axiom_inventory() {
        let expected = names(&["Hibana.A", "Hibana.B"]);
        let good = "'Hibana.A' depends on axioms: [propext, Quot.sound]\n'Hibana.B' does not depend on any axioms\n";
        assert!(verify(good, &expected).is_ok());
        for bad in [
            good.replace("Quot.sound", "sorryAx"),
            good.replace("propext", "Classical.choice"),
            good.replace("'Hibana.B' does not depend on any axioms\n", ""),
            format!("{good}'Hibana.Extra' does not depend on any axioms\n"),
            format!("{good}'Hibana.B' does not depend on any axioms\n"),
        ] {
            assert!(verify(&bad, &expected).is_err(), "{bad}");
        }
    }
}
