mod lean_audit;
use lean_audit::*;
use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::Path};
const BEGIN: &str = "HIBANA_GENERATED_CLAIM_SURFACE_BEGIN";
const END: &str = "HIBANA_GENERATED_CLAIM_SURFACE_END";
const FAMILIES: &[&str] = &[
    "",
    "Resolved",
    "Rejecting",
    "Rolled",
    "RolledResolved",
    "NestedResolved",
    "NestedRolled",
    "CyclicRoll",
];
fn exact_certificates() -> Names {
    let mut result = Names::new();
    for (family, count) in FAMILIES.iter().zip([4, 2, 2, 2, 2, 2, 2, 3]) {
        for role in 0..count {
            result.insert(format!(
                "generated{family}ProjectionRole{role}ExactAccepted"
            ));
        }
    }
    result.extend(names(&[
        "generatedFullRoleDomainProjectionRole254ExactAccepted",
        "generatedFullRoleDomainProjectionRole255ExactAccepted",
        "generatedLaneMatchingProjectionRole3ExactAccepted",
    ]));
    result
}
fn certificates(kind: &str) -> Names {
    FAMILIES
        .iter()
        .map(|f| format!("generated{f}{kind}Accepted"))
        .collect()
}
fn kernel_production() -> Names {
    names(&[
        "generatedProductionKernelArtifactAccepted",
        "generated_production_protocol_cases_refine_prepared_kernels",
        "generated_production_codec_coverage_for",
        "generated_production_codec_coverage",
        "generated_production_closing_carrier",
    ])
}
fn native_production() -> Names {
    names(&[
        "generatedVerifiedProtocolFamilyCapabilitiesAccepted",
        "generated_verified_protocol_family_covers_core_capabilities",
        "generatedStaticDeploymentCertificateAccepted",
        "generated_static_deployment_certificate_refines_exact_family",
        "generatedProductionRoleImagesAccepted",
    ])
}
fn rejections() -> Names {
    names(&[
        "generatedMissingStaticDeploymentCertificateRejected",
        "generatedExtraStaticDeploymentCertificateRejected",
        "generatedCorruptStaticDeploymentCertificateRejected",
    ])
}
fn is_exact(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("generated")
        .and_then(|s| s.strip_suffix("ExactAccepted"))
    else {
        return false;
    };
    let Some((_, role)) = stem.rsplit_once("ProjectionRole") else {
        return false;
    };
    !role.is_empty() && role.chars().all(|c| c.is_ascii_digit())
}
fn classify(theorems: &Names) -> AuditResult<(Names, Names)> {
    let exact = theorems.iter().filter(|n| is_exact(n)).cloned().collect();
    let projectability = theorems
        .iter()
        .filter(|n| n.starts_with("generated") && n.ends_with("ProjectabilityAccepted"))
        .cloned()
        .collect();
    let verified = theorems
        .iter()
        .filter(|n| n.starts_with("generated") && n.ends_with("VerifiedProtocolAccepted"))
        .cloned()
        .collect();
    same_inventory("generated exact certificate", &exact, &exact_certificates())?;
    same_inventory(
        "generated projectability",
        &projectability,
        &certificates("Projectability"),
    )?;
    same_inventory(
        "generated verified protocol",
        &verified,
        &certificates("VerifiedProtocol"),
    )?;
    let mut kernel: Names = exact.union(&kernel_production()).cloned().collect();
    let mut native: Names = projectability.union(&verified).cloned().collect();
    native.extend(native_production());
    let contract: Names = kernel.union(&native).cloned().collect();
    require(
        contract.len() == 48,
        format!(
            "expected 48 generated contract theorems, found {}",
            contract.len()
        ),
    )?;
    require(
        contract.is_subset(theorems),
        format!(
            "generated contract theorem inventory changed: missing={:?}",
            contract.difference(theorems).collect::<Vec<_>>()
        ),
    )?;
    for n in theorems.difference(&contract) {
        if n.ends_with("ProjectabilityEnsuresGlobalProgress")
            || n.ends_with("VerifiedProtocolAllRolesRefine")
            || rejections().contains(n)
        {
            native.insert(n.clone());
        } else {
            kernel.insert(n.clone());
        }
    }
    Ok((kernel, native))
}
fn expected_owners(theorem: &str) -> AuditResult<Names> {
    let verified = certificates("VerifiedProtocol");
    if certificates("Projectability").contains(theorem) || verified.contains(theorem) {
        return Ok(names(&[theorem]));
    }
    if theorem == "generatedProductionRoleImagesAccepted" {
        return Ok(names(&["generatedVerifiedProtocolAccepted"]));
    }
    if native_production().contains(theorem) || rejections().contains(theorem) {
        return Ok(verified);
    }
    for (ending, suffix) in [
        (
            "ProjectabilityEnsuresGlobalProgress",
            "EnsuresGlobalProgress",
        ),
        ("VerifiedProtocolAllRolesRefine", "AllRolesRefine"),
    ] {
        if theorem.ends_with(ending) {
            return Ok(names(&[&format!(
                "{}Accepted",
                theorem.strip_suffix(suffix).unwrap()
            )]));
        }
    }
    Err(format!(
        "generated theorem is not classified as native: {theorem}"
    ))
}
fn validate_axioms(
    parsed: &BTreeMap<String, Names>,
    kernel: &Names,
    native: &Names,
) -> AuditResult<()> {
    let expected = kernel.union(native).cloned().collect();
    same_inventory(
        "generated axiom",
        &parsed.keys().cloned().collect(),
        &expected,
    )?;
    for (name, axioms) in parsed {
        let mut owners = Names::new();
        for axiom in axioms {
            if let Some(owner) = native_owner(axiom) {
                owners.insert(owner.to_owned());
            } else {
                require(
                    matches!(axiom.as_str(), "propext" | "Quot.sound"),
                    format!("generated theorem {name} gained forbidden axiom: {axiom}"),
                )?;
            }
        }
        if kernel.contains(name) {
            require(
                owners.is_empty(),
                format!(
                    "kernel-checked generated theorem {name} depends on native_decide: {owners:?}"
                ),
            )?;
        }
        if native.contains(name) {
            require(
                !owners.is_empty(),
                format!("native generated theorem {name} lost its explicit native boundary"),
            )?;
            same_inventory(
                &format!("native generated theorem {name} closure dependencies"),
                &owners,
                &expected_owners(name)?,
            )?;
        }
    }
    Ok(())
}
fn validate_surface(actual: &str, expected: &str, theorems: &Names) -> AuditResult<()> {
    same_inventory(
        "generated claim surface",
        &claim_names(actual, false)?,
        theorems,
    )?;
    check_surface(actual, expected)
}
fn checked_source(generated: &Path, expected_decisions: usize) -> AuditResult<String> {
    let source = read(generated)?;
    let code = erase_non_code(&source)?;
    let tokens = PROOF_TOKENS
        .iter()
        .chain(GENERATOR_TOKENS)
        .copied()
        .chain(["example"])
        .collect::<Vec<_>>();
    if let Some(token) = forbidden(&code, &tokens) {
        return Err(format!(
            "generated Lean contains a forbidden declaration, anonymous proof, or proof escape: {token:?}"
        ));
    }
    let count = code.matches("native_decide").count();
    require(
        count == expected_decisions,
        format!("expected {expected_decisions} explicit native decisions, found {count}"),
    )?;
    Ok(source)
}
fn audit(
    generated: &Path,
    root: &Path,
    snapshot: &Path,
    kernel_count: Option<usize>,
    write_snapshot: bool,
) -> AuditResult<i32> {
    let source = checked_source(generated, if kernel_count.is_some() { 0 } else { 16 })?;
    let theorems = theorem_names(&source)?;
    let expected = if write_snapshot {
        require(!snapshot.exists(), "refusing to overwrite a claim snapshot")?;
        None
    } else {
        Some(read(snapshot)?)
    };
    let expected_names = match &expected {
        Some(surface) => claim_names(surface, false)?,
        None => theorems.clone(),
    };
    let count = kernel_count.unwrap_or(506);
    require(
        expected_names.len() == count,
        format!(
            "claim snapshot must name exactly {count} theorems, found {}",
            expected_names.len()
        ),
    )?;
    same_inventory("generated theorem", &theorems, &expected_names)?;
    let (kernel, native) = if kernel_count.is_some() {
        (theorems.clone(), Names::new())
    } else {
        classify(&theorems)?
    };
    let mut audit_source = source + &format!("\n#eval IO.println \"{BEGIN}\"\n");
    for n in &theorems {
        audit_source += &format!("#check @{n}\n");
    }
    audit_source += &format!("#eval IO.println \"{END}\"\n");
    for n in &theorems {
        audit_source += &format!("#print axioms {n}\n");
    }
    let output = run_lean(root, &audit_source)?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Ok(output.status.code().unwrap_or(1));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let actual = extract_claim_surface(&stdout, BEGIN, END)?;
    same_inventory(
        "generated claim surface",
        &claim_names(&actual, false)?,
        &theorems,
    )?;
    validate_axioms(&parse_generated_axioms(&stdout)?, &kernel, &native)?;
    let expected = match expected {
        Some(expected) => expected,
        None => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(snapshot)
                .map_err(|error| error.to_string())?
                .write_all(actual.as_bytes())
                .map_err(|error| error.to_string())?;
            read(snapshot)?
        }
    };
    validate_surface(&actual, &expected, &theorems)?;
    if kernel_count.is_some() {
        println!(
            "Generated Lean kernel artifact audit passed artifact={} theorems={} claims={}",
            generated.file_stem().unwrap_or_default().to_string_lossy(),
            theorems.len(),
            theorems.len()
        );
    } else {
        println!(
            "Generated Lean axiom audit passed theorems={} kernel={} native={} contracts=48 obligations={} native-decisions=16 claims={}",
            theorems.len(),
            kernel.len(),
            native.len(),
            theorems.len() - 48,
            theorems.len()
        );
    }
    Ok(0)
}
fn self_test() -> AuditResult<()> {
    let mut theorems = exact_certificates();
    theorems.extend(certificates("Projectability"));
    theorems.extend(certificates("VerifiedProtocol"));
    theorems.extend(kernel_production());
    theorems.extend(native_production());
    theorems.extend(rejections());
    theorems.extend(names(&[
        "generatedProjectabilityEnsuresGlobalProgress",
        "generatedVerifiedProtocolAllRolesRefine",
    ]));
    let (kernel, native) = classify(&theorems)?;
    let mut output = String::new();
    for n in &kernel {
        output += &format!("'{n}' does not depend on any axioms\n");
    }
    for n in &native {
        let axioms = expected_owners(n)?
            .iter()
            .map(|o| format!("{o}._native.native_decide.ax_1"))
            .collect::<Vec<_>>()
            .join(", ");
        output += &format!("'{n}' depends on axioms: [{axioms}]\n");
    }
    let parsed = parse_generated_axioms(&output)?;
    validate_axioms(&parsed, &kernel, &native)?;
    let mut contamination = parsed.clone();
    contamination
        .get_mut(kernel.first().unwrap())
        .unwrap()
        .insert("generatedVerifiedProtocolAccepted._native.native_decide.ax_1".into());
    require(
        validate_axioms(&contamination, &kernel, &native).is_err(),
        "accepted native contamination",
    )?;
    let mut wrong = parsed.clone();
    wrong.insert(
        "generatedProjectabilityAccepted".into(),
        names(&["generatedResolvedProjectabilityAccepted._native.native_decide.ax_1"]),
    );
    require(
        validate_axioms(&wrong, &kernel, &native).is_err(),
        "accepted substituted closure",
    )?;
    let mut missing = parsed.clone();
    missing.insert("generatedProjectabilityAccepted".into(), Names::new());
    require(
        validate_axioms(&missing, &kernel, &native).is_err(),
        "accepted missing native boundary",
    )?;
    let claim = extract_claim_surface(
        &format!("noise\n{BEGIN}\nalpha : True\n@beta : False\n{END}\nnoise"),
        BEGIN,
        END,
    )?;
    validate_surface(
        &claim,
        "alpha : True\n@beta : False\n",
        &names(&["alpha", "beta"]),
    )?;
    require(
        validate_surface(
            &claim,
            "alpha : False\n@beta : False\n",
            &names(&["alpha", "beta"]),
        )
        .is_err(),
        "accepted weakened theorem type",
    )?;
    for (code, tokens) in [
        ("example : True := by trivial", &["example"][..]),
        ("private axiom hidden : False", PROOF_TOKENS),
        ("run_cmd synthesizeClaim", GENERATOR_TOKENS),
    ] {
        require(
            forbidden(code, tokens).is_some(),
            "accepted forbidden source token",
        )?;
    }
    let mut unknown = parsed.clone();
    unknown
        .get_mut(kernel.first().unwrap())
        .unwrap()
        .insert("Classical.choice".into());
    require(
        validate_axioms(&unknown, &kernel, &native).is_err(),
        "accepted unknown axiom",
    )?;
    let mut missing_report = parsed.clone();
    missing_report.pop_first();
    require(
        validate_axioms(&missing_report, &kernel, &native).is_err(),
        "accepted missing axiom report",
    )?;
    require(
        parse_generated_axioms(
            "'a' does not depend on any axioms\n'a' does not depend on any axioms\n",
        )
        .is_err(),
        "accepted duplicate report",
    )?;
    require(
        claim_names("a : True\na : True\n", false).is_err(),
        "accepted duplicate claims",
    )?;
    let empty = parse_generated_axioms("'a' depends on axioms: []\n")?;
    require(
        validate_axioms(&empty, &names(&["a"]), &Names::new()).is_err(),
        "accepted empty dependency entry",
    )?;
    for axiom in [
        "a._native.native_decide.ax_",
        "a._native.native_decide.ax_x",
        "a._native.native_decide.ax_1x",
    ] {
        require(
            native_owner(axiom).is_none(),
            "accepted malformed native axiom",
        )?;
    }
    Ok(())
}
fn main_result(args: &[String]) -> AuditResult<i32> {
    match args {
        [flag] if flag == "--self-test" => {
            self_test()?;
            println!("Generated Lean axiom audit self-test passed");
            Ok(0)
        }
        [mode, generated, root, snapshot, count]
            if mode == "--kernel" || mode == "--write-kernel" =>
        {
            audit(
                Path::new(generated),
                Path::new(root),
                Path::new(snapshot),
                Some(
                    count
                        .parse()
                        .map_err(|_| format!("invalid theorem count: {count}"))?,
                ),
                mode == "--write-kernel",
            )
        }
        [generated, root, snapshot] => audit(
            Path::new(generated),
            Path::new(root),
            Path::new(snapshot),
            None,
            false,
        ),
        _ => {
            eprintln!(
                "usage: check-generated-lean-axioms --self-test | GENERATED PROOF_DIR CLAIM_SNAPSHOT | --kernel|--write-kernel GENERATED PROOF_DIR CLAIM_SNAPSHOT EXPECTED_THEOREMS"
            );
            Ok(2)
        }
    }
}
fn main() {
    let code = match main_result(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Generated Lean axiom audit failed: {e}");
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
