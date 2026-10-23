//! Check every source-planned borrowed-scope probe in both execution schedules.
use super::{Json, json, source_json};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Cursor},
    path::Path,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn number(value: Option<&Json>) -> Result<usize> {
    match value {
        Some(Json::Number(value)) => Ok(value.parse()?),
        _ => Err("invalid borrowed-scope number".into()),
    }
}

fn fields(value: &Json, count: usize) -> Result<()> {
    match value {
        Json::Object(fields) if fields.len() == count => Ok(()),
        _ => Err("invalid borrowed-scope fields".into()),
    }
}

fn case_name(case: &Json) -> Result<&str> {
    match case.get("name") {
        Some(Json::Number(name)) if name.parse::<usize>().is_ok() => Ok(name),
        _ => Err("invalid borrowed-scope case name".into()),
    }
}

fn read(reader: &mut impl BufRead) -> Result<Json> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err("missing borrowed-scope record".into());
    }
    Ok(json::parse(&line)?)
}

fn end(reader: &mut impl BufRead) -> Result<()> {
    if reader.read_line(&mut String::new())? != 0 {
        return Err("extra borrowed-scope record".into());
    }
    Ok(())
}

fn probes(plan: &Json) -> Result<&[Json]> {
    let Some(Json::Array(records)) = plan.get("probes") else {
        return Err("invalid borrowed-scope probe inventory".into());
    };
    let mut unique = BTreeSet::new();
    for record in records {
        fields(record, 8)?;
        for key in ["part", "actor", "role", "label"] {
            u8::try_from(number(record.get(key))?)?;
        }
        number(record.get("occurrence"))?;
        let Some(Json::Array(group)) = record.get("group") else {
            return Err("missing borrowed-scope group".into());
        };
        if group.len() != 2 {
            return Err("invalid borrowed-scope group".into());
        }
        for value in group {
            u8::try_from(number(Some(value))?)?;
        }
        if !matches!(record.get("receive"), Some(Json::Bool(_)))
            || record.get("accepted") != Some(&Json::Bool(false))
            || !unique.insert(source_json(record)?)
        {
            return Err("invalid, duplicate or accepted borrowed-scope probe".into());
        }
    }
    Ok(records)
}

fn check_case(
    mut expected: impl BufRead,
    mut actual: impl BufRead,
    case: &Json,
) -> Result<(usize, Option<Json>)> {
    fields(case, 3)?;
    let scripts = number(case.get("scripts"))?;
    let header = json::parse(&format!(
        r#"{{"name":{},"scripts":{scripts}}}"#,
        case_name(case)?
    ))?;
    if read(&mut expected)? != header || read(&mut actual)? != header {
        return Err("wrong borrowed-scope header".into());
    }
    let mut checked = 0usize;
    let mut control = None;
    for script in 0..scripts {
        let plan = read(&mut expected)?;
        fields(&plan, 2)?;
        if number(plan.get("script"))? != script {
            return Err("missing, duplicate or reordered borrowed-scope script".into());
        }
        let records = probes(&plan)?;
        checked = checked
            .checked_add(2 * records.len())
            .ok_or("borrowed-scope count overflow")?;
        for concurrent in [false, true] {
            let observed = read(&mut actual)?;
            fields(&observed, 3)?;
            if observed.get("script") != plan.get("script")
                || observed.get("concurrent") != Some(&Json::Bool(concurrent))
                || observed.get("probes") != plan.get("probes")
            {
                return Err("missing, duplicate, changed or accepted borrowed-scope probe".into());
            }
        }
        if control.is_none() && !records.is_empty() {
            control = Some(plan);
        }
    }
    end(&mut expected)?;
    end(&mut actual)?;
    if checked != number(case.get("probes"))? {
        return Err("incomplete borrowed-scope probe inventory".into());
    }
    Ok((checked, control))
}

fn controls(plan: &Json) -> Result<()> {
    let records = probes(plan)?;
    if records.is_empty() {
        return Err("empty borrowed-scope control source".into());
    }
    let missing = format!(
        "[{}]",
        records
            .iter()
            .skip(1)
            .map(source_json)
            .collect::<Result<Vec<_>>>()?
            .join(",")
    );
    let duplicate = format!(
        "[{}]",
        records
            .iter()
            .chain(core::iter::once(&records[0]))
            .map(source_json)
            .collect::<Result<Vec<_>>>()?
            .join(",")
    );
    let records = source_json(plan.get("probes").ok_or("missing control probes")?)?;
    let header = "{\"name\":7,\"scripts\":1}\n";
    let expected = format!("{header}{{\"script\":0,\"probes\":{records}}}\n");
    let sequential = format!("{{\"script\":0,\"concurrent\":false,\"probes\":{records}}}\n");
    let concurrent = format!("{{\"script\":0,\"concurrent\":true,\"probes\":{records}}}\n");
    let actual = format!("{header}{sequential}{concurrent}");
    let case = json::parse(&format!(
        r#"{{"name":7,"scripts":1,"probes":{}}}"#,
        2 * probes(plan)?.len()
    ))?;
    check_case(Cursor::new(&expected), Cursor::new(&actual), &case)?;
    for changed in [
        actual.replacen("\"accepted\":false", "\"accepted\":true", 1),
        format!("{header}{{\"script\":0,\"concurrent\":false,\"probes\":{missing}}}\n{concurrent}"),
        format!(
            "{header}{{\"script\":0,\"concurrent\":false,\"probes\":{duplicate}}}\n{concurrent}"
        ),
        format!("{header}{concurrent}"),
        format!("{header}{sequential}{sequential}{concurrent}"),
        format!("{header}{concurrent}{sequential}"),
        actual.replacen("\"name\":7", "\"name\":8", 1),
        format!("{actual}{{}}\n"),
        format!("{header}{{"),
    ] {
        if check_case(Cursor::new(&expected), Cursor::new(&changed), &case).is_ok() {
            return Err("borrowed-scope negative control was not detected".into());
        }
    }
    Ok(())
}

pub(super) fn audit(output: &Path, sources: &Json) -> Result<()> {
    let Json::Array(sources) = sources else {
        return Err("invalid shape inventory".into());
    };
    let expected = json::parse(&fs::read_to_string(output.join("scope-inputs.json"))?)?;
    let Json::Array(cases) = &expected else {
        return Err("invalid borrowed-scope inventory".into());
    };
    let source_names = sources
        .iter()
        .map(case_name)
        .collect::<Result<BTreeSet<_>>>()?;
    let expected_names = cases
        .iter()
        .map(case_name)
        .collect::<Result<BTreeSet<_>>>()?;
    if source_names != expected_names
        || source_names.len() != sources.len()
        || expected_names.len() != cases.len()
    {
        return Err("missing, duplicate or unknown borrowed-scope source".into());
    }
    let expected_files = expected_names
        .iter()
        .map(|name| format!("{name}.jsonl"))
        .collect();
    for directory in ["scope-plans", "scope-observations"] {
        let actual_files = fs::read_dir(output.join(directory))?
            .map(|entry| {
                let entry = entry?;
                Ok(entry
                    .file_name()
                    .to_str()
                    .ok_or("invalid scope filename")?
                    .to_owned())
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if actual_files != expected_files {
            return Err("missing or unknown borrowed-scope file".into());
        }
    }
    let mut checked = 0;
    let mut control = None;
    for case in cases {
        let filename = format!("{}.jsonl", case_name(case)?);
        let (count, sample) = check_case(
            BufReader::new(fs::File::open(output.join("scope-plans").join(&filename))?),
            BufReader::new(fs::File::open(
                output.join("scope-observations").join(&filename),
            )?),
            case,
        )?;
        checked += count;
        if control.is_none() {
            control = sample;
        }
    }
    println!(
        "Borrowed-scope probes: cases={} attempts={checked} all rejected",
        cases.len()
    );
    if let Some(plan) = control {
        controls(&plan)?;
        println!(
            "Borrowed-scope checker controls: false-admission, missing-probe, duplicate-probe, reordered-schedule, wrong-case, extra-record, malformed: pass"
        );
    } else {
        println!("Borrowed-scope checker controls: no borrowed scopes in this shard");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_or_missing_scope_observations_are_rejected() {
        let plan = json::parse(r#"{"script":2,"probes":[{"group":[1,2],"part":1,"occurrence":3,"actor":1,"role":3,"label":2,"receive":false,"accepted":false}]}"#).unwrap();
        controls(&plan).unwrap();
        let expected = "{\"name\":7,\"scripts\":1}\n{\"script\":0,\"probes\":[]}\n";
        let actual = "{\"name\":7,\"scripts\":1}\n{\"script\":0,\"concurrent\":false,\"probes\":[]}\n{\"script\":0,\"concurrent\":true,\"probes\":[]}\n";
        let case = json::parse(r#"{"name":7,"scripts":1,"probes":0}"#).unwrap();
        assert_eq!(
            check_case(Cursor::new(expected), Cursor::new(actual), &case)
                .unwrap()
                .0,
            0
        );
        let short = expected.lines().take(1).collect::<Vec<_>>().join("\n");
        assert!(check_case(Cursor::new(short), Cursor::new(actual), &case).is_err());
        let wrong_count = json::parse(r#"{"name":7,"scripts":1,"probes":2}"#).unwrap();
        assert!(check_case(Cursor::new(expected), Cursor::new(actual), &wrong_count).is_err());
    }
}
