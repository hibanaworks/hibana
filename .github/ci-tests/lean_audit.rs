//! Private, fail-closed helpers for the repository's Lean proof gates.
#![allow(dead_code)]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
pub type Names = BTreeSet<String>;
pub type AuditResult<T> = Result<T, String>;
pub fn read(path: &Path) -> AuditResult<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn names(items: &[&str]) -> Names {
    items.iter().map(|s| (*s).into()).collect()
}
pub fn require(ok: bool, message: impl Into<String>) -> AuditResult<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
pub fn same_inventory(label: &str, actual: &Names, expected: &Names) -> AuditResult<()> {
    require(
        actual == expected,
        format!(
            "{label} inventory changed: missing={:?} unexpected={:?}",
            expected.difference(actual).collect::<Vec<_>>(),
            actual.difference(expected).collect::<Vec<_>>()
        ),
    )
}
/// Preserve every byte offset and newline, including inside Unicode comments.
pub fn erase_non_code(source: &str) -> AuditResult<String> {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    let mut depth = 0;
    let mut line = false;
    let mut string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let pair = bytes.get(i..i + 2);
        let c = bytes[i];
        if line {
            if c == b'\n' {
                line = false;
            } else {
                out[i] = b' ';
            }
            i += 1;
            continue;
        }
        if depth > 0 {
            if pair == Some(b"/-") {
                out[i] = b' ';
                out[i + 1] = b' ';
                depth += 1;
                i += 2;
            } else if pair == Some(b"-/") {
                out[i] = b' ';
                out[i + 1] = b' ';
                depth -= 1;
                i += 2;
            } else {
                if c != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            continue;
        }
        if string {
            if c != b'\n' {
                out[i] = b' ';
            }
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                string = false;
            }
            i += 1;
            continue;
        }
        if pair == Some(b"--") {
            out[i] = b' ';
            out[i + 1] = b' ';
            line = true;
            i += 2;
        } else if pair == Some(b"/-") {
            out[i] = b' ';
            out[i + 1] = b' ';
            depth = 1;
            i += 2;
        } else if c == b'"' {
            out[i] = b' ';
            string = true;
            i += 1;
        } else {
            i += 1;
        }
    }
    require(depth == 0 && !string, "unterminated Lean comment or string")?;
    String::from_utf8(out).map_err(|e| e.to_string())
}
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
pub fn token_positions(code: &str, token: &str) -> Vec<usize> {
    code.match_indices(token)
        .filter(|(i, _)| {
            code[..*i].chars().next_back().is_none_or(|c| !word(c))
                && code[*i + token.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !word(c))
        })
        .map(|(i, _)| i)
        .collect()
}
pub fn forbidden<'a>(code: &str, tokens: &'a [&str]) -> Option<&'a str> {
    tokens
        .iter()
        .copied()
        .find(|token| !token_positions(code, token).is_empty())
}
pub const PROOF_TOKENS: &[&str] = &["sorry", "admit", "axiom", "constant", "opaque", "unsafe"];
pub const GENERATOR_TOKENS: &[&str] = &[
    "macro",
    "macro_rules",
    "syntax",
    "elab",
    "run_cmd",
    "run_tac",
];
pub fn theorem_names(source: &str) -> AuditResult<Names> {
    let code = erase_non_code(source)?;
    let mut result = Names::new();
    for token in ["theorem", "lemma"] {
        for i in token_positions(&code, token) {
            let suffix = &code[i + token.len()..];
            if !suffix.chars().next().is_some_and(char::is_whitespace) {
                continue;
            }
            if code[..i].split_whitespace().next_back() == Some("private") {
                continue;
            }
            let name = suffix
                .trim_start()
                .split(|c: char| c.is_whitespace() || ":({".contains(c))
                .next()
                .unwrap_or("");
            if name.is_empty() {
                continue;
            }
            require(
                !name.starts_with('«'),
                format!("escaped theorem names are not inventory-safe: {name}"),
            )?;
            result.insert(name.to_owned());
        }
    }
    Ok(result)
}
pub fn lean_files(root: &Path) -> AuditResult<Vec<PathBuf>> {
    fn walk(root: &Path, out: &mut Vec<PathBuf>) -> AuditResult<()> {
        for entry in fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "lean") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, &mut out)?;
    out.sort();
    Ok(out)
}
pub fn extract_claim_surface(output: &str, begin: &str, end: &str) -> AuditResult<String> {
    let begin = format!("{begin}\n");
    let end = format!("\n{end}");
    require(
        output.matches(&begin).count() == 1 && output.matches(&end).count() == 1,
        "claim surface markers are missing or duplicated",
    )?;
    let tail = output.split_once(&begin).unwrap().1;
    let surface = tail
        .split_once(&end)
        .ok_or("claim surface end marker precedes begin marker")?
        .0;
    Ok(format!("{surface}\n"))
}
pub fn claim_names(surface: &str, qualified: bool) -> AuditResult<Names> {
    let mut result = Names::new();
    for line in surface.lines() {
        let line = line.strip_prefix('@').unwrap_or(line);
        let line = if qualified {
            match line.strip_prefix("Hibana.") {
                Some(s) => s,
                None => continue,
            }
        } else {
            line
        };
        let Some((name, _)) = line.split_once(" :") else {
            continue;
        };
        let valid = if qualified {
            !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c == ':')
        } else {
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'')
        };
        if valid {
            require(
                result.insert(name.to_string()),
                "claim surface contains duplicate theorem headers",
            )?;
        }
    }
    Ok(result)
}
pub fn parse_axioms(output: &str) -> AuditResult<BTreeMap<String, Names>> {
    parse_axiom_blocks(output, false)
}
pub fn parse_generated_axioms(output: &str) -> AuditResult<BTreeMap<String, Names>> {
    parse_axiom_blocks(output, true)
}
fn parse_axiom_blocks(output: &str, keep_empty: bool) -> AuditResult<BTreeMap<String, Names>> {
    let mut parsed = BTreeMap::new();
    for (offset, _) in output
        .match_indices('\'')
        .filter(|(i, _)| *i == 0 || output.as_bytes()[*i - 1] == b'\n')
    {
        let rest = &output[offset + 1..];
        let first_line = rest.split('\n').next().unwrap();
        let free = "' does not depend on any axioms";
        let dependent = "' depends on axioms: [";
        let (name, axioms, tail) = if let Some(pos) = first_line.rfind(free) {
            (&rest[..pos], Names::new(), &rest[pos + free.len()..])
        } else if let Some(pos) = first_line.rfind(dependent) {
            let body = &rest[pos + dependent.len()..];
            let Some(end) = body.match_indices(']').find_map(|(i, _)| {
                let tail = &body[i + 1..];
                (tail.starts_with("\n'") || tail.trim().is_empty()).then_some(i)
            }) else {
                continue;
            };
            (
                &rest[..pos],
                body[..end]
                    .split(',')
                    .map(str::trim)
                    .filter(|s| keep_empty || !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                &body[end + 1..],
            )
        } else {
            continue;
        };
        if !tail.starts_with("\n'") && !tail.trim().is_empty() {
            continue;
        }
        require(
            parsed.insert(name.into(), axioms).is_none(),
            format!("duplicate axiom report: {name}"),
        )?;
    }
    Ok(parsed)
}
pub fn native_owner(axiom: &str) -> Option<&str> {
    let (owner, suffix) = axiom.rsplit_once("._native.native_decide.ax_")?;
    (!owner.is_empty()
        && !suffix.is_empty()
        && suffix.bytes().all(|c| c.is_ascii_digit() || c == b'_'))
    .then_some(owner)
}
pub fn allowed_axiom(axiom: &str) -> Option<&str> {
    let base = if let Some((base, universe)) = axiom.split_once(".{") {
        if !universe.ends_with('}')
            || universe.len() < 2
            || universe[..universe.len() - 1].contains('}')
        {
            return None;
        }
        base
    } else {
        axiom
    };
    matches!(base, "propext" | "Quot.sound").then_some(base)
}
pub fn run_lean(root: &Path, source: &str) -> AuditResult<Output> {
    let mut child = Command::new("lake")
        // One worker can still queue asynchronous declaration elaboration.
        // Check complete declarations synchronously without retaining that queue.
        .args([
            "env",
            "lean",
            "-j",
            "1",
            "-D",
            "Elab.async=false",
            "--stdin",
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("launch Lean: {e}"))?;
    // Drain stdout/stderr while writing: a large audit can fill either pipe.
    let mut stdin = child.stdin.take().ok_or("Lean stdin unavailable")?;
    let input = source.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let written = writer.join().map_err(|_| "Lean stdin writer panicked")?;
    if output.status.success() {
        written.map_err(|e| format!("write Lean input: {e}"))?;
    }
    Ok(output)
}
pub fn successful_lean(root: &Path, source: &str) -> AuditResult<String> {
    let output = run_lean(root, source)?;
    if !output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(format!("Lean failed to elaborate audit: {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
pub fn check_surface(actual: &str, expected: &str) -> AuditResult<()> {
    if actual == expected {
        return Ok(());
    }
    let first = actual
        .lines()
        .zip(expected.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| actual.lines().count().min(expected.lines().count()));
    Err(format!(
        "Lean theorem type surface changed at line {}:\nexpected: {}\nactual: {}",
        first + 1,
        expected.lines().nth(first).unwrap_or("<end>"),
        actual.lines().nth(first).unwrap_or("<end>")
    ))
}
