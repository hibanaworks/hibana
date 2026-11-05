//! Corrupt result data to test the compiled shape checker, not Rust source text.
#[path = "kani_json.rs"]
mod json;
#[path = "protocol_shape_roll_coverage.rs"]
mod roll_coverage;
#[path = "protocol_shape_scope_audit.rs"]
mod scope_audit;
use json::Json;
use std::{
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
};

type Error = Box<dyn std::error::Error>;

// Only a header or one history group is decoded at a time. Mutations preserve
// all other records byte-for-byte and replace the target inode, never a shared
// immutable baseline linked into another control directory.
fn rewrite_records(
    path: &Path,
    mut change: impl FnMut(usize, &str) -> Result<Vec<String>, Error>,
) -> Result<(), Error> {
    let temporary = path.with_extension("new");
    let mut writer = BufWriter::new(fs::File::create(&temporary)?);
    for (index, line) in BufReader::new(fs::File::open(path)?).lines().enumerate() {
        for record in change(index, &line?)? {
            writeln!(writer, "{record}")?;
        }
    }
    writer.flush()?;
    drop(writer);
    fs::rename(temporary, path)?;
    Ok(())
}

fn answers(group: &Json) -> Result<&Vec<Json>, Error> {
    match group.get("answers") {
        Some(Json::Array(rows)) => Ok(rows),
        _ => Err("missing observation answer matrix".into()),
    }
}

fn admission(row: &Json) -> Result<bool, Error> {
    match row {
        Json::Array(fields) if fields.len() == 2 => match &fields[0] {
            Json::Bool(accepted) => Ok(*accepted),
            _ => Err("invalid admission answer".into()),
        },
        _ => Err("invalid observation answer".into()),
    }
}

fn continuation(group: &Json) -> Result<bool, Error> {
    match group.get("prefix") {
        Some(Json::Array(word)) => Ok(!word.is_empty()),
        _ => Err("missing observation prefix".into()),
    }
}

fn outcome_witness(
    path: &Path,
    continued: bool,
    accepted: bool,
) -> Result<Option<(usize, usize)>, Error> {
    for (index, line) in BufReader::new(fs::File::open(path)?)
        .lines()
        .enumerate()
        .skip(1)
    {
        let group = json::parse(&line?)?;
        if continued && !continuation(&group)? {
            continue;
        }
        for (row, answer) in answers(&group)?.iter().enumerate() {
            if admission(answer)? == accepted {
                return Ok(Some((index, row)));
            }
        }
    }
    Ok(None)
}

// One source word can have several recorded resolver histories. Compare full
// admission rows, then omit only an outcome-distinguishing history group.
fn missing_history(path: &Path) -> Result<Option<usize>, Error> {
    let mut seen = std::collections::BTreeMap::new();
    for (index, line) in BufReader::new(fs::File::open(path)?)
        .lines()
        .enumerate()
        .skip(1)
    {
        let group = json::parse(&line?)?;
        if !continuation(&group)? {
            continue;
        }
        let word = source_json(group.get("prefix").ok_or("missing prefix")?)?;
        let history = source_json(group.get("prefixChoices").ok_or("missing history")?)?;
        let row = answers(&group)?
            .iter()
            .map(admission)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some((previous, answer)) = seen.get(&word) {
            if previous != &history && answer != &row {
                return Ok(Some(index));
            }
        } else {
            seen.insert(word, (history, row));
        }
    }
    Ok(None)
}

// Probe kinds are the only string-valued fields in these test records.
fn source_json(value: &Json) -> Result<String, Box<dyn std::error::Error>> {
    Ok(match value {
        Json::Number(number) => number.clone(),
        Json::Bool(value) => value.to_string(),
        Json::String(kind) if kind == "borrow" || kind == "causal" => format!("\"{kind}\""),
        Json::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(source_json)
                .collect::<Result<Vec<_>, _>>()?
                .join(",")
        ),
        Json::Object(fields) => {
            let mut encoded = Vec::new();
            for (key, value) in fields {
                if !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    return Err("invalid source field name".into());
                }
                encoded.push(format!("\"{key}\":{}", source_json(value)?));
            }
            format!("{{{}}}", encoded.join(","))
        }
        Json::String(_) | Json::Null => return Err("unsupported source field value".into()),
    })
}

fn single_case(
    inputs: &Json,
    target: &Path,
    observation: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let Json::Array(cases) = inputs else {
        return Err("invalid source inventory".into());
    };
    let name = observation
        .file_stem()
        .ok_or("missing case name")?
        .to_str()
        .ok_or("invalid case name")?;
    let matching = cases
        .iter()
        .filter(|case| case.get("name") == Some(&Json::Number(name.into())))
        .collect::<Vec<_>>();
    let [case] = matching.as_slice() else {
        return Err("missing or duplicate control source".into());
    };
    let encoded = source_json(case)?;
    if json::parse(&encoded)? != **case {
        return Err("control source encoding changed its content".into());
    }
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    let observations = target.join("observations");
    fs::create_dir_all(&observations)?;
    let input = target.join("input.json");
    fs::write(&input, format!("[{encoded}]"))?;
    fs::hard_link(
        observation,
        observations.join(observation.file_name().ok_or("missing observation file")?),
    )?;
    let source_root = observation
        .parent()
        .and_then(Path::parent)
        .ok_or("missing shape root")?;
    for directory in ["scripts", "scope-observations"] {
        fs::create_dir_all(target.join(directory))?;
        let filename = format!("{name}.jsonl");
        fs::copy(
            source_root.join(directory).join(&filename),
            target.join(directory).join(filename),
        )?;
    }
    Ok((input, observations))
}

fn completed_controls(
    checker: &Path,
    inventory: &Json,
    candidates: &[PathBuf],
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Json::Array(cases) = inventory else {
        return Err("invalid control inventory".into());
    };
    let mut receive_target = None;
    for observation in candidates {
        let name = observation
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("invalid observation name")?;
        let case = cases
            .iter()
            .find(|case| case.get("name") == Some(&Json::Number(name.into())))
            .ok_or("missing mutation source")?;
        let Some(Json::Array(nodes)) = case.get("nodes") else {
            return Err("missing mutation source nodes".into());
        };
        let labels = nodes
            .iter()
            .filter(|node| {
                node.get("kind") == Some(&Json::Number("0".into()))
                    && node.get("sender") != node.get("receiver")
            })
            .filter_map(|node| match node.get("event") {
                Some(Json::Number(label)) => Some(label.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let path = output
            .join("scope-observations")
            .join(observation.file_name().ok_or("missing observation name")?)
            .with_extension("jsonl");
        for (index, line) in BufReader::new(fs::File::open(path)?)
            .lines()
            .skip(1)
            .enumerate()
        {
            let record = json::parse(&line?)?;
            let Some(Json::Array(calls)) = record.get("completed") else {
                return Err("missing mutation record calls".into());
            };
            if let Some(call_index)=calls.iter().position(|call|matches!(call,Json::Array(parts) if parts.get(3)==Some(&Json::Bool(true)) && matches!(parts.get(2),Some(Json::Number(label)) if labels.contains(label)))) {
                receive_target = Some((observation, index, call_index));
                break;
            }
        }
        if receive_target.is_some() {
            break;
        }
    }
    if receive_target.is_none() {
        if cases.iter().any(|case|matches!(case.get("nodes"),Some(Json::Array(nodes)) if nodes.iter().any(|node|node.get("kind")==Some(&Json::Number("0".into())) && node.get("sender")!=node.get("receiver")))) {
            return Err("a source receive has no completed mutation point".into());
        }
        println!(
            "Completed trace checker: receive-before-send is inapplicable to self-role operations only"
        );
    }
    let mut checked = Vec::new();
    for mutation in [
        "receive-before-send",
        "missing-call",
        "duplicate-call",
        "wrong-actor",
        "wrong-role",
    ] {
        let (observation, record_index, call_index) = if mutation == "receive-before-send" {
            let Some(target) = receive_target else {
                continue;
            };
            target
        } else {
            (
                candidates.first().ok_or("no completed mutation source")?,
                0,
                0,
            )
        };
        let target = output
            .join("controls")
            .join(format!("completed-{mutation}"));
        let (input, observations) = single_case(inventory, &target, observation)?;
        let filename = format!(
            "{}.jsonl",
            observation
                .file_stem()
                .unwrap()
                .to_str()
                .ok_or("invalid case name")?
        );
        let path = target.join("scope-observations").join(filename);
        let temporary = path.with_extension("new");
        let mut reader = BufReader::new(fs::File::open(&path)?);
        let mut writer = BufWriter::new(fs::File::create(&temporary)?);
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err("missing completed header".into());
        }
        writer.write_all(line.as_bytes())?;
        for _ in 0..record_index {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Err("missing mutation target record".into());
            }
            writer.write_all(line.as_bytes())?;
        }
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err("missing completed execution".into());
        }
        let mut record = json::parse(&line)?;
        let Json::Object(fields) = &mut record else {
            return Err("invalid completed execution".into());
        };
        let Some(Json::Array(calls)) = fields.get_mut("completed") else {
            return Err("missing completed calls".into());
        };
        match mutation {
            "receive-before-send" => {
                let call = calls.remove(call_index);
                calls.insert(0, call);
            }
            "missing-call" => {
                calls.pop().ok_or("no missing-call mutation point")?;
            }
            "duplicate-call" => {
                let first = calls.first().ok_or("no duplicate-call mutation point")?;
                calls.push(json::parse(&source_json(first)?)?);
            }
            "wrong-actor" => {
                let Some(Json::Array(first)) = calls.first_mut() else {
                    return Err("no actor mutation point".into());
                };
                *first.first_mut().ok_or("empty completed call")? = Json::Number("255".into());
            }
            "wrong-role" => {
                let Some(Json::Array(first)) = calls.first_mut() else {
                    return Err("no role mutation point".into());
                };
                *first.get_mut(1).ok_or("missing completed role")? = Json::Number("255".into());
            }
            _ => unreachable!(),
        }
        writeln!(writer, "{}", source_json(&record)?)?;
        std::io::copy(&mut reader, &mut writer)?;
        writer.flush()?;
        drop(writer);
        fs::rename(temporary, path)?;
        rejected(
            checker,
            &input,
            &observations,
            if mutation == "missing-call" {
                "incomplete completed trace"
            } else {
                "invalid completed trace"
            },
        )?;
        checked.push(mutation);
    }
    println!(
        "Completed trace checker controls: {}: pass",
        checked.join(", ")
    );
    Ok(())
}
fn rejected(
    checker: &Path,
    inputs: &Path,
    outputs: &Path,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = Command::new(checker).arg(inputs).arg(outputs).output()?;
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(
        outputs
            .parent()
            .ok_or("missing control directory")?
            .join("rejection.log"),
        &report,
    )?;
    if result.status.success() || !report.contains(expected) {
        return Err(format!("negative control was not detected: {expected}: {report}").into());
    }
    Ok(())
}

fn admission_controls(checker: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let inventory = json::parse(&fs::read_to_string(output.join("inputs.json"))?)?;
    let mut paths = fs::read_dir(output.join("observations"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for (name, accepted, continued, expected) in [
        (
            "false-rejection",
            true,
            false,
            "runtime-only=0 model-only=1",
        ),
        (
            "false-admission",
            false,
            false,
            "runtime-only=1 model-only=0",
        ),
        (
            "continuation-false-rejection",
            true,
            true,
            "runtime-only=0 model-only=1",
        ),
        (
            "continuation-false-admission",
            false,
            true,
            "runtime-only=1 model-only=0",
        ),
    ] {
        let mut selected = None;
        for path in &paths {
            if let Some(witness) = outcome_witness(path, continued, accepted)? {
                selected = Some((path, witness));
                break;
            }
        }
        let (path, (record, row)) = selected.ok_or("missing admission control point")?;
        let (input, observations) = single_case(
            &inventory,
            &output.join("admission-controls").join(name),
            path,
        )?;
        rewrite_records(
            &observations.join(path.file_name().unwrap()),
            |index, line| {
                if index != record {
                    return Ok(vec![line.into()]);
                }
                let Json::Object(mut group) = json::parse(line)? else {
                    return Err("invalid group".into());
                };
                let Some(Json::Array(rows)) = group.get_mut("answers") else {
                    return Err("missing answers".into());
                };
                let Json::Array(fields) = &mut rows[row] else {
                    return Err("invalid answer".into());
                };
                fields[0] = Json::Bool(!accepted);
                Ok(vec![source_json(&Json::Object(group))?])
            },
        )?;
        rejected(checker, &input, &observations, expected)?;
        println!("Admission checker control: {name}: pass");
    }
    Ok(())
}

fn parallel_coverage_control(
    checker: &Path,
    inventory: &Json,
    candidates: &[PathBuf],
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    fn settings(script: &Json) -> Result<(bool, bool, String), Box<dyn std::error::Error>> {
        let Json::Array(fields) = script else {
            return Err("invalid control script".into());
        };
        let Some(Json::Number(kind)) = fields.first() else {
            return Err("missing script kind".into());
        };
        match kind.as_str() {
            "0" => Ok((true, false, source_json(script)?)),
            "1" | "2" | "4" => {
                let (a, a_par, a_path) = settings(fields.get(1).ok_or("missing left script")?)?;
                let (b, b_par, b_path) = settings(fields.get(2).ok_or("missing right script")?)?;
                let path = format!("[{kind},{a_path},{b_path}]");
                if kind == "2" {
                    let Some(Json::Bool(reverse)) = fields.get(3) else {
                        return Err("missing parallel order".into());
                    };
                    Ok((a && b && !reverse, true, path))
                } else {
                    Ok((a && b, a_par || b_par, path))
                }
            }
            "3" => {
                let (forward, parallel, path) =
                    settings(fields.get(1).ok_or("missing choice script")?)?;
                let right = source_json(fields.get(2).ok_or("missing choice arm")?)?;
                Ok((forward, parallel, format!("[3,{path},{right}]")))
            }
            _ => Err("unknown control script kind".into()),
        }
    }
    let Json::Array(cases) = inventory else {
        return Err("invalid parallel source inventory".into());
    };
    let mut checked_orders = false;
    let mut checked_paths = false;
    for observation in candidates {
        let name = observation
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("invalid case name")?;
        let case = cases
            .iter()
            .find(|case| case.get("name") == Some(&Json::Number(name.into())))
            .ok_or("missing source case")?;
        if case.get("parallelCoverage") != Some(&Json::Bool(true)) {
            continue;
        }
        let filename = format!("{name}.jsonl");
        let scripts = BufReader::new(fs::File::open(output.join("scripts").join(&filename))?)
            .lines()
            .collect::<std::io::Result<Vec<_>>>()?;
        let settings = scripts
            .iter()
            .map(|line| settings(&json::parse(line)?))
            .collect::<Result<Vec<_>, _>>()?;
        let first_path = settings
            .first()
            .ok_or("empty control script inventory")?
            .2
            .clone();
        let selections = [
            (
                !checked_orders && settings.iter().any(|(_, parallel, _)| *parallel),
                "missing-parallel-orders",
                "missing parallel pair coverage",
                settings
                    .iter()
                    .map(|(forward, _, _)| *forward)
                    .collect::<Vec<_>>(),
            ),
            (
                !checked_paths && settings.iter().any(|(_, _, path)| *path != first_path),
                "missing-choice-path",
                "missing source choice path",
                settings
                    .iter()
                    .map(|(_, _, path)| *path != first_path)
                    .collect::<Vec<_>>(),
            ),
        ];
        for (applicable, mutation, expected, keep) in selections {
            if !applicable {
                continue;
            }
            let count = keep.iter().filter(|value| **value).count();
            if count == 0 || count == scripts.len() {
                return Err("parallel control has no omitted schedule".into());
            }
            let target = output.join("controls").join(mutation);
            let (input, observations) = single_case(inventory, &target, observation)?;
            let mut script_output =
                BufWriter::new(fs::File::create(target.join("scripts").join(&filename))?);
            let mut completed_output = BufWriter::new(fs::File::create(
                target.join("scope-observations").join(&filename),
            )?);
            let mut completed_input = BufReader::new(fs::File::open(
                output.join("scope-observations").join(&filename),
            )?)
            .lines();
            completed_input.next().ok_or("missing control header")??;
            writeln!(completed_output, "{{\"name\":{name},\"scripts\":{count}}}")?;
            let mut next = 0;
            for (index, (line, keep)) in scripts.iter().zip(keep).enumerate() {
                if keep {
                    writeln!(script_output, "{line}")?;
                }
                for reverse in [false, true] {
                    let mut record = json::parse(
                        &completed_input
                            .next()
                            .ok_or("missing control execution")??,
                    )?;
                    let Json::Object(fields) = &mut record else {
                        return Err("invalid control execution".into());
                    };
                    if fields.get("script") != Some(&Json::Number(index.to_string()))
                        || fields.get("reversePoll") != Some(&Json::Bool(reverse))
                    {
                        return Err("wrong control execution order".into());
                    }
                    if keep {
                        fields.insert("script".into(), Json::Number(next.to_string()));
                        writeln!(completed_output, "{}", source_json(&record)?)?;
                    }
                }
                if keep {
                    next += 1;
                }
            }
            if completed_input.next().is_some() || next != count {
                return Err("invalid control inventory".into());
            }
            script_output.flush()?;
            completed_output.flush()?;
            rejected(checker, &input, &observations, expected)?;
            println!("Source schedule coverage control: {mutation} with updated counts: pass");
            match mutation {
                "missing-parallel-orders" => checked_orders = true,
                "missing-choice-path" => checked_paths = true,
                _ => unreachable!(),
            }
        }
        if checked_orders && checked_paths {
            break;
        }
    }
    if !checked_orders {
        println!("Parallel schedule coverage control: no pairwise parallel case in this shard");
    }
    if !checked_paths {
        println!(
            "Choice path coverage control: no pairwise case with several choice paths in this shard"
        );
    }
    Ok(())
}

fn causal_control(
    checker: &Path,
    inventory: &Json,
    candidates: &[PathBuf],
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for observation in candidates {
        let filename = observation.file_name().ok_or("missing observation name")?;
        let source = output
            .join("scope-observations")
            .join(filename)
            .with_extension("jsonl");
        let mut reader = BufReader::new(fs::File::open(&source)?);
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let mut prefix = header;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let mut record = json::parse(&line)?;
            let Json::Object(fields) = &mut record else {
                return Err("invalid local probe record".into());
            };
            let Some(Json::Array(probes)) = fields.get_mut("probes") else {
                return Err("missing local probes".into());
            };
            if let Some(Json::Object(probe)) = probes
                .iter_mut()
                .find(|probe| probe.get("kind") == Some(&Json::String("causal".into())))
            {
                // A send cannot be its own preceding receive. Keep the schema,
                // rejection outcome and every observed call unchanged.
                let label = source_json(probe.get("label").ok_or("missing causal target")?)?;
                probe.insert("before".into(), json::parse(&label)?);
                let target = output.join("controls/causal-self-dependency");
                let (input, observations) = single_case(inventory, &target, observation)?;
                let path = target
                    .join("scope-observations")
                    .join(filename)
                    .with_extension("jsonl");
                let mut writer = BufWriter::new(fs::File::create(path)?);
                writer.write_all(prefix.as_bytes())?;
                writeln!(writer, "{}", source_json(&record)?)?;
                std::io::copy(&mut reader, &mut writer)?;
                writer.flush()?;
                drop(writer);
                rejected(
                    checker,
                    &input,
                    &observations,
                    "invalid causal rejection witness",
                )?;
                println!("Causal source checker control: self-dependency: pass");
                return Ok(());
            }
            prefix.push_str(&line);
        }
    }
    println!("Causal source checker control: no required receive-before-send in this shard");
    Ok(())
}

// Exercise shrinking against a real Endpoint run and a deliberately weakened
// source model. The mutation removes the root seq dependency, never Rust code.
fn shrink_probe(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    if args.len() != 4 {
        return Err("shrink probe requires REPOSITORY DEPTH RANK OUTPUT".into());
    }
    let repo = Path::new(&args[0]);
    let output = Path::new(&args[3]);
    let observed = Command::new("bash")
        .arg(repo.join(".github/scripts/check_protocol_shapes.sh"))
        .args([
            "--syntax-depth",
            &args[1],
            &args[2],
            "1",
            "--",
            "--mutant-root-seq-as-par",
        ])
        .env("HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT", output.canonicalize()?)
        .output()?;
    fs::write(
        output.join("validation.log"),
        [observed.stdout.as_slice(), observed.stderr.as_slice()].concat(),
    )?;
    std::io::stdout().write_all(&observed.stdout)?;
    std::io::stderr().write_all(&observed.stderr)?;
    Ok(observed.status.code().unwrap_or(1))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--check-shrink") {
        if args.len() != 2 {
            return Err("usage: protocol-shape-controls --check-shrink DIRECTORY".into());
        }
        let root = Path::new(&args[1]);
        let source = fs::read_to_string(root.join("best.tsv"))?;
        let mut rows = source.lines();
        if rows.next() != Some("depth\trank\toperators") {
            return Err("invalid reduced source header".into());
        }
        let fields = rows
            .next()
            .ok_or("missing reduced source")?
            .split('\t')
            .map(str::parse::<usize>)
            .collect::<Result<Vec<_>, _>>()?;
        // seq(0 -> 1, 0 -> 1) has exact-height rank 32 + 1*32 + 1.
        if fields != [1, 65, 1] || rows.next().is_some() {
            return Err("shrinker control did not retain exactly two ordered messages".into());
        }
        if fs::read_to_string(root.join("result.txt"))?
            != "failure=ModelOnly\nminimal_under=subtree-removal\n"
        {
            return Err("shrinker control did not preserve its semantic mismatch".into());
        }
        println!("Shrinker control: two ordered messages, same model-only mismatch: pass");
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--shrink-probe") {
        std::process::exit(shrink_probe(&args[1..])?);
    }
    if let [command, checker, output] = args.as_slice()
        && command == "--admission-controls"
    {
        return admission_controls(Path::new(checker), Path::new(output));
    }
    if args.len() != 2 {
        return Err("usage: protocol-shape-controls CHECKER GENERATED_DIRECTORY".into());
    }
    let checker = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let inputs = output.join("inputs.json");
    let inventory = json::parse(&fs::read_to_string(&inputs)?)?;
    scope_audit::audit(&output, &inventory)?;
    roll_coverage::audit(&output)?;
    let original = output.join("observations");
    let mut paths = fs::read_dir(&original)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let first = paths.first().ok_or("no observed cases")?;
    completed_controls(&checker, &inventory, &paths, &output)?;
    parallel_coverage_control(&checker, &inventory, &paths, &output)?;
    causal_control(&checker, &inventory, &paths, &output)?;
    let missing = output.join("controls/missing-case");
    let (input, observations) = single_case(&inventory, &missing, first)?;
    fs::remove_file(observations.join(first.file_name().unwrap()))?;
    rejected(
        &checker,
        &input,
        &observations,
        "missing, duplicate or unknown shape observation",
    )?;
    let malformed = output.join("controls/malformed");
    let (input, observations) = single_case(&inventory, &malformed, first)?;
    rewrite_records(
        &observations.join(first.file_name().unwrap()),
        |index, line| Ok(vec![if index == 0 { "{".into() } else { line.into() }]),
    )?;
    rejected(&checker, &input, &observations, "invalid shape JSON")?;
    let (input, observations) = single_case(
        &inventory,
        &output.join("controls/unknown-resolver-input"),
        first,
    )?;
    rewrite_records(
        &observations.join(first.file_name().unwrap()),
        |index, line| {
            if index != 0 {
                return Ok(vec![line.into()]);
            }
            let Json::Object(mut header) = json::parse(line)? else {
                return Err("invalid header".into());
            };
            header.insert(
                "choices".into(),
                Json::Array(vec![Json::Array(vec![Json::Number("65536".into())])]),
            );
            Ok(vec![source_json(&Json::Object(header))?])
        },
    )?;
    rejected(
        &checker,
        &input,
        &observations,
        "incomplete probe inventory",
    )?;
    let mut checked = Vec::new();
    for (name, expected) in [
        ("continuation-missing-probe", "incomplete probe inventory"),
        ("continuation-missing-prefix", "incomplete probe inventory"),
        ("continuation-duplicate-probe", "incomplete probe inventory"),
        (
            "continuation-duplicate-prefix",
            "incomplete probe inventory",
        ),
        (
            "continuation-missing-resolver-inputs",
            "invalid replayed resolver inputs",
        ),
    ] {
        let mut selected = None;
        for path in &paths {
            if let Some(witness) = outcome_witness(path, true, false)? {
                selected = Some((path, witness));
                break;
            }
        }
        let (path, (record, row)) = selected.ok_or("missing continuation mutation point")?;
        let selected = BufReader::new(fs::File::open(path)?)
            .lines()
            .nth(record)
            .ok_or("missing selected prefix")??;
        let selected = json::parse(&selected)?;
        let word = selected
            .get("prefix")
            .ok_or("missing selected prefix word")?;
        let target = output.join("controls").join(name);
        let (input, observations) = single_case(&inventory, &target, path)?;
        rewrite_records(
            &observations.join(path.file_name().unwrap()),
            |index, line| {
                if name == "continuation-missing-prefix"
                    && index != 0
                    && json::parse(line)?.get("prefix") == Some(word)
                {
                    return Ok(vec![]);
                }
                if index != record {
                    return Ok(vec![line.into()]);
                }
                let Json::Object(mut group) = json::parse(line)? else {
                    return Err("invalid group".into());
                };
                match name {
                    "continuation-missing-prefix" => {
                        return Err("selected prefix was not removed".into());
                    }
                    "continuation-duplicate-prefix" => return Ok(vec![line.into(), line.into()]),
                    "continuation-missing-resolver-inputs" => {
                        group.insert("prefixChoices".into(), Json::Array(vec![]));
                    }
                    "continuation-missing-probe" | "continuation-duplicate-probe" => {
                        let Some(Json::Array(rows)) = group.get_mut("answers") else {
                            return Err("missing answers".into());
                        };
                        if name == "continuation-missing-probe" {
                            rows.remove(row);
                        } else {
                            let extra = json::parse(&source_json(&rows[row])?)?;
                            rows.insert(row, extra);
                        }
                    }
                    _ => return Err("unknown continuation mutation".into()),
                }
                Ok(vec![source_json(&Json::Object(group))?])
            },
        )?;
        rejected(&checker, &input, &observations, expected)?;
        checked.push(name);
    }
    println!(
        "Generated continuation checker controls: {}: pass",
        checked.join(", ")
    );
    let mut history_control = false;
    for path in &paths {
        if let Some(record) = missing_history(path)? {
            let (input, observations) = single_case(
                &inventory,
                &output.join("controls/continuation-missing-history"),
                path,
            )?;
            rewrite_records(
                &observations.join(path.file_name().unwrap()),
                |index, line| {
                    Ok(if index == record {
                        vec![]
                    } else {
                        vec![line.into()]
                    })
                },
            )?;
            rejected(
                &checker,
                &input,
                &observations,
                "incomplete probe inventory",
            )?;
            history_control = true;
            break;
        }
    }
    println!(
        "Resolver history coverage control: {}",
        if history_control {
            "one outcome-distinguishing history removed, other history retained: pass"
        } else {
            "inapplicable: no prefix has outcome-distinguishing recorded histories"
        }
    );
    println!(
        "Generated shape checker controls: missing-case, malformed, unknown-resolver-input: pass"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mutation_preserves_the_shared_baseline_and_every_other_record() {
        let root =
            env::temp_dir().join(format!("hibana-observation-groups-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let original = root.join("baseline.jsonl");
        let changed = root.join("changed.jsonl");
        let text = concat!(
            "{\"name\":0,\"choices\":[[]],\"events\":[0]}\n",
            "{\"prefix\":[],\"prefixChoices\":[],\"answers\":[[true,[]],[false,[]]]}\n",
            "{\"prefix\":[0],\"prefixChoices\":[[]],\"answers\":[[false,[]],[true,[[1,true]]]]}\n",
            "{\"prefix\":[0],\"prefixChoices\":[[0]],\"answers\":[[true,[]],[true,[]]]}\n",
        );
        fs::write(&original, text).unwrap();
        fs::hard_link(&original, &changed).unwrap();
        assert_eq!(
            outcome_witness(&original, true, false).unwrap(),
            Some((2, 0))
        );
        let record = missing_history(&original).unwrap().unwrap();
        assert_eq!(record, 3);
        rewrite_records(&changed, |index, line| {
            Ok(if index == record {
                vec![]
            } else {
                vec![line.into()]
            })
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), text);
        let remaining = fs::read_to_string(&changed).unwrap();
        assert_eq!(
            remaining.lines().collect::<Vec<_>>(),
            text.lines().take(3).collect::<Vec<_>>()
        );
        assert!(missing_history(&changed).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
