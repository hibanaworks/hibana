//! Corrupt result data to test the compiled shape checker, not Rust source text.
#[path = "kani_json.rs"]
mod json;
#[path = "protocol_shape_scope_audit.rs"]
mod scope_audit;
use json::Json;
use std::{
    env, fs,
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
// ShapeInput contains numeric fields, booleans, arrays and objects only.
fn source_json(value: &Json) -> Result<String, Box<dyn std::error::Error>> {
    Ok(match value {
        Json::Number(number) => number.clone(),
        Json::Bool(value) => value.to_string(),
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
    Ok((input, observations))
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
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
            let (path, mut text, range) = selected.ok_or("missing continuation mutation point")?;
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
        }
        println!(
            "Generated continuation checker controls: false-admission, false-rejection, missing-probe, duplicate-probe: pass"
        );
    } else {
        println!(
            "Generated continuation checker controls: no source with continuation probes in this shard"
        );
    }
    println!(
        "Generated shape checker controls: false-admission, false-rejection, missing-case, malformed: pass"
    );
    Ok(())
}
