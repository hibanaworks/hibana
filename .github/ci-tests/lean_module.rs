//! Check actual compiled declarations, including private and unprinted proofs.
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub fn check(lean: &str, path: &Path, output: Option<&Path>) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut input = String::from("import Lean.Elab.Command\n");
    input.push_str(&source);
    input.push_str(
        r#"
open Lean Elab Command in
run_cmd
  let env ← getEnv
  let mut checked := 0
  for (name, info) in env.constants.toList do
    if (env.getModuleIdxFor? name).isNone then
      if info.isAxiom then
        throwError "Untrusted local axiom: {name}"
      if info.isTheorem then
        checked := checked + 1
        for axiomName in ← Lean.collectAxioms name do
          unless axiomName == `propext || axiomName == `Quot.sound ||
              axiomName == `Classical.choice do
            throwError "Untrusted theorem dependency: {name}: {axiomName}"
  if checked == 0 then
    throwError "No compiled local theorem was checked"
"#,
    );
    let mut command = Command::new(lean);
    command.args(["-j1", "-DwarningAsError=true", "--stdin"]);
    if let Some(output) = output {
        command.arg("-o").arg(output);
    }
    let mut child = command
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("start Lean: {e}"))?;
    // Always reap the compiler, including an early exit while reading input.
    let written = child
        .stdin
        .take()
        .ok_or("missing Lean stdin")?
        .write_all(input.as_bytes());
    let result = child
        .wait_with_output()
        .map_err(|e| format!("wait for Lean: {e}"))?;
    print!("{}", String::from_utf8_lossy(&result.stdout));
    eprint!("{}", String::from_utf8_lossy(&result.stderr));
    if !result.status.success() {
        return Err(format!(
            "Lean rejected {}: {}",
            path.display(),
            result.status
        ));
    }
    written.map_err(|e| format!("write Lean input: {e}"))
}
