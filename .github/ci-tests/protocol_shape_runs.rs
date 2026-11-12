//! Run generated tests in bounded processes and combine every observation.
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const DEFAULT_BATCH: usize = 8192;
const FIRST: &str = "HIBANA_PROTOCOL_SHAPE_SCRIPT_FIRST";
const COUNT: &str = "HIBANA_PROTOCOL_SHAPE_SCRIPT_COUNT";
const OUTPUT: &str = "HIBANA_PROTOCOL_SHAPE_OBSERVATION_DIR";

#[path = "protocol_shape_measurements.rs"]
mod measurements;
#[path = "protocol_shape_state_data.rs"]
mod state_data;

struct Case {
    name: usize,
    scripts: usize,
}

fn number(value: &Value, field: &str) -> Result<usize> {
    Ok(value[field]
        .as_u64()
        .ok_or("missing script inventory number")?
        .try_into()?)
}

fn ranges(total: usize, size: usize) -> Result<Vec<core::ops::Range<usize>>> {
    if total == 0 || size == 0 {
        return Err("script inventory and process size must be positive".into());
    }
    Ok((0..total)
        .step_by(size)
        .map(|start| start..start.saturating_add(size).min(total))
        .collect())
}

fn invoke(
    binary: &Path,
    name: &str,
    part: Option<(&core::ops::Range<usize>, &Path)>,
) -> Result<()> {
    let configured = env::var_os("HIBANA_PROTOCOL_SHAPE_TIMEOUT_SECONDS");
    let seconds = match configured.as_ref() {
        None => "3600",
        Some(value) => value.to_str().ok_or("non-Unicode test time limit")?,
    }
    .parse::<u64>()?;
    if seconds == 0 {
        return Err("test process time limit must be positive".into());
    }
    let mut command = Command::new("timeout");
    command
        .args(["--kill-after=10s", &format!("{seconds}s")])
        .arg(binary)
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env_remove(FIRST)
        .env_remove(COUNT)
        .env_remove(OUTPUT);
    if let Some((range, output)) = part {
        println!(
            "{name}: rejection probes for scripts {}..{}; full positive baselines",
            range.start, range.end
        );
        command
            .env(FIRST, range.start.to_string())
            .env(COUNT, range.len().to_string())
            .env(OUTPUT, output);
    }
    let status = command.status()?;
    if !status.success() {
        return Err(format!("generated test {name} failed: {status}").into());
    }
    if let Some((range, _)) = part {
        println!(
            "{name}: completed rejection probes for scripts {}..{}",
            range.start, range.end
        );
    }
    Ok(())
}

fn binaries(output: &Path, manifest: &Path, transcript: &Path) -> Result<Vec<PathBuf>> {
    let manifest = manifest.canonicalize()?;
    let target = output.join("target").canonicalize()?;
    let mut paths = BTreeSet::new();
    for line in BufReader::new(fs::File::open(transcript)?).lines() {
        let value: Value = serde_json::from_str(&line?)?;
        if value["reason"] == "compiler-artifact" && value["profile"]["test"] == true {
            let owner = value["manifest_path"]
                .as_str()
                .ok_or("missing artifact manifest")?;
            if Path::new(owner).canonicalize()? != manifest {
                return Err("foreign generated test artifact".into());
            }
            let path = PathBuf::from(
                value["executable"]
                    .as_str()
                    .ok_or("test has no executable")?,
            )
            .canonicalize()?;
            if !path.is_file() || !path.starts_with(&target) {
                return Err("generated test binary is outside its target directory".into());
            }
            if !paths.insert(path) {
                return Err("duplicate generated test binary".into());
            }
        }
    }
    if paths.is_empty() {
        return Err("empty generated test artifact inventory".into());
    }
    Ok(paths.into_iter().collect())
}

fn compile(root: &Path, output: &Path, input: &Path, unit: usize) -> Result<PathBuf> {
    let transcript = output.join(format!("native-compile-{unit}.jsonl"));
    let manifest = root.join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .arg(format!(
            "+{}",
            env::var("TOOLCHAIN").unwrap_or("stable".into())
        ))
        .args([
            "test",
            "--locked",
            "--lib",
            "--no-run",
            "--message-format=json",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(output.join("target"))
        .env("HIBANA_GENERATED_PROTOCOL_SHAPES", input)
        .env("CARGO_PROFILE_TEST_OPT_LEVEL", "1")
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_DEBUG_ASSERTIONS", "true")
        .env("CARGO_PROFILE_TEST_OVERFLOW_CHECKS", "true")
        .env("CARGO_PROFILE_TEST_INCREMENTAL", "false")
        .stdin(Stdio::null())
        .stdout(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&transcript)?,
        );
    if let Some(flags) = env::var_os("CARGO_ENCODED_RUSTFLAGS") {
        let mut flags = flags;
        flags.push("\x1f--cfg\x1fhibana_generated_protocols");
        command.env("CARGO_ENCODED_RUSTFLAGS", flags);
    } else {
        let mut flags = env::var_os("RUSTFLAGS").unwrap_or_default();
        flags.push(" --cfg hibana_generated_protocols");
        command.env("RUSTFLAGS", flags);
    }
    if !command.status()?.success() {
        return Err(format!("generated native input {unit} failed to compile").into());
    }
    let paths = binaries(output, &manifest, &transcript)?;
    let [binary] = paths.as_slice() else {
        return Err("one canonical library test binary is required per generated input".into());
    };
    Ok(binary.clone())
}

fn file_hash(path: &Path) -> Result<String> {
    let result = Command::new("shasum")
        .args(["-a", "256", "--"])
        .arg(path)
        .output()?;
    if !result.status.success() {
        return Err("cannot hash a generated source or executable".into());
    }
    Ok(String::from_utf8(result.stdout)?)
}

fn same_bytes(first: &Path, second: &Path) -> Result<bool> {
    let mut first = BufReader::new(fs::File::open(first)?);
    let mut second = BufReader::new(fs::File::open(second)?);
    loop {
        let a = first.fill_buf()?;
        let b = second.fill_buf()?;
        let width = a.len().min(b.len());
        if width == 0 {
            return Ok(a.is_empty() && b.is_empty());
        }
        if a[..width] != b[..width] {
            return Ok(false);
        }
        first.consume(width);
        second.consume(width);
    }
}

fn merge_record(line: &str, script: usize, reverse: bool, output: &mut impl Write) -> Result<()> {
    let value: Value = serde_json::from_str(line)?;
    if value
        .as_object()
        .ok_or("expected script observation")?
        .len()
        != 4
        || number(&value, "script")? != script
        || value["reversePoll"] != reverse
        || !value["completed"].is_array()
        || !value["probes"].is_array()
    {
        return Err("missing, duplicate or reordered script observation".into());
    }
    writeln!(output, "{line}")?;
    Ok(())
}

fn run_case(binary: &Path, name: &str, output: &Path, case: &Case, size: usize) -> Result<()> {
    let parts = fs::canonicalize(output)?
        .join("scope-parts")
        .join(case.name.to_string());
    fs::create_dir_all(&parts)?;
    let observation = output
        .join("scope-observations")
        .join(format!("{}.jsonl", case.name));
    let initial = output
        .join("observations")
        .join(format!("{}.jsonl", case.name));
    let first_initial = parts.join("initial.jsonl");
    let assembled = parts.join("assembled.jsonl");
    for file in [&observation, &initial] {
        fs::create_dir_all(file.parent().ok_or("observation has no parent directory")?)?;
        if file.exists() {
            fs::remove_file(file)?;
        }
    }
    let mut writer = BufWriter::new(fs::File::create(&assembled)?);
    let header = serde_json::json!({"name":case.name,"scripts":case.scripts});
    writeln!(writer, "{header}")?;
    let mut completed = 0;
    let parts = ranges(case.scripts, size)?
        .into_iter()
        .map(|range| {
            let directory = parts.join(format!("{}-{}", range.start, range.len()));
            (range, directory)
        })
        .collect::<Vec<_>>();
    let workers = std::thread::available_parallelism()?.get().min(parts.len());
    println!(
        "{name}: {} script partitions, up to {workers} concurrent processes",
        parts.len()
    );
    for batch in parts.chunks(workers) {
        for (_, directory) in batch {
            for file in [
                directory
                    .join("observations")
                    .join(format!("{}.jsonl", case.name)),
                directory
                    .join("scope-observations")
                    .join(format!("{}.jsonl", case.name)),
            ] {
                if file.exists() {
                    fs::remove_file(file)?;
                }
            }
        }
        std::thread::scope(|scope| -> Result<()> {
            let mut children = Vec::new();
            for (range, directory) in batch {
                children.push(scope.spawn(move || {
                    invoke(binary, name, Some((range, directory)))
                        .map_err(|error| error.to_string())
                }));
            }
            for child in children {
                child
                    .join()
                    .map_err(|_| "generated test worker panicked")??;
            }
            Ok(())
        })?;
        for (range, directory) in batch {
            let initial_part = directory
                .join("observations")
                .join(format!("{}.jsonl", case.name));
            if range.start == 0 {
                fs::rename(&initial_part, &first_initial)?;
            } else if !same_bytes(&initial_part, &first_initial)? {
                return Err(
                    "initial or continuation observations changed between partitions".into(),
                );
            }
            let part = directory
                .join("scope-observations")
                .join(format!("{}.jsonl", case.name));
            let mut lines = BufReader::new(fs::File::open(&part)?).lines();
            let observed_header: Value =
                serde_json::from_str(&lines.next().ok_or("missing partition header")??)?;
            if observed_header != header || range.start != completed {
                return Err("wrong partition header or script interval".into());
            }
            for script in range.clone() {
                for reverse in [false, true] {
                    merge_record(
                        &lines.next().ok_or("missing partition observation")??,
                        script,
                        reverse,
                        &mut writer,
                    )?;
                }
            }
            if lines.next().is_some() {
                return Err("extra partition observation".into());
            }
            drop(lines);
            writer.flush()?;
            fs::remove_file(part)?;
            completed = range.end;
        }
    }
    if completed != case.scripts {
        return Err("incomplete generated script execution".into());
    }
    writer.flush()?;
    drop(writer);
    fs::rename(&first_initial, initial)?;
    fs::rename(assembled, observation)?;
    measurements::assemble(output, case, &parts)?;
    let whole_states = output.join("whole-operation-states");
    fs::create_dir_all(&whole_states)?;
    let filename = format!("{}.jsonl", case.name);
    let destination = whole_states.join(&filename);
    for (_, part) in &parts {
        let source = part.join("whole-operation-states").join(&filename);
        if destination.try_exists()? {
            if !same_bytes(&source, &destination)? {
                return Err("whole-call cuts changed between script partitions".into());
            }
        } else {
            fs::copy(source, &destination)?;
        }
    }
    fs::remove_dir_all(
        first_initial
            .parent()
            .ok_or("missing partition directory")?,
    )?;
    println!(
        "{name}: all {} source scripts and all rejection probes completed",
        case.scripts
    );
    Ok(())
}

fn run(root: &Path, output: &Path, size: usize) -> Result<()> {
    let root = root.canonicalize()?;
    let output = output.canonicalize()?;
    let output = output.as_path();
    let entries: Vec<Value> =
        serde_json::from_reader(fs::File::open(output.join("scope-inputs.json"))?)?;
    let mut cases = BTreeMap::new();
    for entry in entries {
        let case = Case {
            name: number(&entry, "name")?,
            scripts: number(&entry, "scripts")?,
        };
        ranges(case.scripts, size)?;
        if cases.insert(format!("shape_{}", case.name), case).is_some() {
            return Err("duplicate source case".into());
        }
    }
    if cases.is_empty() {
        return Err("empty generated execution inventory".into());
    }
    let mut seen = BTreeSet::new();
    let inputs: Vec<PathBuf> =
        serde_json::from_reader(fs::File::open(output.join("native-inputs.json"))?)?;
    let mut unique_inputs = BTreeSet::new();
    let mut compilation = Duration::ZERO;
    let mut execution = Duration::ZERO;
    let mut receipts = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("native-builds.jsonl"))?,
    );
    if inputs.is_empty() {
        return Err("empty native compilation inventory".into());
    }
    for (unit, input) in inputs.into_iter().enumerate() {
        let input = output.join(input).canonicalize()?;
        if !input.starts_with(output.join("src")) || !unique_inputs.insert(input.clone()) {
            return Err("foreign or duplicated native source input".into());
        }
        let input_before = file_hash(&input)?;
        let started = Instant::now();
        let binary = compile(&root, output, &input, unit)?;
        compilation += started.elapsed();
        let binary_before = file_hash(&binary)?;
        let started = Instant::now();
        let listed = Command::new(&binary)
            .args(["--list", "--format", "terse"])
            .output()?;
        if !listed.status.success() {
            return Err("cannot list generated test binary".into());
        }
        let listing = String::from_utf8(listed.stdout)?;
        let mut unique = BTreeSet::new();
        for line in listing.lines().filter(|line| !line.is_empty()) {
            let qualified = line
                .strip_suffix(": test")
                .ok_or("unexpected generated test listing")?;
            let Some(name) = qualified.strip_prefix("endpoint::generated_protocol_shapes::") else {
                // Ordinary core tests have their own required native gate.
                continue;
            };
            if !unique.insert(name) {
                return Err("duplicate test in executable inventory".into());
            }
            if let Some(case) = cases.get(name) {
                if !seen.insert(name.to_owned()) {
                    return Err("source case appears in multiple binaries".into());
                }
                run_case(&binary, qualified, output, case, size)?;
            } else if name.starts_with("shape_") {
                return Err("unknown generated source case".into());
            } else {
                invoke(&binary, qualified, None)?;
            }
        }
        execution += started.elapsed();
        if file_hash(&input)? != input_before || file_hash(&binary)? != binary_before {
            return Err("generated source or executable changed during its validation".into());
        }
        writeln!(
            receipts,
            "{}",
            serde_json::json!({
                "unit":unit,"input":input,"input_hash":input_before,
                "binary":binary,"binary_hash":binary_before,
            })
        )?;
        receipts.flush()?;
    }
    if seen.len() != cases.len() {
        return Err("missing executable source case".into());
    }
    fs::write(
        output.join("compile.time"),
        format!("real {}\n", compilation.as_secs_f64()),
    )?;
    fs::write(
        output.join("execute.time"),
        format!("real {}\n", execution.as_secs_f64()),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 && args.len() != 3 {
        return Err("usage: protocol-shape-runs CORE OUTPUT [SCRIPTS_PER_PROCESS]".into());
    }
    let size = args
        .get(2)
        .map(|value| -> Result<usize> {
            Ok(value.to_str().ok_or("invalid process size")?.parse()?)
        })
        .transpose()?
        .unwrap_or(DEFAULT_BATCH);
    run(Path::new(&args[0]), Path::new(&args[1]), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_transcript_requires_unique_binaries_from_the_selected_source() {
        let root = env::temp_dir().join(format!("hibana-shape-artifacts-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let manifest = root.join("Cargo.toml");
        fs::write(&manifest, "").unwrap();
        let executable = target.join("shapes");
        fs::write(&executable, "").unwrap();
        let other_manifest = root.join("foreign.toml");
        fs::write(&other_manifest, "").unwrap();
        let foreign_binary = root.join("foreign-shapes");
        fs::write(&foreign_binary, "").unwrap();
        let mut record = serde_json::json!({
            "reason":"compiler-artifact", "profile":{"test":true},
            "manifest_path":manifest, "executable":executable,
        });
        let transcript = root.join("test-binaries.jsonl");
        fs::write(&transcript, record.to_string()).unwrap();
        assert_eq!(
            binaries(&root, &manifest, &transcript).unwrap(),
            [executable.canonicalize().unwrap()]
        );
        record["manifest_path"] = serde_json::json!(other_manifest);
        fs::write(&transcript, record.to_string()).unwrap();
        assert!(binaries(&root, &manifest, &transcript).is_err());
        record["manifest_path"] = serde_json::json!(manifest);
        record["executable"] = serde_json::json!(foreign_binary);
        fs::write(&transcript, record.to_string()).unwrap();
        assert!(binaries(&root, &manifest, &transcript).is_err());
        record["executable"] = serde_json::json!(executable);
        fs::write(&transcript, format!("{record}\n{record}\n")).unwrap();
        assert!(binaries(&root, &manifest, &transcript).is_err());
        record["executable"] = Value::Null;
        fs::write(&transcript, record.to_string()).unwrap();
        assert!(binaries(&root, &manifest, &transcript).is_err());
        fs::write(&transcript, "").unwrap();
        assert!(binaries(&root, &manifest, &transcript).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_intervals_keep_every_script_exactly_once() {
        for size in [1, 3, 8192] {
            for total in [1, 8191, 8192, 8193, 65_536, 65_537] {
                let actual = ranges(total, size)
                    .unwrap()
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                assert_eq!(actual, (0..total).collect::<Vec<_>>());
            }
        }
        assert!(ranges(0, 1).is_err());
        assert!(ranges(1, 0).is_err());
    }

    #[test]
    fn assembly_requires_both_schedules_at_the_exact_source_index() {
        let record = r#"{"script":4,"reversePoll":false,"completed":[],"probes":[]}"#;
        merge_record(record, 4, false, &mut Vec::new()).unwrap();
        assert!(merge_record(record, 3, false, &mut Vec::new()).is_err());
        assert!(merge_record(record, 4, true, &mut Vec::new()).is_err());
        let extra = r#"{"script":4,"reversePoll":false,"completed":[],"probes":[],"extra":0}"#;
        assert!(merge_record(extra, 4, false, &mut Vec::new()).is_err());
    }
}
