//! Private acceptance checks for the Lean proof launchers.
mod lean_audit;
use lean_audit::*;
use std::{
    io::{self, Read},
    path::Path,
    process::Command,
};
const TOOLCHAIN: &str = "leanprover/lean4:v4.30.0";
fn markers(kind: &str) -> AuditResult<&'static [(&'static str, &'static str)]> {
    match kind {
        "static" => Ok(&[(
            "Lean static theorem audit passed theorems=709 both=364 propext=248 free=97",
            "static theorem boundary",
        )]),
        "parallel" => Ok(&[(
            "Generated Lean kernel artifact audit passed artifact=ParallelGenerated theorems=182 claims=182",
            "parallel descriptor correspondence",
        )]),
        "causal" => Ok(&[(
            "Generated Lean kernel artifact audit passed artifact=CausalGenerated theorems=36 claims=36",
            "causal correspondence boundary",
        )]),
        "generated" => Ok(&[
            (
                "hibana Lean generated certificate check passed traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8",
                "generated certificate",
            ),
            (
                "hibana Lean production artifact passed transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing",
                "production evidence",
            ),
            (
                "Generated Lean axiom audit passed theorems=506 kernel=466 native=40 contracts=48 obligations=458 native-decisions=16 claims=506",
                "generated axiom boundary",
            ),
        ]),
        "runtime" => Ok(&[
            (
                "hibana Lean runtime proof passed regions=4 poison=1 generation=1 atomic-failures=4",
                "runtime certificate",
            ),
            (
                "Generated Lean kernel artifact audit passed artifact=RuntimeGenerated theorems=16 claims=16",
                "runtime theorem boundary",
            ),
        ]),
        "public-operation" => Ok(&[
            (
                "hibana Lean public-operation kernel proof passed states=9 edges=16 transitions=144",
                "public-operation certificate",
            ),
            (
                "Generated Lean kernel artifact audit passed artifact=PublicOperationGenerated theorems=2 claims=2",
                "public-operation theorem boundary",
            ),
        ]),
        _ => Err(format!("unknown Lean audit marker group: {kind}")),
    }
}
fn check_markers(kind: &str, output: &str) -> AuditResult<()> {
    for (marker, label) in markers(kind)? {
        require(
            output.contains(marker),
            format!("Lean proof gate {label} mismatch"),
        )?;
    }
    Ok(())
}
fn check_toolchain(root: &Path) -> AuditResult<()> {
    require(
        read(&root.join("lean-toolchain"))?.trim_end_matches('\n') == TOOLCHAIN,
        format!("Lean proof gate requires pinned toolchain {TOOLCHAIN}"),
    )?;
    let output = Command::new("lake")
        .args(["env", "lean", "--version"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Lean proof gate requires lake from pinned elan: {e}"))?;
    require(
        output.status.success(),
        format!(
            "Lean version query failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    require(
        String::from_utf8_lossy(&output.stdout).contains("version 4.30.0"),
        "Lean proof gate toolchain mismatch",
    )
}
fn raw_forbidden(line: &str) -> bool {
    // Conservatively treat non-ASCII continuations as boundaries. This avoids
    // weakening the former Unicode regex policy without a regex dependency.
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    ["sorry", "admit", "axiom", "native_decide", "unsafe"]
        .iter()
        .any(|token| {
            line.match_indices(token).any(|(offset, _)| {
                line[..offset].chars().next_back().is_none_or(|c| !word(c))
                    && line[offset + token.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !word(c))
            })
        })
}
fn check_sources(label: &str, paths: &[String]) -> AuditResult<()> {
    require(
        !paths.is_empty(),
        "proof source audit requires at least one file",
    )?;
    let mut violations = Vec::new();
    for path in paths {
        let source = read(Path::new(path))?;
        // The supplemental gate deliberately scans comments and strings too,
        // retaining its former whole-file forbidden-token policy.
        for (line_no, line) in source.lines().enumerate() {
            if raw_forbidden(line) {
                violations.push(format!("{path}:{}:{line}", line_no + 1));
            }
        }
    }
    require(
        violations.is_empty(),
        format!(
            "{}\nUntrusted proof declaration: {label}",
            violations.join("\n")
        ),
    )
}
fn run(args: &[String]) -> AuditResult<()> {
    match args{
        [mode,root]if mode=="toolchain"=>check_toolchain(Path::new(root)),
        [mode,kind]if mode=="markers"=>{let mut output=String::new();io::stdin().read_to_string(&mut output).map_err(|e|e.to_string())?;check_markers(kind,&output)},
        [mode,path,label]if mode=="artifact"=>{let metadata=std::fs::metadata(path).map_err(|e|format!("{path}: {e}"))?;require(metadata.is_file() && metadata.len()>0,format!("Lean proof gate {label} exporter did not create a nonempty artifact"))},
        [mode,label,paths @ ..]if mode=="sources"=>check_sources(label,paths),
        _=>Err("usage: check-lean-proof-guard toolchain PROOF_DIR | markers KIND | artifact PATH LABEL | sources LABEL FILE...".into())
    }
}
fn main() {
    if let Err(e) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_source_policy_does_not_erase_comments_or_unicode_boundaries() {
        for text in [
            "private axiom hidden : False",
            "-- sorry",
            "\"native_decide\"",
            "axiom²",
            "axiom一",
        ] {
            assert!(raw_forbidden(text), "accepted {text}");
        }
        for text in [
            "def safe : Nat := 0",
            "unsafe_name",
            "pre_axiom",
            "admitted",
            "sorryish",
        ] {
            assert!(!raw_forbidden(text), "rejected {text}");
        }
    }
    #[test]
    fn every_marker_is_required() {
        for kind in [
            "static",
            "parallel",
            "causal",
            "generated",
            "runtime",
            "public-operation",
        ] {
            let all = markers(kind)
                .unwrap()
                .iter()
                .map(|(marker, _)| *marker)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(check_markers(kind, &all).is_ok());
            assert!(check_markers(kind, "").is_err());
            for (marker, _) in markers(kind).unwrap() {
                assert!(check_markers(kind, &all.replace(marker, "")).is_err());
                assert!(check_markers(kind, &all.replace(marker, "changed marker")).is_err());
            }
        }
        assert!(check_markers("unknown", "").is_err());
    }
}
