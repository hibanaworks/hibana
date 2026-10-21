//! Private acceptance checks for the Lean proof launchers.
mod lean_audit;
use lean_audit::*;
use std::{
    io::{self, Read},
    path::Path,
    process::Command,
};
const TOOLCHAIN: &str = "leanprover/lean4:v4.30.0";
const STATIC_PREFIX: &str = "Lean static theorem audit passed ";
fn markers(kind: &str) -> AuditResult<&'static [(&'static str, &'static str)]> {
    match kind {
        "parallel" => Ok(&[(
            "Generated Lean kernel artifact audit passed artifact=ParallelGenerated theorems=186 claims=186",
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
                "hibana Lean public-operation kernel proof passed states=10 edges=18 transitions=180",
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
/// The theorem inventory owns the exact expected counts and claim types.
/// This boundary checks its successful output, without a second stale census.
fn check_static_marker(output: &str) -> AuditResult<()> {
    let mut lines = output
        .lines()
        .filter(|line| line.starts_with(STATIC_PREFIX));
    let line = lines
        .next()
        .ok_or("Lean proof gate static theorem boundary missing")?;
    require(
        lines.next().is_none(),
        "Lean proof gate duplicate static theorem boundary",
    )?;
    let fields: Vec<_> = line[STATIC_PREFIX.len()..]
        .split_ascii_whitespace()
        .collect();
    require(
        fields.len() == 4,
        "Lean proof gate malformed static theorem totals",
    )?;
    let mut counts = [0usize; 4];
    for ((count, key), field) in counts
        .iter_mut()
        .zip(["theorems", "both", "propext", "free"])
        .zip(fields)
    {
        let prefix = format!("{key}=");
        let digits = field
            .strip_prefix(&prefix)
            .ok_or("Lean proof gate static theorem field mismatch")?;
        require(
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()),
            "Lean proof gate malformed static theorem count",
        )?;
        *count = digits
            .parse()
            .map_err(|_| "Lean proof gate static theorem count overflow")?;
        require(
            count.to_string() == digits,
            "Lean proof gate noncanonical static theorem count",
        )?;
    }
    let covered = counts[1]
        .checked_add(counts[2])
        .and_then(|value| value.checked_add(counts[3]))
        .ok_or("Lean proof gate static theorem total overflow")?;
    require(
        counts[0] > 0 && counts[0] == covered,
        "Lean proof gate inconsistent static theorem totals",
    )
}
fn check_markers(kind: &str, output: &str) -> AuditResult<()> {
    if kind == "static" {
        return check_static_marker(output);
    }
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
    #[test]
    fn static_marker_uses_the_audited_totals() {
        for counts in [
            "theorems=13 both=5 propext=5 free=3",
            "theorems=993 both=588 propext=293 free=112",
        ] {
            assert!(check_markers("static", &format!("{STATIC_PREFIX}{counts}\n")).is_ok());
        }
    }
    #[test]
    fn static_marker_rejects_missing_malformed_duplicate_and_inconsistent_totals() {
        let valid = format!("{STATIC_PREFIX}theorems=13 both=5 propext=5 free=3\n");
        for output in [
            String::new(),
            STATIC_PREFIX.to_owned(),
            format!("{STATIC_PREFIX}theorems=13 both=5 propext=5 free=2\n"),
            format!("{STATIC_PREFIX}theorems=0 both=0 propext=0 free=0\n"),
            format!("{STATIC_PREFIX}theorems=13 both=5 propext=5 free=3 extra=1\n"),
            format!("{STATIC_PREFIX}theorems=13 both=5 propext=5\n"),
            format!("{STATIC_PREFIX}theorems=13 both=5 free=3 propext=5\n"),
            format!("{STATIC_PREFIX}theorems=13 both=05 propext=5 free=3\n"),
            format!("{STATIC_PREFIX}theorems=13 both=+5 propext=5 free=3\n"),
            format!("{STATIC_PREFIX}theorems=13 both=-5 propext=5 free=3\n"),
            format!(
                "{STATIC_PREFIX}theorems={} both={} propext=1 free=0\n",
                usize::MAX,
                usize::MAX
            ),
            format!(
                "{STATIC_PREFIX}theorems={}0 both=5 propext=5 free=3\n",
                usize::MAX
            ),
            format!("{valid}{valid}"),
        ] {
            assert!(
                check_markers("static", &output).is_err(),
                "accepted {output:?}"
            );
        }
    }
}
