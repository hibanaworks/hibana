mod lean_audit;
use lean_audit::*;
use std::{fs, path::Path};
const BEGIN: &str = "HIBANA_STATIC_CLAIM_SURFACE_BEGIN";
const END: &str = "HIBANA_STATIC_CLAIM_SURFACE_END";
fn native_module(path: &Path) -> bool {
    matches!(
        path.to_str(),
        Some("DistributedSemanticsExamples.lean" | "StaticProjectabilityExamples.lean")
    )
}
fn validate_source(root: &Path) -> AuditResult<Names> {
    let source_root = root.join("Hibana");
    let mut decisions = 0;
    let mut declared = Names::new();
    for path in lean_files(&source_root)? {
        let relative = path.strip_prefix(&source_root).map_err(|e| e.to_string())?;
        let source = read(&path)?;
        let code = erase_non_code(&source)?;
        if let Some(token) = forbidden(&code, PROOF_TOKENS) {
            return Err(format!(
                "{}: forbidden proof token {token:?}",
                relative.display()
            ));
        }
        if let Some(token) = forbidden(&code, GENERATOR_TOKENS) {
            return Err(format!(
                "{}: proof-generating command {token:?} would bypass source theorem discovery",
                relative.display()
            ));
        }
        let count = token_positions(&code, "native_decide").len();
        let named = theorem_names(&source)?;
        if native_module(relative) {
            require(
                named.is_empty(),
                format!(
                    "{}: native regression modules cannot export named claims: {named:?}",
                    relative.display()
                ),
            )?;
            decisions += count;
        } else {
            require(
                count == 0,
                format!(
                    "{}: native_decide is outside a native regression module",
                    relative.display()
                ),
            )?;
        }
        declared.extend(named);
    }
    require(
        decisions == 32,
        format!("native regression inventory changed: expected 32, found {decisions}"),
    )?;
    Ok(declared)
}
fn validate_environment(output: &str, theorems: &Names) -> AuditResult<()> {
    let expected = theorems
        .iter()
        .filter(|n| !n.contains('.'))
        .map(|n| format!("Hibana.{n}"))
        .collect();
    let actual = output
        .lines()
        .filter_map(|l| l.strip_prefix("HIBANA_ENV_THEOREM="))
        .filter(|n| n.starts_with("Hibana.") && n.len() > 7)
        .map(str::to_owned)
        .collect();
    same_inventory("Lean environment theorem", &actual, &expected)
}
fn validate_static_axioms(output: &str, theorems: &Names, counts: [usize; 3]) -> AuditResult<()> {
    let expected = theorems.iter().map(|n| format!("Hibana.{n}")).collect();
    let parsed = parse_axioms(output)?;
    same_inventory(
        "static theorem axiom",
        &parsed.keys().cloned().collect(),
        &expected,
    )?;
    let mut actual = [0; 3];
    for (theorem, axioms) in parsed {
        let mut normalized = Names::new();
        for axiom in axioms {
            normalized.insert(
                allowed_axiom(&axiom)
                    .ok_or_else(|| {
                        format!("static theorem {theorem} gained a forbidden axiom: {axiom}")
                    })?
                    .to_owned(),
            );
        }
        if normalized == names(&["propext", "Quot.sound"]) {
            actual[0] += 1;
        } else if normalized == names(&["propext"]) {
            actual[1] += 1;
        } else if normalized.is_empty() {
            actual[2] += 1;
        } else {
            return Err(format!(
                "static theorem {theorem} has an unexpected allowed axiom set: {normalized:?}"
            ));
        }
    }
    require(
        actual == counts,
        format!(
            "static theorem axiom closure counts changed: expected={counts:?} actual={actual:?}"
        ),
    )
}
fn check_static(
    root: &Path,
    snapshot: &Path,
    count: usize,
    counts: [usize; 3],
    write: bool,
) -> AuditResult<()> {
    let declared = validate_source(root)?;
    require(
        declared.len() == count,
        format!(
            "expected {count} static theorem types, found {}",
            declared.len()
        ),
    )?;
    let mut source = format!(
        "import Hibana.MainTheorems\n\nimport Lean.Elab.Command\n\nopen Lean Elab Command\n\nset_option pp.universes true\nset_option pp.explicit true\n\n#eval IO.println \"{BEGIN}\"\n"
    );
    for name in &declared {
        source += &format!("#check @Hibana.{name}\n");
    }
    source += &format!(
        "#eval IO.println \"{END}\"\nrun_cmd\n  let env ← getEnv\n  for (name, info) in env.constants.toList do\n    if name.getPrefix == `Hibana && info.isTheorem then\n      logInfo m!\"HIBANA_ENV_THEOREM={{name}}\"\n"
    );
    for name in &declared {
        source += &format!("#print axioms Hibana.{name}\n");
    }
    let output = successful_lean(root, &source)?;
    let actual = extract_claim_surface(&output, BEGIN, END)?;
    same_inventory("static claim type", &claim_names(&actual, true)?, &declared)?;
    validate_static_axioms(&output, &declared, counts)?;
    validate_environment(&output, &declared)?;
    if write {
        fs::write(snapshot, &actual).map_err(|e| e.to_string())?;
    }
    let expected = read(snapshot)?;
    same_inventory(
        "static claim snapshot",
        &claim_names(&expected, true)?,
        &declared,
    )?;
    check_surface(&actual, &expected)
}
fn name_examples(path: &Path, source: &str) -> AuditResult<(String, Vec<String>)> {
    let code = erase_non_code(source)?;
    let positions = token_positions(&code, "example");
    let owner = path
        .with_extension("")
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '\'' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let names = positions
        .iter()
        .enumerate()
        .map(|(i, _)| format!("anonymousRegression_{owner}_{:03}", i + 1))
        .collect::<Vec<_>>();
    let mut transformed = source.to_string();
    for (i, name) in positions.iter().zip(&names).rev() {
        transformed.replace_range(*i..*i + "example".len(), &format!("theorem {name}"));
    }
    Ok((transformed, names))
}
fn validate_examples(output: &str, path: &Path, theorems: &[String]) -> AuditResult<()> {
    let expected = theorems.iter().map(|n| format!("Hibana.{n}")).collect();
    let parsed = parse_axioms(output)?;
    same_inventory(
        "anonymous example axiom",
        &parsed.keys().cloned().collect(),
        &expected,
    )?;
    for (theorem, axioms) in parsed {
        let extra = axioms
            .iter()
            .filter(|a| allowed_axiom(a).is_none())
            .collect::<Vec<_>>();
        if native_module(path) {
            require(
                extra.len() == 1,
                format!(
                    "native anonymous example {theorem} must own one native decision, found {extra:?}"
                ),
            )?;
            require(
                native_owner(extra[0]) == Some(theorem.as_str()),
                format!(
                    "native anonymous example {theorem} has a foreign decision owner: {}",
                    extra[0]
                ),
            )?;
        } else {
            require(
                extra.is_empty(),
                format!("kernel anonymous example {theorem} gained forbidden axioms: {extra:?}"),
            )?;
        }
    }
    Ok(())
}
fn example_surface(root: &Path) -> AuditResult<(String, Names)> {
    let mut surface = String::new();
    let mut all = Names::new();
    let source_root = root.join("Hibana");
    for path in lean_files(&source_root)? {
        let relative = path.strip_prefix(&source_root).map_err(|e| e.to_string())?;
        let (mut source, theorems) = name_examples(relative, &read(&path)?)?;
        if theorems.is_empty() {
            continue;
        }
        for n in &theorems {
            require(
                all.insert(n.clone()),
                format!("duplicate anonymous example audit name: {n}"),
            )?;
        }
        source += &format!(
            "\nset_option pp.universes true\nset_option pp.explicit true\n\n#eval IO.println \"{BEGIN}\"\n"
        );
        for n in &theorems {
            source += &format!("#check @Hibana.{n}\n");
        }
        source += &format!("#eval IO.println \"{END}\"\n");
        for n in &theorems {
            source += &format!("#print axioms Hibana.{n}\n");
        }
        let output = successful_lean(root, &source)?;
        validate_examples(&output, relative, &theorems)?;
        surface += &extract_claim_surface(&output, BEGIN, END)?;
    }
    Ok((surface, all))
}
fn check_examples(root: &Path, snapshot: &Path, count: usize, write: bool) -> AuditResult<()> {
    validate_source(root)?;
    let (actual, names) = example_surface(root)?;
    require(
        names.len() == count,
        format!(
            "expected {count} anonymous example types, found {}",
            names.len()
        ),
    )?;
    same_inventory(
        "anonymous example elaboration",
        &claim_names(&actual, true)?,
        &names,
    )?;
    if write {
        fs::write(snapshot, &actual).map_err(|e| e.to_string())?;
    }
    let expected = read(snapshot)?;
    same_inventory(
        "anonymous example snapshot",
        &claim_names(&expected, true)?,
        &names,
    )?;
    check_surface(&actual, &expected)
}
fn self_test() -> AuditResult<()> {
    let declarations = r#"namespace Hibana
theorem plain : True := by trivial
@[simp] protected theorem unicode_一意? : True := by trivial
theorem
 punctuation!' : True := by trivial
lemma equivalent_name : True := by trivial
private theorem hidden : True := by trivial
private lemma hidden_lemma : True := by trivial
-- theorem line_comment : False := by trivial
/- theorem block_comment : False := by trivial /- theorem nested_comment : False := by trivial -/ -/
def quoted : String := "theorem string_literal : False"
end Hibana"#;
    let expected = names(&["plain", "unicode_一意?", "punctuation!'", "equivalent_name"]);
    same_inventory(
        "declaration scanner",
        &theorem_names(declarations)?,
        &expected,
    )?;
    require(
        forbidden(
            &erase_non_code("private axiom hidden : False")?,
            PROOF_TOKENS,
        )
        .is_some(),
        "accepted prefixed axiom",
    )?;
    require(
        forbidden(&erase_non_code("-- axiom hidden : False\n")?, PROOF_TOKENS).is_none(),
        "comment treated as axiom",
    )?;
    require(
        forbidden("macro generated : command", GENERATOR_TOKENS).is_some(),
        "accepted proof generator",
    )?;
    let env = expected
        .iter()
        .map(|n| format!("HIBANA_ENV_THEOREM=Hibana.{n}\n"))
        .collect::<String>();
    validate_environment(&env, &expected)?;
    let output = "'Hibana.plain' depends on axioms: [propext, Quot.sound.{u}]\n'Hibana.unicode_一意?' depends on axioms: [propext]\n'Hibana.punctuation!'' does not depend on any axioms\n'Hibana.equivalent_name' does not depend on any axioms\n";
    validate_static_axioms(output, &expected, [1, 1, 2])?;
    require(
        validate_static_axioms(
            "'Hibana.plain' depends on axioms: [Classical.choice]\n",
            &names(&["plain"]),
            [0, 0, 1],
        )
        .is_err(),
        "accepted forbidden axiom",
    )?;
    let surface = extract_claim_surface(
        &format!(
            "noise\n{BEGIN}\nHibana.plain : True\n@Hibana.parameterized : ∀ {{value : Nat}}, value = value\n{END}\nnoise"
        ),
        BEGIN,
        END,
    )?;
    same_inventory(
        "claim scanner",
        &claim_names(&surface, true)?,
        &names(&["plain", "parameterized"]),
    )?;
    let source = "namespace Hibana\nexample : True := by trivial\n-- example : False\ndef quoted : String := \"example : False\"\n/- example : False -/\nexample (value : Nat) : value = value := by rfl\nend Hibana\n";
    let (transformed, example_names) = name_examples(Path::new("Examples.lean"), source)?;
    require(
        example_names
            == [
                "anonymousRegression_Examples_001",
                "anonymousRegression_Examples_002",
            ],
        "example names changed",
    )?;
    require(
        token_positions(&erase_non_code(&transformed)?, "example").is_empty(),
        "unnamed obligation remained",
    )?;
    let n = "anonymousRegression_StaticProjectabilityExamples_001";
    let good = format!(
        "'Hibana.{n}' depends on axioms: [propext, Quot.sound.{{u}}, Hibana.{n}._native.native_decide.ax_1_1]\n"
    );
    validate_examples(
        &good,
        Path::new("StaticProjectabilityExamples.lean"),
        &[n.into()],
    )?;
    require(
        validate_examples(
            &format!("'Hibana.{n}' depends on axioms: [foreign.ax]\n"),
            Path::new("StaticProjectabilityExamples.lean"),
            &[n.into()],
        )
        .is_err(),
        "accepted foreign example axiom",
    )?;
    for source in [
        "/- unterminated",
        "\"unterminated",
        "theorem «escaped» : True",
    ] {
        require(theorem_names(source).is_err(), "accepted malformed source")?;
    }
    require(
        parse_axioms("'a' does not depend on any axioms\n'a' does not depend on any axioms\n")
            .is_err(),
        "accepted duplicate axiom report",
    )?;
    require(
        claim_names("Hibana.a : True\nHibana.a : True\n", true).is_err(),
        "accepted duplicate claims",
    )?;
    require(
        check_surface("a : True\n", "a : False\n").is_err(),
        "accepted changed theorem type",
    )?;
    require(
        validate_environment("HIBANA_ENV_THEOREM=Hibana.foreign\n", &expected).is_err(),
        "accepted changed environment",
    )?;
    Ok(())
}
fn count(s: &str) -> AuditResult<usize> {
    s.parse().map_err(|_| format!("invalid count: {s}"))
}
fn main_result(args: &[String]) -> AuditResult<i32> {
    match args {
        [flag] if flag == "--self-test" => {
            self_test()?;
            println!("Lean theorem inventory self-test passed");
        }
        [mode, root, snapshot, total, both, propext, free]
            if mode == "--static" || mode == "--write-static" =>
        {
            check_static(
                Path::new(root),
                Path::new(snapshot),
                count(total)?,
                [count(both)?, count(propext)?, count(free)?],
                mode == "--write-static",
            )?;
            println!(
                "Lean static theorem audit passed theorems={total} both={both} propext={propext} free={free}"
            );
        }
        [mode, root, snapshot, total]
            if mode == "--example-types" || mode == "--write-example-types" =>
        {
            let total = count(total)?;
            check_examples(
                Path::new(root),
                Path::new(snapshot),
                total,
                mode == "--write-example-types",
            )?;
            println!(
                "Lean anonymous example type audit passed examples={total} native=32 kernel={}",
                total as i128 - 32
            );
        }
        _ => {
            eprintln!(
                "usage: check-lean-theorem-inventory --self-test | --static|--write-static PROOF_DIR SNAPSHOT EXPECTED_COUNT EXPECTED_BOTH EXPECTED_PROPEXT EXPECTED_FREE | --example-types|--write-example-types PROOF_DIR SNAPSHOT EXPECTED_COUNT"
            );
            return Ok(2);
        }
    }
    Ok(0)
}
fn main() {
    let code = match main_result(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Lean theorem inventory audit failed: {e}");
            1
        }
    };
    std::process::exit(code);
}
#[cfg(test)]
mod tests {
    #[test]
    fn self_tests() {
        super::self_test().unwrap();
    }
}
