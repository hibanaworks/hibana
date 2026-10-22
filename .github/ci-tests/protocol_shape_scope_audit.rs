//! Check that every source-planned borrowed-scope probe ran and was rejected.
use super::{Json, json, source_json};
use std::{collections::BTreeSet, fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn case_name(case: &Json) -> Result<&str> {
    match case.get("name") {
        Some(Json::Number(name)) if name.parse::<usize>().is_ok() => Ok(name),
        _ => Err("invalid borrowed-scope case name".into()),
    }
}

fn probes(case: &Json) -> Result<&[Json]> {
    match case.get("probes") {
        Some(Json::Array(records)) => Ok(records),
        _ => Err("invalid borrowed-scope probe inventory".into()),
    }
}

fn check_case(expected: &Json, actual: &Json) -> Result<usize> {
    if actual != expected {
        return Err("missing, duplicate, changed or accepted borrowed-scope probe".into());
    }
    let records = probes(actual)?;
    if records
        .iter()
        .any(|record| record.get("accepted") != Some(&Json::Bool(false)))
    {
        return Err("accepted borrowed-scope probe".into());
    }
    Ok(records.len())
}

fn controls(case: &Json) -> Result<()> {
    let records = probes(case)?;
    let first = records
        .first()
        .ok_or("empty borrowed-scope control source")?;
    let encoded = source_json(case)?;
    let changed = encoded.replacen("\"accepted\":false", "\"accepted\":true", 1);
    let false_admission = json::parse(&changed)?;
    if check_case(case, &false_admission).is_ok() {
        return Err("borrowed-scope false admission was not detected".into());
    }
    for items in [
        records.iter().skip(1).collect::<Vec<_>>(),
        records
            .iter()
            .chain(core::iter::once(first))
            .collect::<Vec<_>>(),
    ] {
        let altered = json::parse(&format!(
            r#"{{"name":{},"probes":[{}]}}"#,
            case_name(case)?,
            items
                .into_iter()
                .map(source_json)
                .collect::<Result<Vec<_>>>()?
                .join(",")
        ))?;
        if check_case(case, &altered).is_ok() {
            return Err("borrowed-scope missing or duplicate probe was not detected".into());
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
    if source_names != expected_names || expected_names.len() != cases.len() {
        return Err("missing, duplicate or unknown borrowed-scope source".into());
    }
    let directory = output.join("scope-observations");
    let actual_files = fs::read_dir(&directory)?
        .map(|entry| {
            let entry = entry?;
            Ok(entry
                .file_name()
                .to_str()
                .ok_or("invalid scope observation filename")?
                .to_owned())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual_files
        != expected_names
            .iter()
            .map(|name| format!("{name}.json"))
            .collect()
    {
        return Err("missing or unknown borrowed-scope observation file".into());
    }
    let mut checked = 0;
    for case in cases {
        let name = case_name(case)?;
        let actual = json::parse(&fs::read_to_string(directory.join(format!("{name}.json")))?)?;
        checked += check_case(case, &actual)?;
    }
    println!(
        "Borrowed-scope probes: cases={} attempts={checked} all rejected",
        cases.len()
    );
    if let Some(case) = cases
        .iter()
        .find(|case| probes(case).is_ok_and(|records| !records.is_empty()))
    {
        controls(case)?;
        println!(
            "Borrowed-scope checker controls: false-admission, missing-probe, duplicate-probe: pass"
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
        let case=json::parse(r#"{"name":7,"probes":[{"actor":1,"occurrence":3,"accepted":false},{"actor":1,"occurrence":7,"accepted":false}]}"#).unwrap();
        assert_eq!(check_case(&case, &case).unwrap(), 2);
        controls(&case).unwrap();
        let wrong_visit=json::parse(r#"{"name":7,"probes":[{"actor":1,"occurrence":2,"accepted":false},{"actor":1,"occurrence":7,"accepted":false}]}"#).unwrap();
        assert!(check_case(&case, &wrong_visit).is_err());
    }
}
