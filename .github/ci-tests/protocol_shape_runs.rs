//! Run generated tests in bounded processes and combine every observation.
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const DEFAULT_BATCH: usize = 8192;
const FIRST: &str = "HIBANA_PROTOCOL_SHAPE_SCRIPT_FIRST";
const COUNT: &str = "HIBANA_PROTOCOL_SHAPE_SCRIPT_COUNT";
const OUTPUT: &str = "HIBANA_PROTOCOL_SHAPE_OBSERVATION_DIR";

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

fn binaries(output: &Path, toolchain: &str) -> Result<Vec<PathBuf>> {
    let mut child = Command::new("cargo")
        .arg(format!("+{toolchain}"))
        .args(["test", "--locked", "--manifest-path"])
        .arg(output.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(output.join("target"))
        .args(["--no-run", "--message-format=json"])
        .stdout(Stdio::piped())
        .spawn()?;
    let mut transcript = BufWriter::new(fs::File::create(output.join("test-binaries.jsonl"))?);
    let mut paths = BTreeSet::new();
    let result = (|| -> Result<()> {
        for line in BufReader::new(child.stdout.take().ok_or("missing cargo output")?).lines() {
            let line = line?;
            writeln!(transcript, "{line}")?;
            let value: Value = serde_json::from_str(&line)?;
            if value["reason"] == "compiler-artifact" && value["profile"]["test"] == true {
                let path = value["executable"]
                    .as_str()
                    .ok_or("test has no executable")?;
                if !paths.insert(PathBuf::from(path)) {
                    return Err("duplicate generated test binary".into());
                }
            }
        }
        transcript.flush()?;
        Ok(())
    })();
    let status = child.wait()?;
    result?;
    if !status.success() || paths.is_empty() {
        return Err(format!("cannot enumerate generated test binaries: {status}").into());
    }
    Ok(paths.into_iter().collect())
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

fn run_case(binary: &Path, output: &Path, case: &Case, size: usize) -> Result<()> {
    let name = format!("shape_{}", case.name);
    let parts = fs::canonicalize(output)?
        .join("scope-parts")
        .join(case.name.to_string());
    fs::create_dir_all(&parts)?;
    let observation = output
        .join("scope-observations")
        .join(format!("{}.jsonl", case.name));
    let initial = output
        .join("observations")
        .join(format!("{}.json", case.name));
    let first_initial = parts.join("initial.json");
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
                    .join(format!("{}.json", case.name)),
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
            let name = name.as_str();
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
                .join(format!("{}.json", case.name));
            if range.start == 0 {
                fs::copy(&initial_part, &first_initial)?;
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
    fs::copy(first_initial, initial)?;
    fs::rename(assembled, observation)?;
    println!(
        "{name}: all {} source scripts and all rejection probes completed",
        case.scripts
    );
    Ok(())
}

fn run(output: &Path, toolchain: &str, size: usize) -> Result<()> {
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
    for binary in binaries(output, toolchain)? {
        let listed = Command::new(&binary)
            .args(["--list", "--format", "terse"])
            .output()?;
        if !listed.status.success() {
            return Err("cannot list generated test binary".into());
        }
        let listing = String::from_utf8(listed.stdout)?;
        let mut unique = BTreeSet::new();
        for line in listing.lines().filter(|line| !line.is_empty()) {
            let name = line
                .strip_suffix(": test")
                .ok_or("unexpected generated test listing")?;
            if !unique.insert(name) {
                return Err("duplicate test in executable inventory".into());
            }
            if let Some(case) = cases.get(name) {
                if !seen.insert(name.to_owned()) {
                    return Err("source case appears in multiple binaries".into());
                }
                run_case(&binary, output, case, size)?;
            } else if name.starts_with("shape_") {
                return Err("unknown generated source case".into());
            } else {
                invoke(&binary, name, None)?;
            }
        }
    }
    if seen.len() != cases.len() {
        return Err("missing executable source case".into());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 && args.len() != 3 {
        return Err("usage: protocol-shape-runs OUTPUT TOOLCHAIN [SCRIPTS_PER_PROCESS]".into());
    }
    let size = args
        .get(2)
        .map(|value| -> Result<usize> {
            Ok(value.to_str().ok_or("invalid process size")?.parse()?)
        })
        .transpose()?
        .unwrap_or(DEFAULT_BATCH);
    run(
        Path::new(&args[0]),
        args[1].to_str().ok_or("invalid toolchain")?,
        size,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
