//! Corrupt result data to test the compiled shape checker, not Rust source text.
#[path = "kani_json.rs"]
mod json;
#[path = "protocol_shape_scope_audit.rs"]
mod scope_audit;
#[path = "protocol_shape_paths.rs"]
mod syntax_paths;
use json::Json;
use std::{
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn continuation(text: &str, accepted: bool) -> Option<core::ops::Range<usize>> {
    for (start, _) in text.match_indices("{\"choice\":") {
        let end = start + text[start..].find('}')? + 1;
        let record = json::parse(&text[start..end]).ok()?;
        if matches!(record.get("prefix"), Some(Json::Array(prefix)) if !prefix.is_empty())
            && record.get("accepted") == Some(&Json::Bool(accepted))
        {
            return Some(start..end);
        }
    }
    None
}

// Bound successful API calls from the source, independently of observations.
// A self-role message has one local operation; other messages have two ends.
fn max_completed_calls(case: &Json) -> Result<usize, String> {
    let Some(Json::Array(nodes)) = case.get("nodes") else {
        return Err("missing source nodes".into());
    };
    let number = |node: &Json, field: &str| match node.get(field) {
        Some(Json::Number(value)) => value
            .parse::<usize>()
            .map_err(|_| "invalid source index".to_owned()),
        _ => Err("missing source number".to_owned()),
    };
    let mut counts: Vec<usize> = Vec::new();
    for node in nodes {
        let kind = number(node, "kind")?;
        let count = if kind == 0 {
            if number(node, "sender")? == number(node, "receiver")? {
                1
            } else {
                2
            }
        } else {
            let left = *counts
                .get(number(node, "left")?)
                .ok_or("source left child is not earlier")?;
            if kind == 5 {
                left.checked_mul(2).ok_or("source call count overflow")?
            } else {
                let right = *counts
                    .get(number(node, "right")?)
                    .ok_or("source right child is not earlier")?;
                match kind {
                    1 | 2 => left
                        .checked_add(right)
                        .ok_or("source call count overflow")?,
                    3 | 4 => left.max(right),
                    _ => return Err("unknown source constructor".into()),
                }
            }
        };
        counts.push(count);
    }
    counts.last().copied().ok_or("empty source".into())
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
    text: &str,
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
    fs::write(
        observations.join(observation.file_name().ok_or("missing observation file")?),
        text,
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
            .filter_map(|node| match node.get("label") {
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
        let (input, observations) = single_case(
            inventory,
            &target,
            observation,
            &fs::read_to_string(observation)?,
        )?;
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
    if result.status.success() || !report.contains(expected) {
        return Err(format!("negative control was not detected: {expected}: {report}").into());
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
                let (input, observations) = single_case(
                    inventory,
                    &target,
                    observation,
                    &fs::read_to_string(observation)?,
                )?;
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
    let depth = args[1].parse::<usize>()?;
    let rank = args[2].parse::<num_bigint::BigUint>()?;
    let generator = std::env::current_exe()?
        .with_file_name(format!("protocol-shapes{}", std::env::consts::EXE_SUFFIX));
    let baseline = Command::new(generator)
        .arg("syntax-probe")
        .args(args)
        .output()?;
    if !baseline.status.success() {
        std::io::stdout().write_all(&baseline.stdout)?;
        std::io::stderr().write_all(&baseline.stderr)?;
        return Ok(baseline.status.code().unwrap_or(1));
    }
    let generated = repo
        .join("target/protocol-shapes")
        .join(syntax_paths::relative(depth, &rank, 1));
    let input = json::parse(&fs::read_to_string(generated.join("inputs.json"))?)?;
    let Json::Array(cases) = &input else {
        return Err("invalid shrink control input".into());
    };
    let Some(Json::Object(case)) = cases.first() else {
        return Ok(0);
    };
    let Some(Json::Array(nodes)) = case.get("nodes") else {
        return Err("missing control nodes".into());
    };
    let Some(Json::Object(root)) = nodes.last() else {
        return Err("missing control root".into());
    };
    if root.get("kind") != Some(&Json::Number("1".into())) {
        return Ok(0);
    }
    let observed = Command::new(repo.join("proofs/lean/.lake/build/bin/protocol-shapes"))
        .arg(generated.join("inputs.json"))
        .arg(generated.join("observations"))
        .arg("--mutant-root-seq-as-par")
        .output()?;
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
    if args.len() != 2 {
        return Err("usage: protocol-shape-controls CHECKER GENERATED_DIRECTORY".into());
    }
    let checker = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let inputs = output.join("inputs.json");
    let inventory = json::parse(&fs::read_to_string(&inputs)?)?;
    scope_audit::audit(&output, &inventory)?;
    let original = output.join("observations");
    let mut paths = fs::read_dir(&original)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let first = paths.first().ok_or("no observed cases")?;
    completed_controls(&checker, &inventory, &paths, &output)?;
    causal_control(&checker, &inventory, &paths, &output)?;
    for (name, needle, replacement, expected) in [
        (
            "false-rejection",
            r#""accepted":true"#,
            r#""accepted":false"#,
            "runtime-only=0 model-only=1",
        ),
        (
            "false-admission",
            r#""accepted":false"#,
            r#""accepted":true"#,
            "runtime-only=1 model-only=0",
        ),
    ] {
        let target = output.join("controls").join(name);
        let text = fs::read_to_string(first)?;
        if !text.contains(needle) {
            return Err(format!("missing mutation point {name}").into());
        }
        let (input, observations) = single_case(
            &inventory,
            &target,
            first,
            &text.replacen(needle, replacement, 1),
        )?;
        rejected(&checker, &input, &observations, expected)?;
    }
    let missing = output.join("controls/missing-case");
    let (input, observations) =
        single_case(&inventory, &missing, first, &fs::read_to_string(first)?)?;
    fs::remove_file(observations.join(first.file_name().unwrap()))?;
    rejected(
        &checker,
        &input,
        &observations,
        "missing, duplicate or unknown shape observation",
    )?;
    let malformed = output.join("controls/malformed");
    let (input, observations) = single_case(&inventory, &malformed, first, "{")?;
    rejected(&checker, &input, &observations, "invalid shape JSON")?;
    let mut invalid_choice = json::parse(&fs::read_to_string(first)?)?;
    let Json::Object(fields) = &mut invalid_choice else {
        return Err("invalid observation object".into());
    };
    let Some(Json::Array(attempts)) = fields.get_mut("attempts") else {
        return Err("missing probe attempts".into());
    };
    let Some(Json::Object(attempt)) = attempts.first_mut() else {
        return Err("missing resolver input control point".into());
    };
    attempt.insert(
        "choice".into(),
        Json::Array(vec![Json::Number("65536".into())]),
    );
    let (input, observations) = single_case(
        &inventory,
        &output.join("controls/unknown-resolver-input"),
        first,
        &source_json(&invalid_choice)?,
    )?;
    rejected(
        &checker,
        &input,
        &observations,
        "incomplete probe inventory",
    )?;
    let continuation_source = match &inventory {
        Json::Array(cases) => cases.iter().any(|case| match case.get("nodes") {
            Some(Json::Array(nodes)) => {
                let repeats = nodes
                    .iter()
                    .filter(|node| node.get("kind") == Some(&Json::Number("5".into())))
                    .count();
                repeats == 0
                    || (repeats == 1
                        && nodes.last().and_then(|node| node.get("kind"))
                            == Some(&Json::Number("5".into())))
            }
            _ => false,
        }),
        _ => return Err("invalid source inventory for continuation controls".into()),
    };
    if continuation_source {
        let Json::Array(cases) = &inventory else {
            return Err("invalid source inventory".into());
        };
        let has_positive_continuation = cases
            .iter()
            .map(max_completed_calls)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|count| count > 1);
        let mut checked = Vec::new();
        for (name, accepted, expected) in [
            (
                "continuation-false-admission",
                false,
                "runtime-only=1 model-only=0",
            ),
            (
                "continuation-false-rejection",
                true,
                "runtime-only=0 model-only=1",
            ),
            (
                "continuation-missing-probe",
                false,
                "incomplete probe inventory",
            ),
            (
                "continuation-duplicate-probe",
                false,
                "incomplete probe inventory",
            ),
        ] {
            let mut selected = None;
            for path in &paths {
                let text = fs::read_to_string(path)?;
                if let Some(range) = continuation(&text, accepted) {
                    selected = Some((path, text, range));
                    break;
                }
            }
            let Some((path, mut text, range)) = selected else {
                if accepted && !has_positive_continuation {
                    println!(
                        "Continuation false-rejection control is inapplicable: every source path has one local operation"
                    );
                    continue;
                }
                return Err("missing continuation mutation point".into());
            };
            let record = text[range.clone()].to_owned();
            match name {
                "continuation-missing-probe" => {
                    let separator = range
                        .start
                        .checked_sub(1)
                        .ok_or("missing continuation separator")?;
                    if text.as_bytes().get(separator) != Some(&b',') {
                        return Err("continuation record has no preceding separator".into());
                    }
                    text.replace_range(separator..range.end, "");
                }
                "continuation-duplicate-probe" => text.insert_str(range.end, &format!(",{record}")),
                _ => {
                    text.replace_range(
                        range,
                        &record.replacen(
                            &format!("\"accepted\":{accepted}"),
                            &format!("\"accepted\":{}", !accepted),
                            1,
                        ),
                    );
                }
            }
            let target = output.join("controls").join(name);
            let (input, observations) = single_case(&inventory, &target, path, &text)?;
            rejected(&checker, &input, &observations, expected)?;
            checked.push(name);
        }
        println!(
            "Generated continuation checker controls: {}: pass",
            checked.join(", ")
        );
    } else {
        println!(
            "Generated continuation checker controls: no source with continuation probes in this shard"
        );
    }
    println!(
        "Generated shape checker controls: false-admission, false-rejection, missing-case, malformed, unknown-resolver-input: pass"
    );
    Ok(())
}

#[cfg(test)]
mod source_bounds {
    use super::*;

    #[test]
    fn local_choices_sequences_and_repetitions_have_different_call_bounds() {
        let choice = r#"{"nodes":[{"kind":0,"sender":0,"receiver":0},{"kind":0,"sender":0,"receiver":0},{"kind":4,"left":0,"right":1}]}"#;
        assert_eq!(
            max_completed_calls(&json::parse(choice).unwrap()).unwrap(),
            1
        );
        let sequence = choice.replace("\"kind\":4", "\"kind\":1");
        assert_eq!(
            max_completed_calls(&json::parse(&sequence).unwrap()).unwrap(),
            2
        );
        let repeated = r#"{"nodes":[{"kind":0,"sender":0,"receiver":0},{"kind":5,"left":0}]}"#;
        assert_eq!(
            max_completed_calls(&json::parse(repeated).unwrap()).unwrap(),
            2
        );
        let ordinary = r#"{"nodes":[{"kind":0,"sender":0,"receiver":1}]}"#;
        assert_eq!(
            max_completed_calls(&json::parse(ordinary).unwrap()).unwrap(),
            2
        );
        for invalid in [
            r#"{"nodes":[]}"#,
            r#"{"nodes":[{"kind":0}]}"#,
            r#"{"nodes":[{"kind":1,"left":0,"right":0}]}"#,
        ] {
            assert!(max_completed_calls(&json::parse(invalid).unwrap()).is_err());
        }
    }
}
